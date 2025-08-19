use crate::native::{self, WindowInfo};
use eframe::{
    Renderer,
    egui::{
        self, Atom, AtomExt, Color32, ColorImage, Direction, FontData, FontDefinitions, FontFamily,
        FontId, IconData, Image, Layout, Margin, RichText, TextStyle, Theme, Vec2,
    },
};
use image::{GenericImageView, ImageFormat, ImageReader};
use std::thread;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use std::{io::Cursor, mem};
use windows_capture::{
    capture::{CaptureControl, Context, GraphicsCaptureApiHandler},
    frame::Frame,
    monitor::Monitor,
    settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    },
};

enum CaptureWorkerEvent {
    Capture(Monitor),
    StopCapture,
}

#[derive(Debug)]
enum InjectorWorkerEvent {
    Update,
    PerformOp(u32, u32, bool, Option<bool>),
}

struct ScreenCapture {
    capture_send: crossbeam_channel::Sender<ColorImage>,
}

impl GraphicsCaptureApiHandler for ScreenCapture {
    type Flags = crossbeam_channel::Sender<ColorImage>;

    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(ScreenCapture {
            capture_send: ctx.flags,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: windows_capture::graphics_capture_api::InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if self.capture_send.is_full() {
            return Ok(());
        }

        let width = frame.width();
        let height = frame.height();
        if let Ok(mut buffer) = frame.buffer() {
            if let Ok(no_pad_buffer) = buffer.as_nopadding_buffer() {
                let img = ColorImage::from_rgba_unmultiplied(
                    [width as usize, height as usize],
                    &no_pad_buffer,
                );
                let _ = self.capture_send.try_send(img);
            }
        }
        Ok(())
    }
}

struct Gui {
    monitors: Vec<Monitor>,
    windows: Arc<Mutex<Vec<WindowInfo>>>,
    event_sender: crossbeam_channel::Sender<InjectorWorkerEvent>,
    capture_event_send: crossbeam_channel::Sender<CaptureWorkerEvent>,
    capture_recv: crossbeam_channel::Receiver<ColorImage>,
    capture_tex: Option<egui::TextureHandle>,
    hide_from_taskbar: bool,
    show_desktop_preview: bool,
    active_monitor: usize,
    icon_cache: HashMap<(u32, u32), Option<egui::TextureHandle>>,
}

impl Gui {
    fn new() -> Gui {
        let windows = Arc::new(Mutex::new(Vec::new()));
        let windows_copy = windows.clone();

        let (sender, receiver) = crossbeam_channel::unbounded();

        thread::spawn(move || {
            for event in receiver {
                match event {
                    InjectorWorkerEvent::Update => {
                        println!("populating");
                        let mut w = native::get_top_level_windows();
                        *windows_copy.lock().unwrap() = mem::take(&mut w);
                        println!("populating done");
                    }
                    InjectorWorkerEvent::PerformOp(pid, hwnd, hide_window, hide_from_taskbar) => {
                        println!("performing on op on {:?}", hwnd);
                        if let Err(error) = native::Injector::set_window_props_with_pid(
                            pid,
                            hwnd,
                            hide_window,
                            hide_from_taskbar,
                        ) {
                            println!("Failed: {:?}", error);
                        }
                    }
                }
            }
        });

        let (capture_send, capture_recv) = crossbeam_channel::bounded(1);
        let (capture_event_send, capture_event_recv) = crossbeam_channel::unbounded();
        thread::spawn(move || {
            let mut active_capture_control: Option<CaptureControl<_, _>> = None;
            for event in capture_event_recv.iter() {
                if let Some(capture_control) = active_capture_control {
                    // TODO: handle error here?
                    let _ = capture_control.stop();
                    active_capture_control = None;
                }

                match event {
                    CaptureWorkerEvent::Capture(monitor) => {
                        let settings = Settings::new(
                            monitor,
                            CursorCaptureSettings::Default,
                            DrawBorderSettings::Default,
                            SecondaryWindowSettings::Default,
                            MinimumUpdateIntervalSettings::Default,
                            DirtyRegionSettings::Default,
                            ColorFormat::Rgba8,
                            capture_send.clone(),
                        );

                        if let Ok(capture_control) = ScreenCapture::start_free_threaded(settings) {
                            active_capture_control = Some(capture_control);
                        }
                    }
                    CaptureWorkerEvent::StopCapture => (),
                }
            }
        });

        let monitors = Monitor::enumerate().unwrap_or_default();

        if monitors.len() > 0 {
            let _ = capture_event_send.send(CaptureWorkerEvent::Capture(monitors[0]));
        }

        Gui {
            show_desktop_preview: monitors.len() > 0,
            monitors,
            windows,
            event_sender: sender,
            capture_event_send,
            capture_recv,
            capture_tex: None,
            hide_from_taskbar: false,
            active_monitor: 0,
            icon_cache: HashMap::new(),
        }
    }

    fn get_icon<'a>(
        icon_cache: &'a mut HashMap<(u32, u32), Option<egui::TextureHandle>>,
        ctx: &egui::Context,
        pid: u32,
        hwnd: u32,
    ) -> &'a Option<egui::TextureHandle> {
        if !icon_cache.contains_key(&(pid, hwnd)) {
            let icon = match native::get_icon(hwnd) {
                Some((width, height, buffer)) => {
                    let image = ColorImage::from_rgba_unmultiplied([width, height], &buffer);
                    Some(ctx.load_texture("icon", image, egui::TextureOptions::LINEAR))
                }
                None => None,
            };

            icon_cache.insert((pid, hwnd), icon);
        }

        icon_cache.get(&(pid, hwnd)).unwrap()
    }

    fn add_section_header(
        ui: &mut egui::Ui,
        theme: Theme,
        header: impl Into<String>,
        desc: impl Into<String>,
    ) {
        let (header_color, desc_color) = match theme {
            Theme::Light => (
                Color32::from_rgb(34, 34, 34),
                Color32::from_rgb(119, 119, 119),
            ),
            Theme::Dark => (
                Color32::from_rgb(242, 242, 242),
                Color32::from_rgb(148, 148, 148),
            ),
        };

        ui.label(RichText::new(header).heading().color(header_color));
        ui.label(RichText::new(desc).color(desc_color));
        ui.add_space(8.0);
    }

    fn handle_hide_on_taskbar_change(&self) {
        for window_info in self.windows.lock().unwrap().iter_mut() {
            if !window_info.hidden {
                continue;
            }

            let event = InjectorWorkerEvent::PerformOp(
                window_info.pid,
                window_info.hwnd,
                true,
                Some(self.hide_from_taskbar),
            );
            self.event_sender.send(event).unwrap();
        }
    }
}

impl eframe::App for Gui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // graceful close
        if ctx.input(|i| i.viewport().close_requested()) {
            let _ = self
                .capture_event_send
                .send(CaptureWorkerEvent::StopCapture);
            return;
        }

        // check if focus has changed
        for event in ctx.input(|i| i.events.clone()) {
            if let egui::Event::WindowFocused(focused) = event {
                if focused {
                    println!("focused");
                    self.event_sender.send(InjectorWorkerEvent::Update).unwrap();
                    if self.show_desktop_preview {
                        let _ = self.capture_event_send.send(CaptureWorkerEvent::Capture(
                            self.monitors[self.active_monitor],
                        ));
                    }
                } else {
                    let _ = self
                        .capture_event_send
                        .send(CaptureWorkerEvent::StopCapture);
                }
            }
        }

        let theme = ctx.theme();
        let focused = ctx.input(|i| i.focused);

        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(Margin::same(14)))
            .show(ctx, |ui| {
                if !focused {
                    ui.with_layout(
                        Layout::centered_and_justified(Direction::LeftToRight),
                        |ui| {
                            ui.label(":)");
                        },
                    );
                    return;
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    if self.show_desktop_preview {
                        Self::add_section_header(
                            ui,
                            theme,
                            "Preview",
                            "How others will see your screen",
                        );

                        if let Ok(img) = self.capture_recv.try_recv() {
                            if let Some(texture_handle) = &mut self.capture_tex {
                                texture_handle.set(img, egui::TextureOptions::LINEAR);
                            } else {
                                self.capture_tex = Some(ctx.load_texture(
                                    "screen_capture",
                                    img,
                                    egui::TextureOptions::LINEAR,
                                ));
                            }
                            ctx.request_repaint();
                        }

                        if let Some(tex) = &self.capture_tex {
                            ui.add(egui::Image::from_texture(tex).shrink_to_fit());
                        }

                        if self.monitors.len() > 1 {
                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
                                for (i, monitor) in self.monitors.iter().enumerate() {
                                    let monitor_label = ui.selectable_label(
                                        i == self.active_monitor,
                                        format!("Screen {}", i + 1),
                                    );
                                    if monitor_label.clicked() && self.active_monitor != i {
                                        self.active_monitor = i;
                                        let _ = self
                                            .capture_event_send
                                            .send(CaptureWorkerEvent::Capture(*monitor));
                                    }
                                }
                            });
                        }

                        ui.add_space(14.0);
                    }

                    Self::add_section_header(
                        ui,
                        theme,
                        "Hide applications",
                        "Select the windows to hide",
                    );

                    for window_info in self.windows.lock().unwrap().iter_mut() {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate); // elide with "…"

                        let icon_atom = if let Some(texture) = Gui::get_icon(
                            &mut self.icon_cache,
                            ctx,
                            window_info.pid,
                            window_info.hwnd,
                        ) {
                            Image::from_texture(texture)
                                .max_height(16.0)
                                .atom_max_width(16.0)
                        } else {
                            Atom::grow().atom_size(Vec2::new(16.0, 0.0))
                        };

                        let checkbox_label = (
                            Atom::grow().atom_size(Vec2::new(0.0, 0.0)),
                            icon_atom,
                            Atom::grow().atom_size(Vec2::new(0.0, 0.0)),
                            &window_info.title,
                        );

                        let checkbox_response =
                            ui.checkbox(&mut window_info.hidden, checkbox_label);
                        if checkbox_response.changed() {
                            let hide_from_taskbar = match self.hide_from_taskbar {
                                true => Some(window_info.hidden),
                                false => None,
                            };

                            let event = InjectorWorkerEvent::PerformOp(
                                window_info.pid,
                                window_info.hwnd,
                                window_info.hidden,
                                hide_from_taskbar,
                            );
                            self.event_sender.send(event).unwrap();
                        }
                        ui.add_space(2.0);
                    }
                    ui.add_space(10.0);
                    ui.collapsing("Advanced settings", |ui| {
                        let taskbar_checkbox_response = ui
                            .checkbox(&mut self.hide_from_taskbar, "Hide from Alt+Tab and Taskbar");

                        if taskbar_checkbox_response.changed() {
                            self.handle_hide_on_taskbar_change();
                        }

                        let preview_checkbox_response =
                            ui.checkbox(&mut self.show_desktop_preview, "Show desktop preview");
                        if preview_checkbox_response.changed() {
                            let event = if self.show_desktop_preview {
                                CaptureWorkerEvent::Capture(self.monitors[self.active_monitor])
                            } else {
                                self.capture_tex = None;
                                CaptureWorkerEvent::StopCapture
                            };
                            self.capture_event_send.send(event).unwrap();
                        }
                    });
                });
            });
    }
}

pub fn start() {
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 540.0]),
        renderer: Renderer::Wgpu,
        ..Default::default()
    };

    // load icon
    if let Ok(d_image) = ImageReader::with_format(
        Cursor::new(include_bytes!("../../Misc/invicon.ico")),
        ImageFormat::Ico,
    )
    .decode()
    {
        let (width, height) = d_image.dimensions();
        options.viewport = options.viewport.with_icon(Arc::new(IconData {
            rgba: d_image.into_rgba8().into_raw(),
            width,
            height,
        }));
    }

    eframe::run_native(
        "Invisiwind",
        options,
        Box::new(|cc| {
            // This gives us image support:
            egui_extras::install_image_loaders(&cc.egui_ctx);

            let mut fonts = FontDefinitions::default();

            fonts.font_data.insert(
                "Inter_18pt-Regular".to_owned(),
                Arc::new(FontData::from_static(include_bytes!(
                    "../../Misc/fonts/Inter_18pt-Regular.ttf"
                ))),
            );

            fonts.families.insert(
                FontFamily::Name("Inter_18pt-Regular".into()),
                vec!["Inter_18pt-Regular".to_owned()],
            );

            fonts.font_data.insert(
                "Inter_18pt-Bold".to_owned(),
                Arc::new(FontData::from_static(include_bytes!(
                    "../../Misc/fonts/Inter_18pt-Bold.ttf"
                ))),
            );

            fonts.families.insert(
                FontFamily::Name("Inter_18pt-Bold".into()),
                vec!["Inter_18pt-Bold".to_owned()],
            );

            cc.egui_ctx.set_fonts(fonts);

            // cc.egui_ctx.set_theme(Theme::Light);

            cc.egui_ctx.all_styles_mut(|style| {
                // no rounded checkboxes
                style.visuals.widgets.inactive.corner_radius = Default::default();
                style.visuals.widgets.hovered.corner_radius = Default::default();
                style.visuals.widgets.active.corner_radius = Default::default();

                // we don't want strokes around checkboxes
                style.visuals.widgets.hovered.bg_stroke = Default::default();
                style.visuals.widgets.active.bg_stroke = Default::default();

                // we don't want checkboxes or collapsibles to expand on hover/active state
                style.visuals.widgets.hovered.expansion = 0.0;
                style.visuals.widgets.active.expansion = 0.0;

                // do not allow text to be selected
                style.interaction.selectable_labels = false;

                let mut text_styles = style.text_styles.clone();
                text_styles.insert(
                    TextStyle::Body,
                    FontId {
                        size: 12.0,
                        family: egui::FontFamily::Name("Inter_18pt-Regular".into()),
                    },
                );

                text_styles.insert(
                    TextStyle::Heading,
                    FontId {
                        size: 16.0,
                        family: egui::FontFamily::Name("Inter_18pt-Bold".into()),
                    },
                );
                style.text_styles = text_styles;
            });

            Ok(Box::new(Gui::new()))
        }),
    )
    .expect("Failed to create window");
}

use crate::injector::{self, WindowInfo};
use eframe::{
    Renderer,
    egui::{self, ColorImage, Direction, IconData, Layout},
};
use image::{GenericImageView, ImageFormat, ImageReader};
use std::sync::{Arc, Mutex};
use std::thread;
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
    CAPTURE(Monitor),
    NONE,
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

#[derive(Debug)]
enum InjectorWorkerEvent {
    UPDATE,
    HIDE(u32, u32, bool),
    SHOW(u32, u32, bool),
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
}

impl Gui {
    fn new() -> Gui {
        let windows = Arc::new(Mutex::new(Vec::new()));
        let windows_copy = windows.clone();

        let (sender, receiver) = crossbeam_channel::unbounded();

        thread::spawn(move || {
            for event in receiver {
                match event {
                    InjectorWorkerEvent::UPDATE => {
                        println!("populating");
                        let mut w = injector::get_top_level_windows();
                        *windows_copy.lock().unwrap() = mem::take(&mut w);
                        println!("populating done");
                    }
                    InjectorWorkerEvent::HIDE(pid, hwnd, show_on_taskbar) => {
                        println!("wanna hide {:?}", hwnd);
                        injector::set_window_props_with_pid(pid, hwnd, true, show_on_taskbar);
                    }
                    InjectorWorkerEvent::SHOW(pid, hwnd, show_on_taskbar) => {
                        println!("wanna show {:?}", hwnd);
                        injector::set_window_props_with_pid(pid, hwnd, false, show_on_taskbar);
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
                    CaptureWorkerEvent::CAPTURE(monitor) => {
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
                    CaptureWorkerEvent::NONE => (),
                }
            }
        });

        let monitors = Monitor::enumerate().unwrap_or_default();

        if monitors.len() > 0 {
            let _ = capture_event_send.send(CaptureWorkerEvent::CAPTURE(monitors[0]));
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
        }
    }
}

impl eframe::App for Gui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (events, focused) = ctx.input(|i| (i.events.clone(), i.focused));

        for event in events {
            if let egui::Event::WindowFocused(focused) = event {
                if focused {
                    println!("focused");
                    self.event_sender.send(InjectorWorkerEvent::UPDATE).unwrap();
                    if self.show_desktop_preview {
                        let _ = self.capture_event_send.send(CaptureWorkerEvent::CAPTURE(
                            self.monitors[self.active_monitor],
                        ));
                    }
                } else {
                    let _ = self.capture_event_send.send(CaptureWorkerEvent::NONE);
                }
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
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
                    ui.heading("Desktop Preview");
                    ui.add_space(4.0);

                    if self.monitors.len() > 1 {
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
                                        .send(CaptureWorkerEvent::CAPTURE(*monitor));
                                }
                            }
                        });
                        ui.add_space(4.0);
                    }

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
                    ui.add_space(8.0);
                }

                ui.heading("Hide applications");
                ui.add_space(4.0);
                for window_info in self.windows.lock().unwrap().iter_mut() {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate); // elide with "…"
                    let checkbox_response =
                        ui.checkbox(&mut window_info.hidden, &window_info.title);
                    if checkbox_response.changed() {
                        let event = if window_info.hidden {
                            InjectorWorkerEvent::HIDE(
                                window_info.pid,
                                window_info.hwnd,
                                self.hide_from_taskbar,
                            )
                        } else {
                            InjectorWorkerEvent::SHOW(
                                window_info.pid,
                                window_info.hwnd,
                                self.hide_from_taskbar,
                            )
                        };
                        self.event_sender.send(event).unwrap();
                    }
                }
                ui.add_space(10.0);
                ui.collapsing("Advanced settings", |ui| {
                    ui.checkbox(&mut self.hide_from_taskbar, "Hide from Alt+Tab and Taskbar");
                    let preview_checkbox_response =
                        ui.checkbox(&mut self.show_desktop_preview, "Show desktop preview");
                    if preview_checkbox_response.changed() {
                        let event = if self.show_desktop_preview {
                            CaptureWorkerEvent::CAPTURE(self.monitors[self.active_monitor])
                        } else {
                            self.capture_tex = None;
                            CaptureWorkerEvent::NONE
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

            Ok(Box::new(Gui::new()))
        }),
    )
    .expect("Failed to create window");
}

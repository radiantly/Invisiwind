use crate::injector::{self, WindowInfo};
use eframe::{
    Renderer,
    egui::{self, ColorImage, IconData},
};
use image::{GenericImageView, ImageFormat, ImageReader};
use std::sync::{Arc, Mutex};
use std::thread;
use std::{io::Cursor, mem};
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

struct ScreenCapture {
    capture_sender: crossbeam_channel::Sender<ColorImage>,
}

impl GraphicsCaptureApiHandler for ScreenCapture {
    type Flags = crossbeam_channel::Sender<ColorImage>;

    type Error = Box<dyn std::error::Error + Send + Sync>;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(ScreenCapture {
            capture_sender: ctx.flags,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _capture_control: windows_capture::graphics_capture_api::InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        let width = frame.width();
        let height = frame.height();
        if let Ok(mut buffer) = frame.buffer() {
            if let Ok(no_pad_buffer) = buffer.as_nopadding_buffer() {
                let img = ColorImage::from_rgba_unmultiplied(
                    [width as usize, height as usize],
                    &no_pad_buffer,
                );
                let _ = self.capture_sender.try_send(img);
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
enum WorkerEvents {
    UPDATE,
    HIDE(u32, u32, bool),
    SHOW(u32, u32, bool),
}

struct Gui {
    windows: Arc<Mutex<Vec<WindowInfo>>>,
    event_sender: crossbeam_channel::Sender<WorkerEvents>,
    capture_receiver: crossbeam_channel::Receiver<ColorImage>,
    hide_from_taskbar: bool,
    capture_tex: Option<egui::TextureHandle>,
}

impl Gui {
    fn new() -> Gui {
        let windows = Arc::new(Mutex::new(Vec::new()));
        let windowst = windows.clone();

        let (sender, receiver) = crossbeam_channel::unbounded();

        thread::spawn(move || {
            for event in receiver {
                match event {
                    WorkerEvents::UPDATE => {
                        println!("populating");
                        let mut w = injector::get_top_level_windows();
                        *windowst.lock().unwrap() = mem::take(&mut w);
                        println!("populating done");
                    }
                    WorkerEvents::HIDE(pid, hwnd, show_on_taskbar) => {
                        println!("wanna hide {:?}", hwnd);
                        injector::set_window_props_with_pid(pid, hwnd, true, show_on_taskbar);
                    }
                    WorkerEvents::SHOW(pid, hwnd, show_on_taskbar) => {
                        println!("wanna show {:?}", hwnd);
                        injector::set_window_props_with_pid(pid, hwnd, false, show_on_taskbar);
                    }
                }
            }
        });

        let (capture_sender, capture_receiver) = crossbeam_channel::bounded(1);

        thread::spawn(move || {
            let primary_monitor = Monitor::primary().expect("There is no primary monitor");

            let settings = Settings::new(
                // Item to capture
                primary_monitor,
                // Capture cursor settings
                CursorCaptureSettings::Default,
                // Draw border settings
                DrawBorderSettings::Default,
                // Secondary window settings, if you want to include secondary windows in the capture
                SecondaryWindowSettings::Default,
                // Minimum update interval, if you want to change the frame rate limit (default is 60 FPS or 16.67 ms)
                MinimumUpdateIntervalSettings::Default,
                // Dirty region settings,
                DirtyRegionSettings::Default,
                // The desired color format for the captured frame.
                ColorFormat::Rgba8,
                // Additional flags for the capture settings that will be passed to the user-defined `new` function.
                capture_sender,
            );

            ScreenCapture::start(settings).expect("screen capture failed");
        });

        Gui {
            windows,
            event_sender: sender,
            capture_receiver,
            hide_from_taskbar: false,
            capture_tex: None,
        }
    }
}

impl eframe::App for Gui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let events = ctx.input(|i| i.events.clone());

        for event in events {
            if let egui::Event::WindowFocused(focused) = event {
                // `focused` is a bool: true=gained focus, false=lost focus
                if focused {
                    println!("focused");
                    self.event_sender.send(WorkerEvents::UPDATE).unwrap();
                } else {
                    println!("unfocused");
                }
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Desktop Preview");
                ui.add_space(4.0);

                if let Ok(img) = self.capture_receiver.try_recv() {
                    if let Some(texture_handle) = &mut self.capture_tex {
                        texture_handle.set(img, egui::TextureOptions::default());
                    } else {
                        self.capture_tex = Some(ctx.load_texture(
                            "screen_capture",
                            img,
                            egui::TextureOptions::LINEAR, // or NEAREST if you want crisp pixels
                        ));
                    }
                }

                if let Some(tex) = &self.capture_tex {
                    // Show at native size:
                    ui.add(egui::Image::from_texture(tex).shrink_to_fit());
                    // or force a size (e.g. scale to 512x512):
                    // ui.image((tex.id(), egui::vec2(512.0, 512.0)));
                }
                ui.add_space(8.0);

                ui.heading("Hide applications");
                ui.add_space(4.0);
                for window_info in self.windows.lock().unwrap().iter_mut() {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate); // elide with "…"
                    let checkbox_response =
                        ui.checkbox(&mut window_info.hidden, &window_info.title);
                    if checkbox_response.changed() {
                        let event = if window_info.hidden {
                            WorkerEvents::HIDE(
                                window_info.pid,
                                window_info.hwnd,
                                self.hide_from_taskbar,
                            )
                        } else {
                            WorkerEvents::SHOW(
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
                    ui.checkbox(&mut self.hide_from_taskbar, "Hide from Alt+Tab and Taskbar")
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

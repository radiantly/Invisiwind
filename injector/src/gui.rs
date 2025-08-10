use crate::injector::{self, WindowInfo};
use eframe::egui::{self, IconData};
use image::{self, GenericImageView};
use std::mem;
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Debug)]
enum WorkerEvents {
    UPDATE,
    HIDE(u32, u32, bool),
    SHOW(u32, u32, bool),
}

#[derive(Debug)]
struct Gui {
    windows: Arc<Mutex<Vec<WindowInfo>>>,
    sender: crossbeam_channel::Sender<WorkerEvents>,
    hide_taskbar_icons: bool,
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

        Gui {
            windows,
            sender,
            hide_taskbar_icons: false,
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
                    self.sender.send(WorkerEvents::UPDATE).unwrap();
                } else {
                    println!("unfocused");
                }
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Hide applications");
            ui.add_space(4.0);

            egui::ScrollArea::vertical().show(ui, |ui| {
                for window_info in self.windows.lock().unwrap().iter_mut() {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate); // elide with "…"
                    let checkbox_response =
                        ui.checkbox(&mut window_info.hidden, &window_info.title);
                    if checkbox_response.changed() {
                        let event = if window_info.hidden {
                            WorkerEvents::HIDE(
                                window_info.pid,
                                window_info.hwnd,
                                !self.hide_taskbar_icons,
                            )
                        } else {
                            WorkerEvents::SHOW(
                                window_info.pid,
                                window_info.hwnd,
                                !self.hide_taskbar_icons,
                            )
                        };
                        self.sender.send(event).unwrap();
                    }
                }
                ui.add_space(10.0);
                ui.collapsing("Advanced settings", |ui| {
                    ui.checkbox(
                        &mut self.hide_taskbar_icons,
                        "Hide from Alt+Tab and Taskbar",
                    )
                });
            });
        });
    }
}

pub fn start() {
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([320.0, 540.0]),
        ..Default::default()
    };

    // load icon
    if let Ok(d_image) = image::open("../Misc/invicon.ico") {
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

//! Native macOS mapper UI (egui/eframe).
//!
//! Create controller→keyboard mappings visually, watch live input, save/load
//! named profiles, and start/stop injection — all from one window.
//!
//!     cargo run --bin app
//!
//! Needs Accessibility permission (System Settings > Privacy & Security >
//! Accessibility) for the app to actually send keys while "Running".

use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use eframe::egui;
use powera_driver::controller::Controller;
use powera_driver::mapping::{
    self, active_keys, key_name, ControlId, Injector, Profile, KEYS,
};
use powera_driver::GamepadState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

/// State shared between the UI thread and the controller/injection thread.
struct Shared {
    live: Mutex<GamepadState>,
    profile: Mutex<Profile>,
    status: Mutex<String>,
    running: AtomicBool,
    alive: AtomicBool,
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([700.0, 780.0])
            .with_title("PowerA Mapper"),
        ..Default::default()
    };
    eframe::run_native(
        "PowerA Mapper",
        options,
        Box::new(|_cc| Ok(Box::new(MapperApp::new()))),
    )
}

struct MapperApp {
    shared: Arc<Shared>,
    profile: Profile, // local working copy, mirrored into shared each frame
    save_name: String,
    profiles: Vec<String>,
    load_pick: Option<String>,
    toast: String,
}

impl MapperApp {
    fn new() -> Self {
        // Make sure mappings/ exists with the bundled profiles, then start on
        // retrogames.cc if present (the layout in use), else the default.
        mapping::ensure_defaults();
        let profile = mapping::load_profile("retrogames.cc")
            .unwrap_or_else(|_| Profile::default_mapping());
        let shared = Arc::new(Shared {
            live: Mutex::new(GamepadState::default()),
            profile: Mutex::new(profile.clone()),
            status: Mutex::new("starting…".to_string()),
            running: AtomicBool::new(false),
            alive: AtomicBool::new(true),
        });
        spawn_worker(shared.clone());
        MapperApp {
            save_name: profile.name.clone(),
            profile,
            shared,
            profiles: mapping::list_profiles(),
            load_pick: None,
            toast: String::new(),
        }
    }
}

/// The controller/injection thread: owns the device and the event source.
fn spawn_worker(shared: Arc<Shared>) {
    thread::spawn(move || {
        let source = match CGEventSource::new(CGEventSourceStateID::HIDSystemState) {
            Ok(s) => s,
            Err(_) => {
                *shared.status.lock().unwrap() = "failed to create event source".into();
                return;
            }
        };
        let mut inj = Injector::new(source);
        let mut controller: Option<Controller> = None;
        // Remember the last (state, running) we acted on so we can skip the
        // (re)compute when the controller is sending identical reports.
        let mut last: Option<(GamepadState, bool)> = None;

        while shared.alive.load(Ordering::SeqCst) {
            // (Re)open the controller if needed.
            if controller.is_none() {
                match Controller::open() {
                    Ok(c) => {
                        controller = Some(c);
                        *shared.status.lock().unwrap() = "controller connected".into();
                    }
                    Err(e) => {
                        *shared.status.lock().unwrap() = e;
                        thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                }
            }

            let c = controller.as_mut().unwrap();
            match c.read(8) {
                Ok(Some(s)) => *shared.live.lock().unwrap() = s,
                Ok(None) => {}
                Err(e) => {
                    *shared.status.lock().unwrap() = format!("read error: {e}");
                    inj.release_all();
                    controller = None;
                    continue;
                }
            }

            let state = *shared.live.lock().unwrap();
            let running = shared.running.load(Ordering::SeqCst);

            // Only touch the injector when the state or running flag changed —
            // the controller streams identical reports at rest, and reprocessing
            // each one is what burned idle CPU.
            if last != Some((state, running)) {
                if running {
                    let prof = shared.profile.lock().unwrap().clone();
                    inj.apply(active_keys(&prof, &state));
                } else {
                    inj.release_all();
                }
                last = Some((state, running));
            }

            // When we're not injecting there's no need to drain the endpoint at
            // its full ~kHz rate; ease off to keep idle CPU low. Live input still
            // updates fast enough for the UI's indicators.
            if !running {
                thread::sleep(Duration::from_millis(8));
            }
        }
        inj.release_all();
    });
}

impl eframe::App for MapperApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let live = *self.shared.live.lock().unwrap();
        let status = self.shared.status.lock().unwrap().clone();
        let running = self.shared.running.load(Ordering::SeqCst);
        let trusted = unsafe { AXIsProcessTrusted() };

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let (label, color) = if running {
                    ("⏹ Stop", egui::Color32::from_rgb(0xd9, 0x53, 0x4f))
                } else {
                    ("▶ Start", egui::Color32::from_rgb(0x35, 0xc7, 0x59))
                };
                if ui.add(egui::Button::new(label).fill(color)).clicked() {
                    self.shared.running.store(!running, Ordering::SeqCst);
                }
                ui.separator();
                ui.label(format!("Status: {status}"));
            });
            ui.colored_label(
                egui::Color32::from_rgb(0x5a, 0xb0, 0xe0),
                "ⓘ Works in 2.4 GHz mode only: set the back switch to “2.4 RF” and plug in the \
                 dongle. The wired cable and Bluetooth are not supported on macOS.",
            );
            if !trusted {
                ui.colored_label(
                    egui::Color32::from_rgb(0xe0, 0xa0, 0x30),
                    "⚠ Not trusted for Accessibility — keys won't be sent. \
                     Grant this app in System Settings › Privacy & Security › Accessibility.",
                );
            }
            ui.add_space(4.0);
        });

        egui::TopBottomPanel::top("profile_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Profile:");
                ui.text_edit_singleline(&mut self.save_name);
                if ui.button("💾 Save").clicked() {
                    self.profile.name = self.save_name.clone();
                    match mapping::save_profile(&self.profile) {
                        Ok(p) => {
                            self.toast = format!("Saved to {}", p.display());
                            self.profiles = mapping::list_profiles();
                        }
                        Err(e) => self.toast = format!("Save failed: {e}"),
                    }
                }
                egui::ComboBox::from_id_salt("load")
                    .selected_text(self.load_pick.clone().unwrap_or_else(|| "Load…".into()))
                    .show_ui(ui, |ui| {
                        for name in &self.profiles {
                            if ui
                                .selectable_label(false, name)
                                .clicked()
                            {
                                match mapping::load_profile(name) {
                                    Ok(p) => {
                                        self.save_name = p.name.clone();
                                        self.load_pick = Some(p.name.clone());
                                        self.profile = p;
                                        self.toast = "Loaded.".into();
                                    }
                                    Err(e) => self.toast = format!("Load failed: {e}"),
                                }
                            }
                        }
                    });
                if ui.button("New").clicked() {
                    self.profile = Profile::default_mapping();
                    self.save_name = "my mapping".to_string();
                }
                if ui.button("Clear all").clicked() {
                    self.profile.bindings.clear();
                }
            });
            ui.horizontal(|ui| {
                // Sharing: export current profile / import someone else's file.
                if ui.button("⬆ Export…").clicked() {
                    self.profile.name = self.save_name.clone();
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("JSON mapping", &["json"])
                        .set_file_name(format!("{}.json", self.profile.name))
                        .save_file()
                    {
                        match mapping::export_profile(&self.profile, &path) {
                            Ok(()) => self.toast = format!("Exported to {}", path.display()),
                            Err(e) => self.toast = format!("Export failed: {e}"),
                        }
                    }
                }
                if ui.button("⬇ Import…").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("JSON mapping", &["json"])
                        .pick_file()
                    {
                        match mapping::import_profile(&path) {
                            Ok(p) => {
                                self.save_name = p.name.clone();
                                self.profile = p;
                                self.toast = format!("Imported {}", path.display());
                            }
                            Err(e) => self.toast = format!("Import failed: {e}"),
                        }
                    }
                }
                ui.label(
                    egui::RichText::new("Save = into mappings/ • Export/Import = share files")
                        .weak(),
                );
            });
            if !self.toast.is_empty() {
                ui.label(egui::RichText::new(&self.toast).weak());
            }
            ui.add_space(4.0);
        });

        egui::TopBottomPanel::bottom("tuning").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Deadzone");
                ui.add(egui::Slider::new(&mut self.profile.deadzone, 0.0..=0.4));
                ui.label("Stick threshold");
                ui.add(egui::Slider::new(&mut self.profile.walk_threshold, 0.1..=0.95));
                ui.label("Trigger");
                ui.add(egui::Slider::new(&mut self.profile.trigger_threshold, 1..=254));
            });
            // Live stick read-out.
            let (lx, ly) = live.left_stick_norm(self.profile.deadzone);
            let (rx, ry) = live.right_stick_norm(self.profile.deadzone);
            ui.label(format!(
                "Live: L({lx:+.2},{ly:+.2}) R({rx:+.2},{ry:+.2})  LT {} RT {}",
                live.left_trigger, live.right_trigger
            ));
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.label("Click a key for each control. The dot lights up when you press it.");
            ui.add_space(4.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                egui::Grid::new("controls")
                    .num_columns(3)
                    .striped(true)
                    .spacing([16.0, 6.0])
                    .show(ui, |ui| {
                        for &id in ControlId::ALL {
                            let active = id.is_active(
                                &live,
                                self.profile.deadzone,
                                self.profile.walk_threshold,
                                self.profile.trigger_threshold,
                            );
                            let dot = if active {
                                egui::Color32::from_rgb(0x35, 0xc7, 0x59)
                            } else {
                                egui::Color32::from_gray(70)
                            };
                            ui.colored_label(dot, "⬤");
                            ui.label(id.label());

                            let mut sel = self.profile.bindings.get(&id).copied();
                            let text = sel
                                .map(key_name)
                                .unwrap_or_else(|| "(none)".to_string());
                            egui::ComboBox::from_id_salt(id.label())
                                .selected_text(text)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut sel, None, "(none)");
                                    for (name, code) in KEYS {
                                        ui.selectable_value(&mut sel, Some(*code), *name);
                                    }
                                });
                            match sel {
                                Some(code) => {
                                    self.profile.bindings.insert(id, code);
                                }
                                None => {
                                    self.profile.bindings.remove(&id);
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
        });

        // Mirror the working profile to the worker thread.
        *self.shared.profile.lock().unwrap() = self.profile.clone();

        // Repaint smoothly only while focused (so live dots feel responsive when
        // binding). In the background — i.e. while you're playing the game — the
        // window barely repaints; key injection happens in the worker thread and
        // doesn't depend on rendering, so input is unaffected.
        let focused = ctx.input(|i| i.focused);
        let interval = if focused { 33 } else { 500 };
        ctx.request_repaint_after(Duration::from_millis(interval));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shared.alive.store(false, Ordering::SeqCst);
        self.shared.running.store(false, Ordering::SeqCst);
        // Give the worker a moment to release any held keys.
        thread::sleep(Duration::from_millis(60));
    }
}

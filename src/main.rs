use eframe::egui;
use serde::{Deserialize, Serialize};
use std::sync::mpsc;
use std::thread;
use xshell::{Shell, cmd};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Settings {
    log_command: String,
    refresh_interval: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            log_command: "journalctl -f".to_string(),
            refresh_interval: 1000,
        }
    }
}

struct LogEntry {
    timestamp: String,
    content: String,
}

struct LogsApp {
    settings: Settings,
    logs: Vec<LogEntry>,
    filter_text: String,
    search_text: String,
    auto_scroll: bool,
    show_settings: bool,
    log_receiver: Option<mpsc::Receiver<String>>,
    log_thread_handle: Option<thread::JoinHandle<()>>,
    settings_changed: bool,
}

impl Default for LogsApp {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            logs: Vec::new(),
            filter_text: String::new(),
            search_text: String::new(),
            auto_scroll: true,
            show_settings: false,
            log_receiver: None,
            log_thread_handle: None,
            settings_changed: false,
        }
    }
}

impl LogsApp {
    fn start_log_collection(&mut self) {
        if self.log_thread_handle.is_some() {
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.log_receiver = Some(rx);

        let command = self.settings.log_command.clone();

        let handle = thread::spawn(move || {
            let sh = Shell::new().unwrap();

            let parts: Vec<&str> = command.split_whitespace().collect();
            if parts.is_empty() {
                return;
            }

            let program = parts[0];
            let args = &parts[1..];

            let result = if args.is_empty() {
                cmd!(sh, "{program}").read()
            } else {
                let mut cmd_builder = cmd!(sh, "{program}");
                for arg in args {
                    cmd_builder = cmd_builder.arg(arg);
                }
                cmd_builder.read()
            };

            if let Ok(output) = result {
                for line in output.lines() {
                    if tx.send(line.to_string()).is_err() {
                        break;
                    }
                }
            }
        });

        self.log_thread_handle = Some(handle);
    }

    fn stop_log_collection(&mut self) {
        self.log_receiver = None;
        if let Some(handle) = self.log_thread_handle.take() {
            // Don't block the UI - let the thread finish naturally
            std::mem::drop(handle);
        }
    }

    fn restart_log_collection(&mut self) {
        self.stop_log_collection();
        self.logs.clear();
        self.start_log_collection();
    }

    fn add_log_entry(&mut self, content: String) {
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.logs.push(LogEntry { timestamp, content });

        if self.logs.len() > 10000 {
            self.logs.drain(0..1000);
        }
    }

    fn filtered_logs(&self) -> Vec<&LogEntry> {
        self.logs
            .iter()
            .filter(|entry| {
                let matches_filter = if self.filter_text.is_empty() {
                    true
                } else {
                    entry
                        .content
                        .to_lowercase()
                        .contains(&self.filter_text.to_lowercase())
                };

                let matches_search = if self.search_text.is_empty() {
                    true
                } else {
                    entry
                        .content
                        .to_lowercase()
                        .contains(&self.search_text.to_lowercase())
                        || entry
                            .timestamp
                            .to_lowercase()
                            .contains(&self.search_text.to_lowercase())
                };

                matches_filter && matches_search
            })
            .collect()
    }
}

impl eframe::App for LogsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut new_logs = Vec::new();
        if let Some(receiver) = &self.log_receiver {
            while let Ok(log_line) = receiver.try_recv() {
                new_logs.push(log_line);
            }
        }

        for log_line in new_logs {
            self.add_log_entry(log_line);
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(
            self.settings.refresh_interval,
        ));

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Settings").clicked() {
                        self.show_settings = !self.show_settings;
                    }
                    if ui.button("Clear Logs").clicked() {
                        self.logs.clear();
                    }
                    if ui.button("Restart Collection").clicked() {
                        self.restart_log_collection();
                    }
                });

                ui.separator();

                ui.label("Filter:");
                ui.text_edit_singleline(&mut self.filter_text);

                ui.separator();

                ui.label("Search:");
                ui.text_edit_singleline(&mut self.search_text);

                ui.separator();

                ui.checkbox(&mut self.auto_scroll, "Auto-scroll");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Logs: {}", self.logs.len()));
                });
            });
        });

        let mut show_settings = self.show_settings;
        let mut apply_settings = false;
        let mut reset_settings = false;

        if show_settings {
            egui::Window::new("Settings")
                .open(&mut show_settings)
                .show(ctx, |ui| {
                    ui.label("Log Command:");
                    if ui
                        .text_edit_singleline(&mut self.settings.log_command)
                        .changed()
                    {
                        self.settings_changed = true;
                    }

                    ui.label("Refresh Interval (ms):");
                    if ui
                        .add(egui::Slider::new(
                            &mut self.settings.refresh_interval,
                            100..=5000,
                        ))
                        .changed()
                    {
                        self.settings_changed = true;
                    }

                    ui.horizontal(|ui| {
                        if ui.button("Apply").clicked() && self.settings_changed {
                            apply_settings = true;
                        }

                        if ui.button("Reset to Default").clicked() {
                            reset_settings = true;
                        }
                    });

                    ui.separator();
                    ui.label("Examples:");
                    ui.label("• journalctl -f");
                    ui.label("• tail -f /var/log/syslog");
                    ui.label("• docker logs -f container_name");
                    ui.label("• kubectl logs -f pod_name");
                });
        }

        self.show_settings = show_settings;

        if apply_settings {
            self.restart_log_collection();
            self.settings_changed = false;
        }

        if reset_settings {
            self.settings = Settings::default();
            self.restart_log_collection();
            self.settings_changed = false;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            let filtered_logs = self.filtered_logs();

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(self.auto_scroll)
                .show(ui, |ui| {
                    egui::Grid::new("log_grid").striped(true).show(ui, |ui| {
                        for log_entry in filtered_logs {
                            ui.label(&log_entry.timestamp);
                            ui.label(&log_entry.content);
                            ui.end_row();
                        }
                    });
                });
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("Logs Viewer"),
        ..Default::default()
    };

    let mut app = LogsApp::default();
    app.start_log_collection();

    eframe::run_native("Logs Viewer", options, Box::new(|_cc| Ok(Box::new(app))))
}

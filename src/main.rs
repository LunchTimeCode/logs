use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::mpsc;
use std::thread;
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Settings {
    log_command: String,
    refresh_interval: u64,
}

#[derive(Debug, Clone, PartialEq)]
enum FilterMode {
    IncludeSelected,
    ExcludeSelected,
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
    selected_log_levels: HashSet<String>,
    filter_mode: FilterMode,
    search_text: String,
    auto_scroll: bool,
    show_settings: bool,
    log_receiver: Option<mpsc::Receiver<String>>,
    log_thread_handle: Option<thread::JoinHandle<()>>,
    settings_changed: bool,
    current_level_filter: String,
}

impl Default for LogsApp {
    fn default() -> Self {
        let mut selected_log_levels = HashSet::new();
        selected_log_levels.insert("trace".to_string());
        selected_log_levels.insert("debug".to_string());
        selected_log_levels.insert("info".to_string());
        selected_log_levels.insert("warn".to_string());
        selected_log_levels.insert("warning".to_string());
        selected_log_levels.insert("error".to_string());
        selected_log_levels.insert("err".to_string());
        selected_log_levels.insert("fatal".to_string());
        selected_log_levels.insert("critical".to_string());
        selected_log_levels.insert("crit".to_string());
        
        Self {
            settings: Settings::default(),
            logs: Vec::new(),
            selected_log_levels,
            filter_mode: FilterMode::IncludeSelected,
            search_text: String::new(),
            auto_scroll: true,
            show_settings: false,
            log_receiver: None,
            log_thread_handle: None,
            settings_changed: false,
            current_level_filter: "All Levels".to_string(),
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
            let parts: Vec<&str> = command.split_whitespace().collect();
            if parts.is_empty() {
                return;
            }

            let program = parts[0];
            let args = &parts[1..];

            let mut cmd = Command::new(program);
            cmd.args(args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            if let Ok(mut child) = cmd.spawn() {
                if let Some(stdout) = child.stdout.take() {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines() {
                        match line {
                            Ok(line_content) => {
                                if tx.send(line_content).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
                
                // Clean up the child process
                let _ = child.wait();
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
                let matches_filter = if self.selected_log_levels.is_empty() {
                    true
                } else {
                    let content_lower = entry.content.to_lowercase();
                    
                    let contains_selected_level = self.selected_log_levels.iter().any(|level| {
                        content_lower.contains(&level.to_lowercase())
                    });
                    
                    match self.filter_mode {
                        FilterMode::IncludeSelected => contains_selected_level,
                        FilterMode::ExcludeSelected => !contains_selected_level,
                    }
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

                ui.label("Command:");
                ui.add(egui::TextEdit::singleline(&mut self.settings.log_command).desired_width(200.0));
                if ui.button("Apply").clicked() {
                    self.restart_log_collection();
                }

                ui.separator();

                ui.label("Log Level Filter:");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_label("Level")
                        .selected_text(&self.current_level_filter)
                        .show_ui(ui, |ui| {
                            let levels = [
                                ("All Levels", "All Levels"),
                                ("TRACE", "trace"),
                                ("DEBUG", "debug"), 
                                ("INFO", "info"),
                                ("WARN", "warn"),
                                ("WARNING", "warning"),
                                ("ERROR", "error"),
                                ("ERR", "err"),
                                ("FATAL", "fatal"),
                                ("CRITICAL", "critical"),
                                ("CRIT", "crit"),
                            ];
                            
                            for (display_name, level_key) in levels {
                                if ui.selectable_value(&mut self.current_level_filter, display_name.to_string(), display_name).clicked() {
                                    self.selected_log_levels.clear();
                                    if level_key != "All Levels" {
                                        self.selected_log_levels.insert(level_key.to_string());
                                    }
                                }
                            }
                        });
                    
                    ui.separator();
                    ui.label("Mode:");
                    ui.radio_value(&mut self.filter_mode, FilterMode::IncludeSelected, "Include");
                    ui.radio_value(&mut self.filter_mode, FilterMode::ExcludeSelected, "Exclude");
                });

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

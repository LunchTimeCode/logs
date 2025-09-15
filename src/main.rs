use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::mpsc;
use std::thread;
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};
use std::fs;
use std::path::PathBuf;
use chrono::{NaiveDateTime, NaiveDate, NaiveTime, Duration, Local, Datelike};
use regex::Regex;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FavoriteCommand {
    name: String,
    command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Settings {
    log_command: String,
    refresh_interval: u64,
    favorite_commands: Vec<FavoriteCommand>,
}

#[derive(Debug, Clone, PartialEq)]
enum FilterMode {
    IncludeSelected,
    ExcludeSelected,
}

#[derive(Debug, Clone, PartialEq)]
enum TimeSpanMode {
    Disabled,
    Predefined(PredefinedSpan),
    Custom,
    Relative,
}

#[derive(Debug, Clone, PartialEq)]
enum PredefinedSpan {
    Last15Minutes,
    Last30Minutes,
    Last1Hour,
    Last6Hours,
    Last24Hours,
    Last3Days,
    Last1Week,
    Last1Month,
}

#[derive(Debug, Clone, PartialEq)]
enum TimeUnit {
    Minutes,
    Hours,
    Days,
}

impl PredefinedSpan {
    fn display_name(&self) -> &'static str {
        match self {
            PredefinedSpan::Last15Minutes => "Last 15 minutes",
            PredefinedSpan::Last30Minutes => "Last 30 minutes",
            PredefinedSpan::Last1Hour => "Last 1 hour",
            PredefinedSpan::Last6Hours => "Last 6 hours",
            PredefinedSpan::Last24Hours => "Last 24 hours",
            PredefinedSpan::Last3Days => "Last 3 days",
            PredefinedSpan::Last1Week => "Last 1 week",
            PredefinedSpan::Last1Month => "Last 1 month",
        }
    }

    fn to_duration(&self) -> Duration {
        match self {
            PredefinedSpan::Last15Minutes => Duration::minutes(15),
            PredefinedSpan::Last30Minutes => Duration::minutes(30),
            PredefinedSpan::Last1Hour => Duration::hours(1),
            PredefinedSpan::Last6Hours => Duration::hours(6),
            PredefinedSpan::Last24Hours => Duration::days(1),
            PredefinedSpan::Last3Days => Duration::days(3),
            PredefinedSpan::Last1Week => Duration::weeks(1),
            PredefinedSpan::Last1Month => Duration::days(30),
        }
    }
}

impl TimeUnit {
    fn display_name(&self) -> &'static str {
        match self {
            TimeUnit::Minutes => "minutes",
            TimeUnit::Hours => "hours",
            TimeUnit::Days => "days",
        }
    }

    fn to_duration(&self, amount: i64) -> Duration {
        match self {
            TimeUnit::Minutes => Duration::minutes(amount),
            TimeUnit::Hours => Duration::hours(amount),
            TimeUnit::Days => Duration::days(amount),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            log_command: "journalctl -f".to_string(),
            refresh_interval: 1000,
            favorite_commands: Vec::new(),
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
    show_favorites: bool,
    new_favorite_name: String,
    favorite_search_text: String,
    editing_favorite_index: Option<usize>,
    edit_favorite_name: String,
    edit_favorite_command: String,
    time_span_mode: TimeSpanMode,
    custom_from_year: i32,
    custom_from_month: u32,
    custom_from_day: u32,
    custom_from_hour: u32,
    custom_from_minute: u32,
    custom_to_year: i32,
    custom_to_month: u32,
    custom_to_day: u32,
    custom_to_hour: u32,
    custom_to_minute: u32,
    relative_amount: i32,
    relative_unit: TimeUnit,
    is_loading: bool,
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
        
        let now = Local::now().naive_local();
        
        Self {
            settings: Self::load_settings(),
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
            show_favorites: false,
            new_favorite_name: String::new(),
            favorite_search_text: String::new(),
            editing_favorite_index: None,
            edit_favorite_name: String::new(),
            edit_favorite_command: String::new(),
            time_span_mode: TimeSpanMode::Disabled,
            custom_from_year: now.year(),
            custom_from_month: now.month(),
            custom_from_day: now.day(),
            custom_from_hour: 0,
            custom_from_minute: 0,
            custom_to_year: now.year(),
            custom_to_month: now.month(),
            custom_to_day: now.day(),
            custom_to_hour: 23,
            custom_to_minute: 59,
            relative_amount: 1,
            relative_unit: TimeUnit::Hours,
            is_loading: false,
        }
    }
}

impl LogsApp {
    fn get_config_path() -> PathBuf {
        let mut path = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        path.push("logs-viewer");
        path.push("settings.json");
        path
    }

    fn load_settings() -> Settings {
        let config_path = Self::get_config_path();
        if let Ok(content) = fs::read_to_string(&config_path) {
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            Settings::default()
        }
    }

    fn save_settings(&self) {
        let config_path = Self::get_config_path();
        if let Some(parent) = config_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(content) = serde_json::to_string_pretty(&self.settings) {
            let _ = fs::write(&config_path, content);
        }
    }

    fn add_favorite_command(&mut self, name: String, command: String) {
        self.settings.favorite_commands.push(FavoriteCommand { name, command });
        self.save_settings();
    }

    fn remove_favorite_command(&mut self, index: usize) {
        if index < self.settings.favorite_commands.len() {
            self.settings.favorite_commands.remove(index);
            self.save_settings();
        }
    }

    fn update_favorite_command(&mut self, index: usize, name: String, command: String) {
        if index < self.settings.favorite_commands.len() {
            self.settings.favorite_commands[index].name = name;
            self.settings.favorite_commands[index].command = command;
            self.save_settings();
        }
    }

    fn apply_favorite_command(&mut self, command: String) {
        self.settings.log_command = command;
        self.restart_log_collection();
    }

    fn get_time_range(&self) -> Option<(NaiveDateTime, NaiveDateTime)> {
        match &self.time_span_mode {
            TimeSpanMode::Disabled => None,
            TimeSpanMode::Predefined(span) => {
                let now = Local::now().naive_local();
                let duration = span.to_duration();
                let from = now - duration;
                Some((from, now))
            },
            TimeSpanMode::Custom => {
                let from = NaiveDate::from_ymd_opt(
                    self.custom_from_year,
                    self.custom_from_month,
                    self.custom_from_day,
                )?.and_time(NaiveTime::from_hms_opt(
                    self.custom_from_hour,
                    self.custom_from_minute,
                    0,
                )?);
                
                let to = NaiveDate::from_ymd_opt(
                    self.custom_to_year,
                    self.custom_to_month,
                    self.custom_to_day,
                )?.and_time(NaiveTime::from_hms_opt(
                    self.custom_to_hour,
                    self.custom_to_minute,
                    59,
                )?);
                
                Some((from, to))
            },
            TimeSpanMode::Relative => {
                let now = Local::now().naive_local();
                let duration = self.relative_unit.to_duration(self.relative_amount as i64);
                let from = now - duration;
                Some((from, now))
            },
        }
    }

    fn parse_time_input(input: &str) -> Option<NaiveDateTime> {
        if input.trim().is_empty() {
            return None;
        }

        let trimmed = input.trim();
        
        // Try full format first: "2025-09-15 12:23:30"
        if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S") {
            return Some(dt);
        }
        
        // Try date + hour:minute: "2025-09-15 12:23"
        if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M") {
            return Some(dt);
        }
        
        // Try date + hour: "2025-09-15 12"
        if let Ok(dt) = NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H") {
            return Some(dt);
        }
        
        // Try just date: "2025-09-15"
        if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
            return Some(date.and_time(NaiveTime::from_hms_opt(0, 0, 0)?));
        }
        
        None
    }

    fn extract_timestamp_from_log(content: &str) -> (Option<String>, String) {
        // Common timestamp patterns in logs
        let patterns = [
            // ISO 8601 with milliseconds: "2025-09-15T14:30:00.123Z"
            (r"(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{3})?Z?)", "%Y-%m-%dT%H:%M:%S%.3fZ"),
            // Standard datetime: "2025-09-15 14:30:00.123"
            (r"(\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}(?:\.\d{3})?)", "%Y-%m-%d %H:%M:%S%.3f"),
            // Standard datetime without milliseconds: "2025-09-15 14:30:00"
            (r"(\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2})", "%Y-%m-%d %H:%M:%S"),
            // Syslog format: "Sep 15 14:30:00" or "Sep  5 14:30:00"
            (r"([A-Za-z]{3}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2})", "%b %d %H:%M:%S"),
            // Time only with milliseconds: "14:30:00.123"
            (r"(\d{2}:\d{2}:\d{2}\.\d{3})", "%H:%M:%S%.3f"),
            // Time only: "14:30:00"
            (r"(\d{2}:\d{2}:\d{2})", "%H:%M:%S"),
            // Date with slashes: "09/15/2025 14:30:00"
            (r"(\d{2}/\d{2}/\d{4}\s+\d{2}:\d{2}:\d{2})", "%m/%d/%Y %H:%M:%S"),
            // Unix timestamp (10 digits): "1726401000"
            (r"(\d{10})", "unix"),
        ];

        for (pattern, format) in &patterns {
            if let Ok(re) = Regex::new(pattern) {
                if let Some(captures) = re.captures(content) {
                    if let Some(timestamp_match) = captures.get(1) {
                        let timestamp_str = timestamp_match.as_str();
                        
                        // Parse the timestamp
                        let parsed_timestamp = if *format == "unix" {
                            // Handle Unix timestamp
                            if let Ok(unix_ts) = timestamp_str.parse::<i64>() {
                                chrono::DateTime::from_timestamp(unix_ts, 0)
                                    .map(|dt| dt.naive_local())
                            } else {
                                None
                            }
                        } else if format.contains("%b") {
                            // Handle syslog format - need to add current year
                            let current_year = Local::now().year();
                            let with_year = format!("{current_year} {timestamp_str}");
                            NaiveDateTime::parse_from_str(&with_year, &format!("%Y {format}")).ok()
                        } else {
                            // Handle other formats
                            NaiveDateTime::parse_from_str(timestamp_str, format).ok()
                        };

                        if let Some(dt) = parsed_timestamp {
                            let formatted_timestamp = dt.format("%Y-%m-%d %H:%M:%S").to_string();
                            // Remove the timestamp from content to avoid duplication
                            let cleaned_content = content.replace(timestamp_str, "").trim().to_string();
                            return (Some(formatted_timestamp), cleaned_content);
                        }
                    }
                }
            }
        }

        // No timestamp found, return original content
        (None, content.to_string())
    }

    fn start_log_collection(&mut self) {
        if self.log_thread_handle.is_some() {
            return;
        }

        let (tx, rx) = mpsc::channel();
        self.log_receiver = Some(rx);
        self.is_loading = true;

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
        self.is_loading = false;
        self.start_log_collection();
    }

    fn add_log_entry(&mut self, content: String) {
        let (extracted_timestamp, cleaned_content) = Self::extract_timestamp_from_log(&content);
        
        let timestamp = extracted_timestamp.unwrap_or_else(|| {
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
        });
        
        self.logs.push(LogEntry { 
            timestamp, 
            content: cleaned_content 
        });

        // Set loading to false when we receive the first log entry
        if self.is_loading {
            self.is_loading = false;
        }

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

                let matches_time = if let Some((from_time, to_time)) = self.get_time_range() {
                    let entry_time = Self::parse_time_input(&entry.timestamp);
                    
                    if let Some(entry_dt) = entry_time {
                        entry_dt >= from_time && entry_dt <= to_time
                    } else {
                        true
                    }
                } else {
                    true
                };

                matches_filter && matches_search && matches_time
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
                    if ui.button("Favorites").clicked() {
                        self.show_favorites = !self.show_favorites;
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
                if ui.button("⭐").on_hover_text("Save as favorite").clicked() {
                    self.new_favorite_name = format!("Command {}", self.settings.favorite_commands.len() + 1);
                    self.show_favorites = true;
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

                ui.label("Time Filter:");
                ui.horizontal(|ui| {
                    egui::ComboBox::from_label("Time Span")
                        .selected_text(match &self.time_span_mode {
                            TimeSpanMode::Disabled => "Disabled",
                            TimeSpanMode::Predefined(span) => span.display_name(),
                            TimeSpanMode::Custom => "Custom Range",
                            TimeSpanMode::Relative => "Relative Time",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.time_span_mode, TimeSpanMode::Disabled, "Disabled");
                            ui.separator();
                            
                            ui.selectable_value(&mut self.time_span_mode, TimeSpanMode::Predefined(PredefinedSpan::Last15Minutes), "Last 15 minutes");
                            ui.selectable_value(&mut self.time_span_mode, TimeSpanMode::Predefined(PredefinedSpan::Last30Minutes), "Last 30 minutes");
                            ui.selectable_value(&mut self.time_span_mode, TimeSpanMode::Predefined(PredefinedSpan::Last1Hour), "Last 1 hour");
                            ui.selectable_value(&mut self.time_span_mode, TimeSpanMode::Predefined(PredefinedSpan::Last6Hours), "Last 6 hours");
                            ui.selectable_value(&mut self.time_span_mode, TimeSpanMode::Predefined(PredefinedSpan::Last24Hours), "Last 24 hours");
                            ui.selectable_value(&mut self.time_span_mode, TimeSpanMode::Predefined(PredefinedSpan::Last3Days), "Last 3 days");
                            ui.selectable_value(&mut self.time_span_mode, TimeSpanMode::Predefined(PredefinedSpan::Last1Week), "Last 1 week");
                            ui.selectable_value(&mut self.time_span_mode, TimeSpanMode::Predefined(PredefinedSpan::Last1Month), "Last 1 month");
                            ui.separator();
                            
                            ui.selectable_value(&mut self.time_span_mode, TimeSpanMode::Custom, "Custom Range");
                            ui.selectable_value(&mut self.time_span_mode, TimeSpanMode::Relative, "Relative Time");
                        });
                });

                match &self.time_span_mode {
                    TimeSpanMode::Custom => {
                        ui.horizontal(|ui| {
                            ui.label("From:");
                            ui.add(egui::DragValue::new(&mut self.custom_from_year).range(2000..=2100).prefix("Year: "));
                            ui.add(egui::DragValue::new(&mut self.custom_from_month).range(1..=12).prefix("Month: "));
                            ui.add(egui::DragValue::new(&mut self.custom_from_day).range(1..=31).prefix("Day: "));
                            ui.add(egui::DragValue::new(&mut self.custom_from_hour).range(0..=23).prefix("Hour: "));
                            ui.add(egui::DragValue::new(&mut self.custom_from_minute).range(0..=59).prefix("Min: "));
                        });
                        ui.horizontal(|ui| {
                            ui.label("To:");
                            ui.add(egui::DragValue::new(&mut self.custom_to_year).range(2000..=2100).prefix("Year: "));
                            ui.add(egui::DragValue::new(&mut self.custom_to_month).range(1..=12).prefix("Month: "));
                            ui.add(egui::DragValue::new(&mut self.custom_to_day).range(1..=31).prefix("Day: "));
                            ui.add(egui::DragValue::new(&mut self.custom_to_hour).range(0..=23).prefix("Hour: "));
                            ui.add(egui::DragValue::new(&mut self.custom_to_minute).range(0..=59).prefix("Min: "));
                        });
                    },
                    TimeSpanMode::Relative => {
                        ui.horizontal(|ui| {
                            ui.label("Last");
                            ui.add(egui::DragValue::new(&mut self.relative_amount).range(1..=999));
                            egui::ComboBox::from_label("")
                                .selected_text(self.relative_unit.display_name())
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut self.relative_unit, TimeUnit::Minutes, "minutes");
                                    ui.selectable_value(&mut self.relative_unit, TimeUnit::Hours, "hours");
                                    ui.selectable_value(&mut self.relative_unit, TimeUnit::Days, "days");
                                });
                        });
                    },
                    _ => {}
                }

                ui.separator();

                ui.checkbox(&mut self.auto_scroll, "Auto-scroll");

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Logs: {}", self.logs.len()));
                });
            });
        });

        let mut show_settings = self.show_settings;
        let mut show_favorites = self.show_favorites;
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

        if show_favorites {
            let mut save_new_favorite = false;
            let mut favorite_to_remove: Option<usize> = None;
            let mut favorite_to_apply: Option<String> = None;
            let mut save_edit: Option<usize> = None;
            let mut cancel_edit = false;
            let mut start_edit: Option<usize> = None;

            egui::Window::new("Favorite Commands")
                .open(&mut show_favorites)
                .show(ctx, |ui| {
                    ui.heading("Save Current Command");
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(&mut self.new_favorite_name);
                        if ui.button("Save").clicked() && !self.new_favorite_name.trim().is_empty() {
                            save_new_favorite = true;
                        }
                    });

                    ui.separator();
                    ui.heading("Favorite Commands");

                    ui.horizontal(|ui| {
                        ui.label("Search:");
                        ui.text_edit_singleline(&mut self.favorite_search_text);
                        if ui.button("Clear").clicked() {
                            self.favorite_search_text.clear();
                        }
                    });

                    if self.settings.favorite_commands.is_empty() {
                        ui.label("No favorite commands saved yet.");
                    } else {
                        let filtered_favorites: Vec<(usize, &FavoriteCommand)> = self.settings.favorite_commands
                            .iter()
                            .enumerate()
                            .filter(|(_, favorite)| {
                                if self.favorite_search_text.is_empty() {
                                    true
                                } else {
                                    let search_lower = self.favorite_search_text.to_lowercase();
                                    favorite.name.to_lowercase().contains(&search_lower) ||
                                    favorite.command.to_lowercase().contains(&search_lower)
                                }
                            })
                            .collect();

                        if filtered_favorites.is_empty() {
                            ui.label("No matching favorite commands found.");
                        } else {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                for (index, favorite) in filtered_favorites {
                                    ui.horizontal(|ui| {
                                        if ui.button("Use").clicked() {
                                            favorite_to_apply = Some(favorite.command.clone());
                                        }
                                        
                                        // Check if this item is being edited
                                        if let Some(edit_index) = self.editing_favorite_index {
                                            if edit_index == index {
                                                // Show editable fields
                                                ui.label("Name:");
                                                ui.text_edit_singleline(&mut self.edit_favorite_name);
                                                ui.label("Command:");
                                                ui.text_edit_singleline(&mut self.edit_favorite_command);
                                                
                                                if ui.button("Save").clicked() {
                                                    save_edit = Some(index);
                                                }
                                                if ui.button("Cancel").clicked() {
                                                    cancel_edit = true;
                                                }
                                            } else {
                                                // Show read-only for other items
                                                ui.label(&favorite.name);
                                                ui.label(&favorite.command);
                                                ui.label("(editing another item)");
                                            }
                                        } else {
                                            // Show read-only with edit button
                                            ui.label(&favorite.name);
                                            ui.label(&favorite.command);
                                            
                                            if ui.button("📝").on_hover_text("Edit").clicked() {
                                                start_edit = Some(index);
                                            }
                                            if ui.button("📋").on_hover_text("Copy command").clicked() {
                                                ui.output_mut(|o| o.copied_text = favorite.command.clone());
                                            }
                                            if ui.button("🗑").on_hover_text("Delete").clicked() {
                                                favorite_to_remove = Some(index);
                                            }
                                        }
                                    });
                                }
                            });
                        }
                    }
                });

            if save_new_favorite {
                let name = self.new_favorite_name.trim().to_string();
                let command = self.settings.log_command.clone();
                self.add_favorite_command(name, command);
                self.new_favorite_name.clear();
            }

            if let Some(index) = favorite_to_remove {
                self.remove_favorite_command(index);
                // Cancel editing if we're deleting the item being edited
                if let Some(edit_index) = self.editing_favorite_index {
                    if edit_index == index {
                        self.editing_favorite_index = None;
                    } else if edit_index > index {
                        // Adjust the editing index if an item before it was deleted
                        self.editing_favorite_index = Some(edit_index - 1);
                    }
                }
            }

            if let Some(index) = start_edit {
                if index < self.settings.favorite_commands.len() {
                    self.editing_favorite_index = Some(index);
                    self.edit_favorite_name = self.settings.favorite_commands[index].name.clone();
                    self.edit_favorite_command = self.settings.favorite_commands[index].command.clone();
                }
            }

            if let Some(index) = save_edit {
                if !self.edit_favorite_name.trim().is_empty() && !self.edit_favorite_command.trim().is_empty() {
                    self.update_favorite_command(
                        index,
                        self.edit_favorite_name.trim().to_string(),
                        self.edit_favorite_command.trim().to_string(),
                    );
                    self.editing_favorite_index = None;
                    self.edit_favorite_name.clear();
                    self.edit_favorite_command.clear();
                }
            }

            if cancel_edit {
                self.editing_favorite_index = None;
                self.edit_favorite_name.clear();
                self.edit_favorite_command.clear();
            }

            if let Some(command) = favorite_to_apply {
                self.apply_favorite_command(command);
                show_favorites = false;
            }
        }

        self.show_settings = show_settings;
        self.show_favorites = show_favorites;

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
            if self.is_loading {
                // Show loading spinner when waiting for command output
                ui.with_layout(egui::Layout::centered_and_justified(egui::Direction::TopDown), |ui| {
                    ui.add_space(50.0);
                    
                    // Create a spinning loading icon
                    let time = ui.input(|i| i.time);
                    let spinner_angle = time as f32 * 2.0; // Rotate 2 radians per second
                    
                    let (rect, _response) = ui.allocate_exact_size(egui::Vec2::splat(40.0), egui::Sense::hover());
                    
                    if ui.is_rect_visible(rect) {
                        let painter = ui.painter();
                        let center = rect.center();
                        let radius = 15.0;
                        let stroke_width = 3.0;
                        
                        // Draw spinning arc
                        for i in 0..8 {
                            let angle = spinner_angle + (i as f32 * std::f32::consts::PI / 4.0);
                            let alpha = (1.0 - (i as f32 / 8.0)) * 0.8 + 0.2;
                            let color = egui::Color32::from_rgba_premultiplied(
                                (255.0 * alpha) as u8,
                                (255.0 * alpha) as u8,
                                (255.0 * alpha) as u8,
                                255
                            );
                            
                            let start = center + egui::Vec2::angled(angle) * (radius - stroke_width);
                            let end = center + egui::Vec2::angled(angle) * radius;
                            
                            painter.line_segment([start, end], egui::Stroke::new(stroke_width, color));
                        }
                    }
                    
                    ui.add_space(20.0);
                    ui.label("Loading logs...");
                    ui.label(format!("Running: {}", self.settings.log_command));
                });
            } else {
                // Show normal log display
                let filtered_logs = self.filtered_logs();

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(self.auto_scroll)
                    .show(ui, |ui| {
                        egui::Grid::new("log_grid")
                            .striped(true)
                            .spacing([10.0, 4.0])
                            .show(ui, |ui| {
                                // Table headers
                                ui.strong("Timestamp");
                                ui.strong("Log Content");
                                ui.end_row();
                                
                                // Add separator line
                                ui.separator();
                                ui.separator();
                                ui.end_row();
                                
                                // Log entries
                                for log_entry in filtered_logs {
                                    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                                        ui.add_sized([180.0, ui.available_height()], egui::Label::new(&log_entry.timestamp));
                                    });
                                    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |ui| {
                                        ui.label(&log_entry.content);
                                    });
                                    ui.end_row();
                                }
                            });
                    });
            }
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

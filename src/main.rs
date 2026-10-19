use anyhow::{Context, Result, bail};
use eframe::egui;
use std::path::PathBuf;
use trievium::clipboard::copy_text;
use trievium::config::{self, Config};
use trievium::shortcut_cache;
use trievium::trie::{Keymap, Lookup};

fn main() {
    if let Err(error) = run() {
        eprintln!("trievium: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut command = "pick".to_owned();
    let mut override_path = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "pick" | "init" | "validate" | "config-path" => command = arg,
            "--config" => {
                override_path = Some(PathBuf::from(
                    args.next().context("--config requires a path")?,
                ));
            }
            "--help" | "-h" => {
                println!(
                    "Trievium - keyboard-first text selection\n\nUsage: trievium [pick|init|validate|config-path] [--config PATH]\n\nDefault: pick. Keys are automatically assigned unless pinned in the JSON config."
                );
                return Ok(());
            }
            other => bail!("Unknown argument '{other}'. Run trievium --help"),
        }
    }
    let path = config::config_path(override_path)?;
    match command.as_str() {
        "init" => {
            config::initialize(&path)?;
            println!("Created {}. Edit it, then run trievium.", path.display());
        }
        "config-path" => println!("{}", path.display()),
        "validate" => {
            let config = Config::load(&path)?;
            let cache_path = shortcut_cache::path_for_config(&path)?;
            let previous = shortcut_cache::load(&cache_path);
            let map = Keymap::build_with_previous(&config.entries, &previous)?;
            for (index, entry) in config.entries.iter().enumerate() {
                println!("{:<5} {}", map.key(index), entry.label);
            }
        }
        _ => {
            let config = Config::load(&path)?;
            let cache_path = shortcut_cache::path_for_config(&path)?;
            let previous = shortcut_cache::load(&cache_path);
            let map = Keymap::build_with_previous(&config.entries, &previous)?;
            if let Err(error) = shortcut_cache::save(&cache_path, &config.entries, &map) {
                eprintln!("trievium: cannot save automatic key assignments: {error:#}");
            }
            let picker = Picker {
                config,
                map,
                prefix: String::new(),
                error: None,
            };
            let native = eframe::NativeOptions {
                renderer: eframe::Renderer::Glow,
                viewport: egui::ViewportBuilder::default()
                    .with_title("Trievium")
                    .with_app_id("trievium")
                    .with_inner_size([560.0, 450.0])
                    .with_min_inner_size([400.0, 250.0])
                    .with_decorations(false),
                ..Default::default()
            };
            eframe::run_native(
                "trievium",
                native,
                Box::new(move |_cc| Ok(Box::new(picker))),
            )
            .map_err(|err| anyhow::anyhow!("Cannot launch picker: {err}"))?;
        }
    }
    Ok(())
}

struct Picker {
    config: Config,
    map: Keymap,
    prefix: String,
    error: Option<String>,
}

impl Picker {
    fn select(&mut self, index: usize, ctx: &egui::Context) {
        match copy_text(&self.config.entries[index].text) {
            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn type_char(&mut self, ch: char, ctx: &egui::Context) -> bool {
        if !ch.is_ascii_alphabetic() {
            return false;
        }
        self.error = None;
        self.prefix.push(ch.to_ascii_uppercase());
        match self.map.lookup(&self.prefix) {
            Lookup::Selected(index) => {
                self.select(index, ctx);
                true
            }
            Lookup::Continue => false,
            Lookup::Invalid => {
                self.error = Some(format!(
                    "No selection matches '{}'. Backspace to correct.",
                    self.prefix
                ));
                false
            }
        }
    }
}

impl eframe::App for Picker {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let events = ui.input(|input| input.events.clone());
        for event in events {
            match event {
                egui::Event::Key {
                    key: egui::Key::Escape,
                    pressed: true,
                    ..
                } => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                egui::Event::Key {
                    key: egui::Key::Backspace,
                    pressed: true,
                    ..
                } => {
                    self.prefix.pop();
                    self.error = None;
                }
                egui::Event::Text(text) => {
                    for ch in text.chars() {
                        if self.type_char(ch, &ctx) {
                            return;
                        }
                    }
                }
                _ => {}
            }
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.heading("Trievium");
            ui.separator();
            ui.label(egui::RichText::new("type to select · Esc closes").weak());
        });
        ui.add_space(5.0);
        let prompt = if self.prefix.is_empty() {
            "Press a highlighted key".to_owned()
        } else {
            format!("Sequence: {}", self.prefix)
        };
        ui.label(egui::RichText::new(prompt).size(17.0).strong());
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(240, 126, 126), error);
        } else {
            ui.label(
                egui::RichText::new("A complete sequence copies immediately and closes.")
                    .small()
                    .weak(),
            );
        }
        ui.separator();

        let mut visible: Vec<usize> = self.map.matching(&self.prefix).collect();
        visible.sort_by(|&a, &b| self.map.key(a).cmp(self.map.key(b)));
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for index in visible {
                    let entry = &self.config.entries[index];
                    ui.horizontal(|ui| {
                        let sequence = self.map.key(index);
                        if ui
                            .add_sized(
                                [85.0, 31.0],
                                egui::Button::new(
                                    egui::RichText::new(sequence).monospace().strong(),
                                ),
                            )
                            .clicked()
                        {
                            clicked = Some(index);
                        }
                        if ui.selectable_label(false, &entry.label).clicked() {
                            clicked = Some(index);
                        }
                        ui.add_space(5.0);
                        let preview = entry.text.lines().next().unwrap_or("");
                        let preview: String = preview.chars().take(45).collect();
                        ui.label(egui::RichText::new(preview).weak().small());
                    });
                    ui.separator();
                }
            });
        if let Some(index) = clicked {
            self.select(index, &ctx);
        }
    }
}

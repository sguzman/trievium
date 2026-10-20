use anyhow::{Context, Result, bail};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::process::Command;
use trievium::clipboard::copy_text;
use trievium::config::{self, Config};
use trievium::hyprland;
use trievium::input::{InputAction, KeyboardInput, Selection};
use trievium::launch::{self, Instance};
use trievium::shortcut_cache;
use trievium::trie::Keymap;

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
            "pick" | "init" | "validate" | "config-path" | "doctor" => command = arg,
            "--config" => {
                override_path = Some(PathBuf::from(
                    args.next().context("--config requires a path")?,
                ));
            }
            "--version" | "-V" => {
                println!("trievium {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!(
                    "Trievium - keyboard-first text selection\n\nUsage: trievium [pick|init|validate|config-path|doctor] [--config PATH]\n\nDefault: pick. Keys are automatically assigned unless pinned in the JSON config."
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
        "doctor" => return doctor(&path),
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
            // Acquired lock must stay in scope until the native window closes.
            // A second hotkey invocation quietly avoids creating a second
            // picker, without killing or disturbing the existing process.
            let _instance_guard = match launch::acquire()? {
                Some(Instance::Busy) => return Ok(()),
                guard => guard,
            };
            let config = Config::load(&path)?;
            let cache_path = shortcut_cache::path_for_config(&path)?;
            let previous = shortcut_cache::load(&cache_path);
            let map = Keymap::build_with_previous(&config.entries, &previous)?;
            if let Err(error) = shortcut_cache::save(&cache_path, &config.entries, &map) {
                eprintln!("trievium: cannot save automatic key assignments: {error:#}");
            }
            let window_height =
                (125.0 + config.entries.len().min(9) as f32 * 39.0).clamp(245.0, 490.0);
            // The key order and matching list change only on prefix edits,
            // never on redraw. Keep the hot UI path proportional to visible rows.
            let mut ordered: Vec<usize> = (0..config.entries.len()).collect();
            ordered.sort_by(|&a, &b| map.key(a).cmp(map.key(b)));
            let mut picker = Picker {
                config,
                map,
                selection: Selection::default(),
                error: None,
                ordered,
                visible: Vec::new(),
                next_keys: String::new(),
                keyboard: KeyboardInput::default(),
            };
            picker.refresh_choices();
            let native = eframe::NativeOptions {
                renderer: eframe::Renderer::Glow,
                viewport: egui::ViewportBuilder::default()
                    .with_title("Trievium")
                    .with_app_id("trievium")
                    .with_inner_size([560.0, window_height])
                    .with_min_inner_size([400.0, 250.0])
                    .with_decorations(false),
                ..Default::default()
            };
            // Hyprland otherwise places a normal Wayland toplevel in the
            // tiling layout. Correct only this process's mapped window, and
            // do it asynchronously so the first egui frame is not blocked.
            hyprland::focus_new_popup(560, window_height.round() as i32);
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

/// Non-destructive preflight: no clipboard writes, no config creation.
fn doctor(path: &Path) -> Result<()> {
    let mut healthy = true;
    println!("Trievium environment check");
    if std::env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty()) {
        println!("[ok] WAYLAND_DISPLAY is set");
    } else {
        println!("[!!] No WAYLAND_DISPLAY in environment");
        healthy = false;
    }

    match Command::new("wl-copy").arg("--version").output() {
        Ok(result) if result.status.success() => println!("[ok] wl-copy is available"),
        _ => {
            println!("[!!] wl-copy missing or not runnable; install wl-clipboard");
            healthy = false;
        }
    }

    match Config::load(path).and_then(|config| {
        let previous_path = shortcut_cache::path_for_config(path)?;
        let previous = shortcut_cache::load(&previous_path);
        let map = Keymap::build_with_previous(&config.entries, &previous)?;
        Ok((config.entries.len(), map))
    }) {
        Ok((total, _)) => println!("[ok] Config and shortcut map valid ({total} entries)"),
        Err(error) => {
            println!("[!!] Config error: {error:#}");
            healthy = false;
        }
    }

    if std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|value| !value.is_empty()) {
        println!("[ok] XDG_RUNTIME_DIR is set (single-instance guard available)");
    } else {
        println!("[note] XDG_RUNTIME_DIR absent; picker runs without single-instance guard");
    }

    if healthy {
        println!(
            "Preflight passed. Window focus and clipboard persistence still require local testing."
        );
        Ok(())
    } else {
        bail!("Preflight failed. Correct the items marked [!!] above.")
    }
}

struct Picker {
    config: Config,
    map: Keymap,
    selection: Selection,
    error: Option<String>,
    ordered: Vec<usize>,
    visible: Vec<usize>,
    next_keys: String,
    keyboard: KeyboardInput,
}

impl Picker {
    fn refresh_choices(&mut self) {
        let prefix = self.selection.prefix();
        self.visible = self
            .ordered
            .iter()
            .copied()
            .filter(|&index| self.map.key(index).starts_with(prefix))
            .collect();
        let mut next: Vec<char> = self
            .visible
            .iter()
            .filter_map(|&index| self.map.key(index).chars().nth(prefix.len()))
            .collect();
        next.sort_unstable();
        next.dedup();
        self.next_keys = next.into_iter().collect();
    }

    fn select(&mut self, index: usize, ctx: &egui::Context) {
        match copy_text(&self.config.entries[index].text) {
            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(error) => {
                self.selection.clear();
                self.refresh_choices();
                self.error = Some(format!(
                    "Clipboard failed: {error}. Retry a shortcut or click an entry."
                ));
            }
        }
    }

    fn type_char(&mut self, ch: char, ctx: &egui::Context) -> bool {
        match self.selection.push(ch, &self.map) {
            trievium::input::SelectionOutcome::Selected(index) => {
                self.error = None;
                self.select(index, ctx);
                true
            }
            trievium::input::SelectionOutcome::Continue => {
                self.refresh_choices();
                self.error = None;
                false
            }
            trievium::input::SelectionOutcome::Invalid => {
                // Unassigned letters are harmless, not a failure.
                self.error = None;
                false
            }
        }
    }
}

impl eframe::App for Picker {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let (events, modifiers) = ui.input(|input| (input.events.clone(), input.modifiers));
        for action in self.keyboard.actions(&events, modifiers, std::time::Instant::now()) {
            match action {
                InputAction::Close => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                InputAction::Backspace => {
                    self.selection.backspace();
                    self.refresh_choices();
                    self.error = None;
                }
                InputAction::Letter(ch) => {
                    if self.type_char(ch, &ctx) {
                        return;
                    }
                }
                InputAction::Ignore => {}
            }
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.heading("Trievium");
            ui.separator();
            ui.label(egui::RichText::new("type to select · Esc closes").weak());
        });
        ui.add_space(5.0);
        if !ui.input(|input| input.focused) {
            ui.colored_label(
                egui::Color32::from_rgb(240, 168, 90),
                "Waiting for keyboard focus from the compositor...",
            );
        }
        let choices = self.next_keys.chars().map(|ch| ch.to_string()).collect::<Vec<_>>().join("  ");
        let prompt = if self.selection.prefix().is_empty() {
            format!("Press one of: {choices}")
        } else {
            format!("Sequence: {}   Next: {choices}", self.selection.prefix())
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

        if self.selection.prefix().is_empty() {
            ui.label(
                egui::RichText::new(format!("{} entries", self.visible.len()))
                    .small()
                    .weak(),
            );
        } else if !self.next_keys.is_empty() {
            ui.label(
                egui::RichText::new(format!(
                    "{} matches · next keys: {}",
                    self.visible.len(),
                    self.next_keys
                ))
                .small()
                .weak(),
            );
        }

        // Virtualize row painting, and rebuild the match list only when the
        // typed prefix changes rather than re-sorting on every redraw.
        let mut clicked = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, 39.0, self.visible.len(), |ui, rows| {
                for row in rows {
                    let index = self.visible[row];
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

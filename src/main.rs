use anyhow::{Context, Result, bail};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::process::Command;
use trievium::appearance::Appearance;
use trievium::clipboard::copy_text;
use trievium::config::{self, Config};
use trievium::hyprland;
use trievium::input::{TypedOutcome, resolve_typed};
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
    let mut diagnose_input = false;
    let mut appearance = Appearance::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "pick" | "init" | "validate" | "config-path" | "doctor" => command = arg,
            "--diagnose-input" => diagnose_input = true,
            "--theme" | "--color-mode" => {
                let mode = args
                    .next()
                    .context("--theme requires system, light, or dark")?;
                appearance = mode.parse()?;
            }
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
                    "Trievium - keyboard-first text selection\n\nUsage: trievium [pick|init|validate|config-path|doctor] [--config PATH] [--theme system|light|dark] [--diagnose-input]\n\nDefault: pick, with --theme system. Keys are automatically assigned unless pinned in the JSON config."
                );
                return Ok(());
            }
            other => bail!("Unknown argument '{other}'. Run trievium --help"),
        }
    }
    if diagnose_input && command != "pick" {
        bail!("--diagnose-input is only supported with the picker");
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
                Some(Instance::Busy) => {
                    eprintln!(
                        "trievium: another picker is already running. Close it before retrying."
                    );
                    return Ok(());
                }
                guard => guard,
            };
            let config = Config::load(&path)?;
            let cache_path = shortcut_cache::path_for_config(&path)?;
            let previous = shortcut_cache::load(&cache_path);
            let map = Keymap::build_with_previous(&config.entries, &previous)?;
            if let Err(error) = shortcut_cache::save(&cache_path, &config.entries, &map) {
                eprintln!("trievium: cannot save automatic key assignments: {error:#}");
            }
            // Only a title/input row and the actual selectable entries.
            let window_height =
                (56.0 + config.entries.len().min(9) as f32 * 39.0).clamp(175.0, 420.0);
            // The key order and matching list change only on prefix edits,
            // never on redraw. Keep the hot UI path proportional to visible rows.
            let mut ordered: Vec<usize> = (0..config.entries.len()).collect();
            ordered.sort_by(|&a, &b| map.key(a).cmp(map.key(b)));
            let mut picker = Picker {
                config,
                map,
                typed: String::new(),
                error: None,
                ordered,
                visible: Vec::new(),
                input_has_focus: None,
                diagnose_input,
                last_focus: None,
            };
            picker.refresh_choices();
            if diagnose_input {
                eprintln!("trievium diagnostics: active (no snippet text is recorded)");
                eprintln!(
                    "trievium diagnostics: app version {}",
                    env!("CARGO_PKG_VERSION")
                );
            }
            let native = eframe::NativeOptions {
                renderer: eframe::Renderer::Glow,
                viewport: egui::ViewportBuilder::default()
                    .with_title("Trievium")
                    .with_app_id("trievium")
                    .with_inner_size([560.0, window_height])
                    .with_min_inner_size([400.0, 160.0])
                    .with_decorations(false)
                    .with_resizable(false)
                    .with_transparent(false)
                    .with_mouse_passthrough(false)
                    .with_always_on_top(),
                ..Default::default()
            };
            // Hyprland otherwise places a normal Wayland toplevel in the
            // tiling layout. Correct only this process's mapped window, and
            // do it asynchronously so the first egui frame is not blocked.
            hyprland::focus_new_popup(560, window_height.round() as i32);
            eframe::run_native(
                "trievium",
                native,
                Box::new(move |cc| {
                    appearance.configure(&cc.egui_ctx);
                    Ok(Box::new(picker))
                }),
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
    typed: String,
    error: Option<String>,
    ordered: Vec<usize>,
    visible: Vec<usize>,
    input_has_focus: Option<bool>,
    diagnose_input: bool,
    last_focus: Option<bool>,
}

impl Picker {
    fn refresh_choices(&mut self) {
        let prefix = &self.typed;
        self.visible = self
            .ordered
            .iter()
            .copied()
            .filter(|&index| self.map.key(index).starts_with(prefix))
            .collect();
    }

    fn select(&mut self, index: usize) {
        if self.diagnose_input {
            eprintln!(
                "trievium diagnostics: shortcut {} selected; copying",
                self.map.key(index)
            );
        }
        match copy_text(&self.config.entries[index].text) {
            Ok(()) => {
                if self.diagnose_input {
                    eprintln!("trievium diagnostics: copy successful; closing popup");
                }
                // A viewport close request left the popup alive on the
                // target stack. Terminate only after wl-copy succeeds and
                // the background clipboard owner has been established.
                // The OS releases the session lock on process exit.
                std::process::exit(0);
            }
            Err(error) => {
                if self.diagnose_input {
                    eprintln!("trievium diagnostics: clipboard error: {error}");
                }
                self.typed.clear();
                self.refresh_choices();
                self.error = Some(format!(
                    "Clipboard failed: {error}. Retry a shortcut or click an entry."
                ));
            }
        }
    }

    fn accept_typed(&mut self) {
        match resolve_typed(&self.typed, &self.map) {
            TypedOutcome::Selected(index) => {
                if self.diagnose_input {
                    eprintln!(
                        "trievium diagnostics: shortcut {} selected",
                        self.map.key(index)
                    );
                }
                self.select(index);
            }
            TypedOutcome::Continue(prefix) => {
                if self.diagnose_input {
                    eprintln!("trievium diagnostics: {} prefix letters", prefix.len());
                }
                self.typed = prefix;
                self.refresh_choices();
                self.error = None;
            }
        }
    }
}

impl eframe::App for Picker {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Paint an opaque surface in both modes. The original fixed dark
        // fill made egui's light theme unreadable.
        let theme = ui.ctx().theme();
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, Appearance::background(theme));
        let focused = ui.input(|input| input.focused);
        if self.diagnose_input && self.last_focus != Some(focused) {
            eprintln!("trievium diagnostics: window focus={focused}");
            self.last_focus = Some(focused);
        }
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            std::process::exit(0);
        }

        let field = ui
            .horizontal(|ui| {
                ui.heading("Trievium");
                ui.add(
                    egui::TextEdit::singleline(&mut self.typed)
                        .desired_width(110.0)
                        .char_limit(4),
                )
            })
            .inner;
        if !field.has_focus() {
            field.request_focus();
        }
        if self.diagnose_input && self.input_has_focus != Some(field.has_focus()) {
            eprintln!(
                "trievium diagnostics: shortcut field focus={}",
                field.has_focus()
            );
            self.input_has_focus = Some(field.has_focus());
        }
        if field.changed() {
            self.accept_typed();
        }
        // Keep the normal picker entirely free of instructions and status
        // chatter. Only actionable clipboard failures warrant an extra line.
        if let Some(error) = &self.error {
            ui.colored_label(Appearance::error_color(theme), error);
        }
        ui.separator();

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
            self.select(index);
        }
    }
}

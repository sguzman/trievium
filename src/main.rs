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
use trievium::manager::{Manager, ManagerAction};
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
            "pick" | "manage" | "init" | "validate" | "config-path" | "doctor" => command = arg,
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
                    "Trievium - keyboard-first text selection\n\nUsage: trievium [pick|manage|init|validate|config-path|doctor] [--config PATH] [--theme system|light|dark] [--diagnose-input]\n\nDefault: pick, with --theme system. Keys are automatically assigned unless pinned in the JSON config."
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
            // A new user should be able to open the picker and create their
            // first entry without running init or editing JSON.
            if !path.exists() {
                config::initialize_empty(&path)?;
            }
            let config = Config::load(&path)?;
            let cache_path = shortcut_cache::path_for_config(&path)?;
            let previous = shortcut_cache::load(&cache_path);
            let map = Keymap::build_with_previous(&config.entries, &previous)?;
            for (index, entry) in config.entries.iter().enumerate() {
                println!(
                    "{:<5} {}",
                    map.selection_key(index).to_ascii_lowercase(),
                    entry.label
                );
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
            let picker_height = picker_height(config.entries.len());
            let start_managing = command == "manage";
            let (window_width, window_height) = if start_managing {
                (780.0, 620.0)
            } else {
                (560.0, picker_height)
            };
            // Preserve the user-defined entry order; shortcut lookup is separate.
            // never on redraw. Keep the hot UI path proportional to visible rows.
            let ordered: Vec<usize> = (0..config.entries.len()).collect();
            let manager = if start_managing {
                Some(Manager::new(
                    path.clone(),
                    cache_path.clone(),
                    config.clone(),
                    Keymap::build_with_previous(&config.entries, &previous)?,
                )?)
            } else {
                None
            };
            let mut picker = Picker {
                config,
                map,
                config_path: path,
                cache_path,
                manager,
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
                    .with_inner_size([window_width, window_height])
                    .with_min_inner_size([400.0, 160.0])
                    .with_decorations(false)
                    .with_resizable(true)
                    .with_transparent(false)
                    .with_mouse_passthrough(false)
                    .with_always_on_top(),
                ..Default::default()
            };
            // A post-map float briefly tiles and rearranges other windows.
            // Install the rule before eframe creates the Wayland surface, or
            // fail without opening a tiled fallback.
            hyprland::prepare_floating_popup().map_err(anyhow::Error::msg)?;
            // Only this process's already-floating window is focused/sized.
            hyprland::focus_new_popup(window_width as i32, window_height.round() as i32);
            eframe::run_native(
                "trievium",
                native,
                Box::new(move |cc| {
                    appearance.configure(&cc.egui_ctx);
                    trievium::typography::install(&cc.egui_ctx);
                    Ok(Box::new(picker))
                }),
            )
            .map_err(|err| anyhow::anyhow!("Cannot launch picker: {err}"))?;
        }
    }
    Ok(())
}

const PICKER_ROW_HEIGHT: f32 = 32.0;

fn picker_height(entries: usize) -> f32 {
    // Account for egui's title row, panel margins, header separator and
    // outer padding in addition to the fixed virtual-row height.
    (96.0 + entries.min(9) as f32 * PICKER_ROW_HEIGHT).clamp(175.0, 420.0)
}

/// On Hyprland a floating window's geometry belongs to the compositor.
/// Do not issue a second competing Wayland size request at the same time.
fn resize_dialog(ctx: &egui::Context, width: i32, height: i32) {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some_and(|s| !s.is_empty()) {
        hyprland::resize_own_popup(width, height);
    } else {
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            width as f32,
            height as f32,
        )));
    }
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
    config_path: PathBuf,
    cache_path: PathBuf,
    manager: Option<Manager>,
    typed: String,
    error: Option<String>,
    ordered: Vec<usize>,
    visible: Vec<usize>,
    input_has_focus: Option<bool>,
    diagnose_input: bool,
    last_focus: Option<bool>,
}

impl Picker {
    fn open_manager(&mut self, ctx: &egui::Context) {
        let previous: std::collections::BTreeMap<String, String> = self
            .config
            .entries
            .iter()
            .enumerate()
            .map(|(i, entry)| (entry.label.clone(), self.map.key(i).to_owned()))
            .collect();
        match Keymap::build_with_previous(&self.config.entries, &previous).and_then(|map| {
            Manager::new(
                self.config_path.clone(),
                self.cache_path.clone(),
                self.config.clone(),
                map,
            )
        }) {
            Ok(manager) => {
                self.manager = Some(manager);
                self.typed.clear();
                self.error = None;
                resize_dialog(ctx, 780, 620);
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    fn close_manager(&mut self, ctx: &egui::Context) {
        if let Some(manager) = self.manager.take() {
            (self.config, self.map) = manager.into_data();
            self.typed.clear();
            self.ordered = (0..self.config.entries.len()).collect();
            self.refresh_choices();
            self.error = None;
            let height = picker_height(self.config.entries.len());
            resize_dialog(ctx, 560, height.round() as i32);
        }
    }

    fn refresh_choices(&mut self) {
        let prefix = self.typed.to_ascii_uppercase();
        self.visible = self
            .ordered
            .iter()
            .copied()
            .filter(|&index| self.map.key(index).starts_with(&prefix))
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
                self.typed = prefix.to_ascii_lowercase();
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
        // Both the picker and the entry manager must be opaque and respect
        // the same desktop/system appearance.
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, Appearance::background(theme));
        // Escape is a guaranteed escape hatch in both picker and manager.
        // Do this before the manager body: it previously swallowed Escape.
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            std::process::exit(0);
        }
        if let Some(manager) = self.manager.as_mut() {
            match manager.ui(ui) {
                ManagerAction::Back => self.close_manager(ui.ctx()),
                ManagerAction::Exit => std::process::exit(0),
                ManagerAction::Stay => {}
            }
            return;
        }
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(18, 10))
            .show(ui, |ui| {
                let focused = ui.input(|input| input.focused);
                if self.diagnose_input && self.last_focus != Some(focused) {
                    eprintln!("trievium diagnostics: window focus={focused}");
                    self.last_focus = Some(focused);
                }
                let (field, edit_clicked) = ui
                    .horizontal(|ui| {
                        ui.heading("Trievium");
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut self.typed)
                                .desired_width(92.0)
                                .char_limit(4),
                        );
                        let edit_clicked = ui.button("Edit").clicked();
                        (field, edit_clicked)
                    })
                    .inner;
                if edit_clicked {
                    self.open_manager(ui.ctx());
                    return;
                }
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
                if let Some(error) = &self.error {
                    ui.colored_label(Appearance::error_color(theme), error);
                }
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(3.0);

                // Fixed compact row geometry. Never pair a 39px virtual row
                // with a 50px+ button, separator and global item spacing.
                let mut clicked = None;
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, PICKER_ROW_HEIGHT, self.visible.len(), |ui, rows| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for row in rows {
                            let index = self.visible[row];
                            let entry = &self.config.entries[index];
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::symmetric(0, 4))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let accent = trievium::colors::for_label(
                                            &entry.label, entry.color.as_deref(),
                                        );
                                        let (rect, _) = ui.allocate_exact_size(
                                            egui::vec2(8.0, 20.0), egui::Sense::hover(),
                                        );
                                        ui.painter().rect_filled(
                                            rect, egui::CornerRadius::same(3), accent,
                                        );
                                        let sequence =
                                            self.map.selection_key(index).to_ascii_lowercase();
                                        if ui
                                            .add_sized(
                                                [54.0, 24.0],
                                                egui::Button::new(
                                                    egui::RichText::new(sequence)
                                                        .monospace()
                                                        .strong(),
                                                ),
                                            )
                                            .clicked()
                                        {
                                            clicked = Some(index);
                                        }
                                        if ui.selectable_label(false, &entry.label).clicked() {
                                            clicked = Some(index);
                                        }
                                        let preview: String = entry
                                            .text
                                            .lines()
                                            .next()
                                            .unwrap_or("")
                                            .chars()
                                            .take(35)
                                            .collect();
                                        ui.label(egui::RichText::new(preview).weak().small());
                                    });
                                });
                        }
                    });
                if let Some(index) = clicked {
                    self.select(index);
                }
            });
    }
}

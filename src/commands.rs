//! Independent Hyprland command-tree mode.
//! This module never reads or writes the copy library (items.json).
use anyhow::{Context, Result, bail};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::appearance::Appearance;
use crate::command_editor::{CommandEditor, EditorEvent};
use crate::hyprland;

const MAX_SEQUENCE: usize = 12;
const ROW_HEIGHT: f32 = 38.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandConfig {
    pub version: u32,
    #[serde(default)]
    pub groups: Vec<Group>,
    #[serde(default)]
    pub commands: Vec<CommandEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub prefix: String,
    pub label: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEntry {
    pub sequence: String,
    pub label: String,
    pub action: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Spawn a program directly, without a shell.
    Exec {
        program: String,
        #[serde(default)]
        args: Vec<String>,
    },
    /// Use the Hyprland dispatcher. Explicitly refocus the original window
    /// before focus-dependent commands, never the command popup.
    Hyprland {
        /// Hyprland 0.55+ Lua dispatcher expression.
        expression: String,
        #[serde(default = "focus_original_by_default")]
        focus_original: bool,
    },
}

fn focus_original_by_default() -> bool {
    true
}

fn normalized(sequence: &str) -> Result<String> {
    let mut out = String::new();
    for token in sequence.split_whitespace() {
        if !token.bytes().all(|ch| ch.is_ascii_alphanumeric()) {
            bail!("Sequences accept ASCII letters/digits and spaces only");
        }
        out.push_str(&token.to_ascii_uppercase());
    }
    if out.is_empty() || out.len() > MAX_SEQUENCE {
        bail!("Sequence must contain 1–{MAX_SEQUENCE} ASCII letters/digits");
    }
    Ok(out)
}

impl CommandConfig {
    pub fn empty() -> Self {
        Self {
            version: 1,
            groups: Vec::new(),
            commands: Vec::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).with_context(|| format!("Reading {}", path.display()))?;
        let value: Self = serde_json::from_slice(&bytes)
            .with_context(|| format!("Parsing {}", path.display()))?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("Unsupported command configuration version {}", self.version);
        }
        if self.commands.len() > 10_000 || self.groups.len() > 10_000 {
            bail!("Too many command or group entries (limit 10000)");
        }
        let mut seen = BTreeSet::new();
        for entry in &self.commands {
            let key = normalized(&entry.sequence)
                .with_context(|| format!("Invalid sequence for {}", entry.label))?;
            if entry.label.trim().is_empty() {
                bail!("A command has an empty label");
            }
            if !seen.insert(key.clone()) {
                bail!("Duplicate command sequence {key}");
            }
            match &entry.action {
                Action::Exec { program, args } => {
                    if program.trim().is_empty() || program.contains('\0') {
                        bail!("Invalid executable for {key}");
                    }
                    if args.iter().any(|arg| arg.contains('\0')) {
                        bail!("NUL in executable arguments for {key}");
                    }
                }
                Action::Hyprland { expression, .. } => {
                    if !expression.starts_with("hl.dsp.")
                        || expression.len() > 4096
                        || expression
                            .chars()
                            .any(|ch| matches!(ch, '\n' | '\r' | '\0'))
                    {
                        bail!("Invalid Hyprland Lua dispatcher expression for {key}");
                    }
                }
            }
        }
        // A command may never be an ancestor of another command. Unlike the
        // copy picker, a unique partial prefix is NOT a complete command.
        for key in &seen {
            for n in 1..key.len() {
                if seen.contains(&key[..n]) {
                    bail!("Command {key} conflicts with prefix {}", &key[..n]);
                }
            }
        }
        let mut prefixes = BTreeSet::new();
        for group in &self.groups {
            let key = normalized(&group.prefix)?;
            if group.label.trim().is_empty() || !prefixes.insert(key.clone()) {
                bail!("Empty or duplicate group label/prefix: {key}");
            }
            if seen.contains(&key) {
                bail!("Group {key} shadows an executable command");
            }
            // A sorted prefix range finds a matching command in O(log n)
            // rather than scanning the entire command library per group.
            if !seen
                .range(key.clone()..)
                .next()
                .is_some_and(|sequence| sequence.starts_with(&key))
            {
                bail!("Group {key} does not contain any commands");
            }
        }
        Ok(())
    }

    fn matched(&self, prefix: &str) -> bool {
        self.commands.iter().any(|entry| {
            normalized(&entry.sequence).is_ok_and(|sequence| sequence.starts_with(prefix))
        })
    }

    fn completed(&self, prefix: &str) -> Option<usize> {
        self.commands
            .iter()
            .position(|entry| normalized(&entry.sequence).is_ok_and(|sequence| sequence == prefix))
    }

    /// A command executes only at its *full* configured sequence.
    /// Invalid keys are ignored; a valid preceding prefix remains editable.
    pub fn resolve(&self, typed: &str) -> Resolution {
        let mut prefix = String::new();
        for (offset, key) in typed.char_indices() {
            if !key.is_ascii_alphanumeric() {
                continue;
            }
            prefix.push(key.to_ascii_uppercase());
            if let Some(index) = self.completed(&prefix) {
                // A paste or IME update is one input event. Never execute the
                // first complete command in a longer string such as "wf rm".
                // Ordinary incremental typing still selects at the last key.
                return if offset + key.len_utf8() == typed.len()
                    && normalized(typed).is_ok_and(|raw| raw == prefix)
                {
                    Resolution::Selected(index)
                } else {
                    Resolution::Continue(String::new())
                };
            }
            if !self.matched(&prefix) {
                prefix.pop();
            }
        }
        Resolution::Continue(prefix)
    }

    // Reference reducer for equivalence tests; the live HUD uses CommandIndex.
    #[cfg(test)]
    fn choices(&self, prefix: &str) -> Vec<Choice> {
        // Normalize each group once instead of rescanning every group for
        // every command (which was quadratic on large command registries).
        let groups: BTreeMap<String, &str> = self
            .groups
            .iter()
            .filter_map(|group| {
                normalized(&group.prefix)
                    .ok()
                    .map(|key| (key, group.label.as_str()))
            })
            .collect();
        let mut choices = BTreeMap::new();
        for (index, entry) in self.commands.iter().enumerate() {
            let Ok(sequence) = normalized(&entry.sequence) else {
                continue;
            };
            if !sequence.starts_with(prefix) || sequence.len() == prefix.len() {
                continue;
            }
            let key = sequence.as_bytes()[prefix.len()] as char;
            let next = format!("{prefix}{key}");
            let terminal = sequence == next;
            let label = if terminal {
                entry.label.clone()
            } else {
                groups
                    .get(&next)
                    .map(|label| (*label).to_owned())
                    .unwrap_or_else(|| format!("Group {}", next.to_ascii_lowercase()))
            };
            choices.entry(key).or_insert(Choice {
                key,
                label,
                terminal: terminal.then_some(index),
            });
        }
        choices.into_values().collect()
    }
}

/// Immutable prefix lookup compiled before the HUD starts or on a worker
/// after editing. Key events only access one short prefix and its children.
struct CommandIndex {
    branches: BTreeMap<String, Vec<Choice>>,
    terminals: BTreeMap<String, usize>,
}

impl CommandIndex {
    fn compile(config: &CommandConfig) -> Self {
        let group_labels: BTreeMap<String, &str> = config
            .groups
            .iter()
            .filter_map(|group| {
                normalized(&group.prefix)
                    .ok()
                    .map(|prefix| (prefix, group.label.as_str()))
            })
            .collect();
        let mut branches: BTreeMap<String, BTreeMap<char, Choice>> = BTreeMap::new();
        let mut terminals = BTreeMap::new();

        for (index, entry) in config.commands.iter().enumerate() {
            let sequence = normalized(&entry.sequence).expect("validated command sequence");
            terminals.insert(sequence.clone(), index);
            for position in 0..sequence.len() {
                let prefix = &sequence[..position];
                let next = &sequence[..position + 1];
                let key = sequence.as_bytes()[position] as char;
                let terminal = position + 1 == sequence.len();
                let label = if terminal {
                    entry.label.clone()
                } else {
                    group_labels
                        .get(next)
                        .map(|label| (*label).to_owned())
                        .unwrap_or_else(|| format!("Group {}", next.to_ascii_lowercase()))
                };
                branches
                    .entry(prefix.to_owned())
                    .or_default()
                    .entry(key)
                    .or_insert(Choice {
                        key,
                        label,
                        terminal: terminal.then_some(index),
                    });
            }
        }

        Self {
            branches: branches
                .into_iter()
                .map(|(prefix, children)| (prefix, children.into_values().collect()))
                .collect(),
            terminals,
        }
    }

    fn choices(&self, prefix: &str) -> &[Choice] {
        self.branches.get(prefix).map(Vec::as_slice).unwrap_or(&[])
    }

    fn resolve(&self, typed: &str) -> Resolution {
        let mut prefix = String::new();
        for (offset, key) in typed.char_indices() {
            if !key.is_ascii_alphanumeric() {
                continue;
            }
            prefix.push(key.to_ascii_uppercase());
            if let Some(&index) = self.terminals.get(&prefix) {
                return if offset + key.len_utf8() == typed.len()
                    && normalized(typed).is_ok_and(|raw| raw == prefix)
                {
                    Resolution::Selected(index)
                } else {
                    Resolution::Continue(String::new())
                };
            }
            if !self.branches.contains_key(&prefix) {
                prefix.pop();
            }
        }
        Resolution::Continue(prefix)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Resolution {
    Continue(String),
    Selected(usize),
}

#[derive(Clone)]
struct Choice {
    key: char,
    label: String,
    terminal: Option<usize>,
}

pub fn config_path(override_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = override_path {
        return Ok(path);
    }
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(dir).join("trievium/commands.json"));
    }
    let home = std::env::var_os("HOME").context("HOME or XDG_CONFIG_HOME is required")?;
    Ok(PathBuf::from(home).join(".config/trievium/commands.json"))
}

/// A bare relative filename has an empty parent (`""`), which is not a
/// directory that can be created. Treat it as the current directory.
fn config_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

/// Never overwrite an existing command registry, particularly on a second
/// invocation. The item library is neither discovered nor opened.
pub fn initialize_empty(path: &Path) -> Result<()> {
    let parent = config_parent(path);
    fs::create_dir_all(parent).with_context(|| format!("Creating {}", parent.display()))?;
    let mut options = fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("Refusing to overwrite existing registry {}", path.display()))?;
    use std::io::Write;
    file.write_all(serde_json::to_string_pretty(&CommandConfig::empty())?.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Launch a user-defined command on a worker; never block egui with hyprctl
/// or process startup. There is deliberately no shell interpolation.
fn execute(action: &Action, original: Option<&str>) -> Result<()> {
    match action {
        Action::Exec { program, args } => {
            Command::new(program)
                .args(args)
                .spawn()
                .with_context(|| format!("Launching {program}"))?;
        }
        Action::Hyprland {
            expression,
            focus_original,
        } => {
            if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
                bail!("Hyprland command requested outside a Hyprland session");
            }
            if *focus_original {
                let address = original.context(
                    "No original Hyprland window; cannot safely target a focused-window command",
                )?;
                hyprland::restore_original_focus(address).map_err(anyhow::Error::msg)?;
            }
            hyprland::dispatch_lua(expression).map_err(anyhow::Error::msg)?;
        }
    }
    Ok(())
}

pub fn run(args: &[String]) -> Result<()> {
    let mut appearance = Appearance::default();
    let mut override_path = None;
    let mut operation = "pick";
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "init" | "validate" | "config-path" | "edit" => operation = &args[index],
            "--version" | "-V" => {
                println!("trievium {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!(
                    "Trievium command tree - hierarchical command HUD\n\nUsage: trievium commands [pick|edit|init|validate|config-path] [--config PATH] [--theme system|light|dark]\n\nDefault: command HUD. Independent registry: commands.json"
                );
                return Ok(());
            }
            "--config" => {
                index += 1;
                override_path = Some(PathBuf::from(
                    args.get(index).context("--config requires a path")?,
                ));
            }
            "--theme" | "--color-mode" => {
                index += 1;
                appearance = args
                    .get(index)
                    .context("--theme requires a value")?
                    .parse()?;
            }
            "pick" => operation = "pick",
            other => bail!("Unknown commands argument '{other}'"),
        }
        index += 1;
    }

    let path = config_path(override_path)?;
    match operation {
        "config-path" => {
            println!("{}", path.display());
            return Ok(());
        }
        "init" => {
            initialize_empty(&path)?;
            println!("Created {}", path.display());
            return Ok(());
        }
        _ => {}
    }
    // Read-only checks must work even while a HUD is open. Interactive
    // modes acquire the singleton before first-run registry initialization:
    // two rapid Super+Space presses must not race a create_new/write pair.
    if operation == "validate" {
        let config = CommandConfig::load(&path)?;
        println!(
            "Valid command registry ({} commands)",
            config.commands.len()
        );
        return Ok(());
    }
    let _guard = match crate::launch::acquire()? {
        Some(crate::launch::Instance::Busy) => {
            eprintln!("trievium: another HUD is already open");
            return Ok(());
        }
        value => value,
    };
    if !path.exists() {
        initialize_empty(&path)?;
    }
    let config = CommandConfig::load(&path)?;
    // Compilation happens before the GUI event loop, never on its render thread.
    let command_index = CommandIndex::compile(&config);
    let original_window = hyprland::focused_window_before_popup();
    let editing = operation == "edit";
    let height = if editing {
        680.0
    } else {
        (118.0 + ROW_HEIGHT * command_index.choices("").len().min(8) as f32).clamp(185.0, 460.0)
    };
    let width = if editing { 900.0 } else { 560.0 };
    let native = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        system_font_fallback: false,
        viewport: egui::ViewportBuilder::default()
            .with_title("Trievium Commands")
            .with_app_id("trievium")
            .with_inner_size([width, height])
            .with_min_inner_size([400.0, 160.0])
            .with_decorations(false)
            .with_resizable(true)
            .with_always_on_top(),
        ..Default::default()
    };
    // Complete disk reads and editor validation *before* starting the
    // asynchronous map/focus worker. Otherwise a slow registry load can
    // consume the worker's bounded mapping retry window before the native
    // window even exists, leaving the editor without keyboard focus.
    let editor = if editing {
        Some(CommandEditor::new(path.clone())?)
    } else {
        None
    };
    hyprland::prepare_floating_popup().map_err(anyhow::Error::msg)?;
    let focus_state =
        hyprland::focus_new_command_popup(width.round() as i32, height.round() as i32);
    let visible_choices = command_index.choices("").to_vec();
    let app = CommandPicker {
        config,
        command_index,
        visible_prefix: String::new(),
        visible_choices,
        config_path: path,
        editor,
        pending_editor: None,
        pending_compilation: None,
        original_window,
        focus_state,
        typed: String::new(),
        error: None,
        pending: None,
        font_load_after: None,
        font_load_started: false,
    };
    eframe::run_native(
        "trievium-commands",
        native,
        Box::new(move |cc| {
            appearance.configure(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| anyhow::anyhow!("Cannot launch command HUD: {error}"))?;
    Ok(())
}

type PendingEditor = Arc<Mutex<Option<Result<CommandEditor>>>>;
type PendingCompilation = Arc<Mutex<Option<(CommandConfig, CommandIndex)>>>;

struct CommandPicker {
    config: CommandConfig,
    command_index: CommandIndex,
    // Only regenerate visible entries when the prefix or registry changes,
    // never on egui's every-frame paint path.
    visible_prefix: String,
    visible_choices: Vec<Choice>,
    config_path: PathBuf,
    editor: Option<CommandEditor>,
    pending_editor: Option<PendingEditor>,
    pending_compilation: Option<PendingCompilation>,
    original_window: Option<String>,
    focus_state: hyprland::CommandPopupFocus,
    typed: String,
    error: Option<String>,
    pending: Option<Arc<Mutex<Option<Result<()>>>>>,
    font_load_after: Option<std::time::Instant>,
    font_load_started: bool,
}

impl CommandPicker {
    fn refresh_choices(&mut self) {
        let prefix = self.typed.to_ascii_uppercase();
        if self.visible_prefix != prefix {
            self.visible_choices = self.command_index.choices(&prefix).to_vec();
            self.visible_prefix = prefix;
        }
    }

    fn resize(ctx: &egui::Context, width: i32, height: i32) {
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some_and(|s| !s.is_empty()) {
            hyprland::resize_own_popup(width, height);
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                width as f32,
                height as f32,
            )));
        }
    }

    fn open_editor(&mut self, ctx: &egui::Context) {
        if self.pending_editor.is_some() || self.pending.is_some() {
            return;
        }
        // Never load and validate a potentially large registry on egui's UI thread.
        let result: PendingEditor = Arc::new(Mutex::new(None));
        let sender = Arc::clone(&result);
        let path = self.config_path.clone();
        self.pending_editor = Some(result);
        self.error = None;
        std::thread::spawn(move || {
            let editor = CommandEditor::new(path);
            if let Ok(mut slot) = sender.lock() {
                *slot = Some(editor);
            }
        });
        ctx.request_repaint_after(Duration::from_millis(20));
    }

    fn choose(&mut self, index: usize) {
        if self.pending.is_some() {
            return;
        }
        let result = Arc::new(Mutex::new(None));
        let sender = Arc::clone(&result);
        let action = self.config.commands[index].action.clone();
        let original = self.original_window.clone();
        let focus = self.focus_state.clone();
        focus.mark_action_started();
        self.pending = Some(result);
        std::thread::spawn(move || {
            // Serialized against pending initial popup focus operations.
            // All compositor/process calls remain off egui's UI thread.
            let outcome = focus.run_action(|| execute(&action, original.as_deref()));
            if outcome.is_err() {
                focus.recover_after_error();
            }
            if let Ok(mut slot) = sender.lock() {
                *slot = Some(outcome);
            }
        });
    }

    fn accept(&mut self) {
        match self.command_index.resolve(&self.typed) {
            Resolution::Selected(index) => self.choose(index),
            Resolution::Continue(prefix) => {
                self.typed = prefix.to_ascii_lowercase();
                self.refresh_choices();
                self.error = None;
            }
        }
    }
}

impl eframe::App for CommandPicker {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(pending) = &self.pending_compilation {
            let compiled = pending.lock().ok().and_then(|mut state| state.take());
            if let Some((config, index)) = compiled {
                self.pending_compilation = None;
                self.config = config;
                self.command_index = index;
                self.typed.clear();
                self.visible_prefix = String::from("stale");
                self.refresh_choices();
                self.error = None;
                let height = (118.0
                    + ROW_HEIGHT * self.command_index.choices("").len().min(8) as f32)
                    .clamp(185.0, 460.0);
                Self::resize(ctx, 560, height.round() as i32);
            } else {
                ctx.request_repaint_after(Duration::from_millis(20));
            }
        }
        if let Some(pending) = &self.pending_editor {
            let loaded = pending.lock().ok().and_then(|mut state| state.take());
            if let Some(result) = loaded {
                self.pending_editor = None;
                match result {
                    Ok(editor) => {
                        self.editor = Some(editor);
                        self.typed.clear();
                        self.error = None;
                        Self::resize(ctx, 900, 680);
                    }
                    Err(error) => self.error = Some(format!("{error:#}")),
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(20));
            }
        }
        if !self.font_load_started
            && self
                .font_load_after
                .is_some_and(|when| std::time::Instant::now() >= when)
        {
            self.font_load_started = true;
            self.font_load_after = None;
            crate::typography::install_async(ctx);
        }
        if let Some(pending) = &self.pending {
            let completed = pending.lock().ok().and_then(|mut state| state.take());
            if let Some(result) = completed {
                match result {
                    Ok(()) => std::process::exit(0),
                    Err(error) => {
                        self.error = Some(format!("{error:#}"));
                        self.pending = None;
                        self.typed.clear();
                        self.refresh_choices();
                    }
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(20));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if !self.font_load_started && self.font_load_after.is_none() {
            self.font_load_after = Some(std::time::Instant::now() + Duration::from_millis(50));
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        let theme = ui.ctx().theme();
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, Appearance::background(theme));
        if self.pending.is_none() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            if let Some(editor) = self.editor.as_mut() {
                if editor.close_requested() {
                    std::process::exit(0);
                }
            } else {
                std::process::exit(0);
            }
        }
        if self.pending_editor.is_some() {
            ui.label("Opening command editor…");
            return;
        }
        if self.pending_compilation.is_some() {
            ui.label("Preparing command tree…");
            return;
        }
        if let Some(editor) = self.editor.as_mut() {
            match editor.ui(ui) {
                EditorEvent::Stay => {}
                EditorEvent::Exit => std::process::exit(0),
                EditorEvent::Done => {
                    let editor = self.editor.take().unwrap();
                    // Reindex the edited registry on a worker before exposing
                    // the picker. Do not scan thousands of sequences on egui.
                    let config = editor.config;
                    let result: PendingCompilation = Arc::new(Mutex::new(None));
                    let sender = Arc::clone(&result);
                    self.pending_compilation = Some(result);
                    std::thread::spawn(move || {
                        let index = CommandIndex::compile(&config);
                        if let Ok(mut slot) = sender.lock() {
                            *slot = Some((config, index));
                        }
                    });
                    ui.ctx().request_repaint_after(Duration::from_millis(20));
                }
            }
            return;
        }
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(18, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Trievium / Commands");
                    if ui
                        .add_enabled(self.pending.is_none(), egui::Button::new("Edit"))
                        .clicked()
                    {
                        self.open_editor(ui.ctx());
                        return;
                    }
                    ui.add_enabled_ui(self.pending.is_none(), |ui| {
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut self.typed)
                                .desired_width(135.0)
                                .char_limit(MAX_SEQUENCE),
                        );
                        if !field.has_focus() {
                            field.request_focus();
                        }
                        if field.changed() {
                            self.accept();
                        }
                    });
                });
                if let Some(error) = &self.error {
                    ui.colored_label(Appearance::error_color(theme), error);
                }
                if self.pending.is_some() {
                    ui.label("Running command…");
                }
                ui.separator();
                let choices = &self.visible_choices;
                if self.config.commands.is_empty() {
                    ui.label("No commands configured in commands.json");
                }
                let mut clicked = None;
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show_rows(ui, ROW_HEIGHT, choices.len(), |ui, rows| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for row in rows {
                            let choice = &choices[row];
                            ui.horizontal(|ui| {
                                let button = ui.add_sized(
                                    [45.0, 28.0],
                                    egui::Button::new(
                                        egui::RichText::new(
                                            choice.key.to_ascii_lowercase().to_string(),
                                        )
                                        .monospace()
                                        .strong(),
                                    ),
                                );
                                if button.clicked()
                                    || ui.selectable_label(false, &choice.label).clicked()
                                {
                                    clicked = Some((choice.key, choice.terminal));
                                }
                            });
                        }
                    });
                if let Some((key, terminal)) = clicked {
                    if let Some(index) = terminal {
                        self.choose(index);
                    } else {
                        self.typed.push(key.to_ascii_lowercase());
                        self.refresh_choices();
                    }
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> CommandConfig {
        CommandConfig {
            version: 1,
            groups: vec![Group {
                prefix: "w".into(),
                label: "Windows".into(),
            }],
            commands: vec![
                CommandEntry {
                    sequence: "w f".into(),
                    label: "Fullscreen".into(),
                    action: Action::Hyprland {
                        expression: r#"hl.dsp.window.fullscreen({ mode = "fullscreen" })"#.into(),
                        focus_original: true,
                    },
                },
                CommandEntry {
                    sequence: "w c".into(),
                    label: "Close".into(),
                    action: Action::Hyprland {
                        expression: "hl.dsp.window.close()".into(),
                        focus_original: true,
                    },
                },
                CommandEntry {
                    sequence: "a t".into(),
                    label: "Terminal".into(),
                    action: Action::Exec {
                        program: "kitty".into(),
                        args: Vec::new(),
                    },
                },
            ],
        }
    }

    #[test]
    fn bare_relative_command_registry_uses_current_directory() {
        assert_eq!(config_parent(Path::new("commands.json")), Path::new("."));
        assert_eq!(
            config_parent(Path::new("config/trievium/commands.json")),
            Path::new("config/trievium")
        );
        assert_eq!(
            config_parent(Path::new("/tmp/trievium/commands.json")),
            Path::new("/tmp/trievium")
        );
    }

    #[test]
    fn exact_sequence_required_even_with_single_candidate() {
        let config = demo();
        config.validate().unwrap();
        assert_eq!(config.resolve("w"), Resolution::Continue("W".into()));
        assert_eq!(config.resolve("wf"), Resolution::Selected(0));
        assert_eq!(config.resolve("wxc"), Resolution::Selected(1));
        assert_eq!(config.resolve("a"), Resolution::Continue("A".into()));
        assert_eq!(config.resolve("at"), Resolution::Selected(2));
    }

    #[test]
    fn pasted_extra_text_never_executes_a_prefix_command() {
        let config = demo();
        config.validate().unwrap();
        let index = CommandIndex::compile(&config);
        for typed in [
            "wfx", "wf extra", "wf ", "wf!", "wf\nwat", "wxcfoo", "zwf", "wzf", "?wf", "w?f",
            "w f!",
        ] {
            assert_eq!(
                config.resolve(typed),
                Resolution::Continue(String::new()),
                "reference reducer executed a truncated paste: {typed:?}"
            );
            assert_eq!(
                index.resolve(typed),
                Resolution::Continue(String::new()),
                "indexed reducer executed a truncated paste: {typed:?}"
            );
        }
        for (typed, selected) in [("wf", 0), ("w f", 0), ("wxc", 1), ("at", 2)] {
            assert_eq!(config.resolve(typed), Resolution::Selected(selected));
            assert_eq!(index.resolve(typed), Resolution::Selected(selected));
        }
    }

    #[test]
    fn ordinary_invalid_key_is_ignored_between_incremental_shortcut_keys() {
        let config = demo();
        config.validate().unwrap();
        let index = CommandIndex::compile(&config);
        let mut typed = String::new();
        for (key, expected) in [
            ('w', Resolution::Continue("W".into())),
            ('z', Resolution::Continue("W".into())),
            ('f', Resolution::Selected(0)),
        ] {
            typed.push(key);
            let resolved = index.resolve(&typed);
            assert_eq!(resolved, expected);
            assert_eq!(resolved, config.resolve(&typed));
            if let Resolution::Continue(prefix) = resolved {
                // Matches CommandPicker::accept: ignore invalid individual
                // keys before the next keyboard event reaches the field.
                typed = prefix.to_ascii_lowercase();
            }
        }
    }

    #[test]
    fn compiled_prefix_index_matches_reference_reducer_and_branch_labels() {
        let config = demo();
        config.validate().unwrap();
        let index = CommandIndex::compile(&config);
        for typed in ["", "w", "wf", "W C", "wxc", "a", "at", "zawf", "a!", "wxz"] {
            assert_eq!(
                index.resolve(typed),
                config.resolve(typed),
                "typed {typed:?}"
            );
        }
        for prefix in ["", "W", "A", "WF", "Z"] {
            let indexed = index.choices(prefix);
            let reference = config.choices(prefix);
            assert_eq!(indexed.len(), reference.len(), "prefix {prefix:?}");
            for (got, want) in indexed.iter().zip(&reference) {
                assert_eq!(got.key, want.key);
                assert_eq!(got.label, want.label);
                assert_eq!(got.terminal, want.terminal);
            }
        }
    }

    #[test]
    fn index_keeps_deep_commands_exact_when_they_are_only_candidate() {
        let config = CommandConfig {
            version: 1,
            groups: vec![Group {
                prefix: "w a".into(),
                label: "Advanced".into(),
            }],
            commands: vec![CommandEntry {
                sequence: "w a t".into(),
                label: "Deep terminal".into(),
                action: Action::Exec {
                    program: "true".into(),
                    args: Vec::new(),
                },
            }],
        };
        config.validate().unwrap();
        let index = CommandIndex::compile(&config);
        assert_eq!(index.resolve("w"), Resolution::Continue("W".into()));
        assert_eq!(index.resolve("wa"), Resolution::Continue("WA".into()));
        assert_eq!(index.resolve("wat"), Resolution::Selected(0));
        assert_eq!(index.choices("W")[0].label, "Advanced");
        assert_eq!(index.choices("WA")[0].label, "Deep terminal");
    }

    #[test]
    fn compiled_index_supports_maximum_registry_without_early_execution() {
        let config = CommandConfig {
            version: 1,
            groups: Vec::new(),
            commands: (0..10_000)
                .map(|i| CommandEntry {
                    sequence: format!("a{i:04}"),
                    label: format!("Command {i}"),
                    action: Action::Exec {
                        program: "true".into(),
                        args: Vec::new(),
                    },
                })
                .collect(),
        };
        config.validate().unwrap();
        let index = CommandIndex::compile(&config);
        assert_eq!(index.choices("").len(), 1);
        assert_eq!(index.choices("A").len(), 10);
        assert_eq!(index.resolve("a9"), Resolution::Continue("A9".into()));
        assert_eq!(index.resolve("a9999"), Resolution::Selected(9999));
        assert_eq!(index.resolve("a0000"), Resolution::Selected(0));
    }

    #[test]
    fn nested_three_key_command_never_executes_at_an_ancestor() {
        let config = CommandConfig {
            version: 1,
            groups: vec![
                Group {
                    prefix: "w".into(),
                    label: "Windows".into(),
                },
                Group {
                    prefix: "w a".into(),
                    label: "Advanced".into(),
                },
            ],
            commands: vec![CommandEntry {
                sequence: "w a t".into(),
                label: "Deep action".into(),
                action: Action::Exec {
                    program: "true".into(),
                    args: Vec::new(),
                },
            }],
        };
        config.validate().unwrap();
        assert_eq!(config.resolve("w"), Resolution::Continue("W".into()));
        assert_eq!(config.resolve("wa"), Resolution::Continue("WA".into()));
        assert_eq!(config.resolve("wat"), Resolution::Selected(0));
        assert_eq!(config.choices("W")[0].label, "Advanced");
        assert_eq!(config.choices("WA")[0].label, "Deep action");
    }

    #[test]
    fn groups_and_commands_are_separate_in_the_hud() {
        let config = demo();
        let root = config.choices("");
        assert_eq!(root.len(), 2);
        assert_eq!(config.choices("W")[0].label, "Close");
        assert_eq!(config.choices("W")[1].label, "Fullscreen");
        assert_eq!(root[1].label, "Windows");
    }

    #[test]
    fn rejects_collisions_including_whitespace_and_case() {
        let mut config = demo();
        config.commands.push(config.commands[0].clone());
        config.commands.last_mut().unwrap().sequence = "WF".into();
        assert!(config.validate().is_err());
        config.commands.pop();
        config.commands.push(CommandEntry {
            sequence: "w".into(),
            label: "Ambiguous ancestor".into(),
            action: Action::Exec {
                program: "true".into(),
                args: Vec::new(),
            },
        });
        assert!(config.validate().is_err());
    }

    #[test]
    fn group_prefix_requires_actual_descendant_command() {
        let mut config = demo();
        config.groups.push(Group {
            prefix: "x".into(),
            label: "Unused".into(),
        });
        assert!(config.validate().is_err());
        config.groups.last_mut().unwrap().prefix = "a".into();
        assert!(config.validate().is_ok());
        // A group must be strictly above the command it describes.
        config.groups.last_mut().unwrap().prefix = "a t".into();
        assert!(config.validate().is_err());
    }

    #[test]
    fn command_file_never_requires_or_mutates_items_json() {
        let dir =
            std::env::temp_dir().join(format!("trievium-command-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let original = b"private clipboard library".to_vec();
        fs::write(dir.join("items.json"), &original).unwrap();
        initialize_empty(&dir.join("commands.json")).unwrap();
        assert_eq!(
            CommandConfig::load(&dir.join("commands.json"))
                .unwrap()
                .commands
                .len(),
            0
        );
        assert!(initialize_empty(&dir.join("commands.json")).is_err());
        assert_eq!(fs::read(dir.join("items.json")).unwrap(), original);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn accepts_empty_registry_and_rejects_malformed_actions() {
        CommandConfig::empty().validate().unwrap();
        let mut config = demo();
        if let Action::Hyprland { expression, .. } = &mut config.commands[0].action {
            *expression = "fullscreen; rm".into();
        }
        assert!(config.validate().is_err());
        assert!(serde_json::from_str::<Action>(r#"{"type":"shell","command":"echo hi"}"#).is_err());
    }
}

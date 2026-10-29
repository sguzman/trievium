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
        dispatcher: String,
        #[serde(default)]
        args: String,
        #[serde(default)]
        focus_original: bool,
    },
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
                Action::Hyprland {
                    dispatcher, args, ..
                } => {
                    if dispatcher.is_empty()
                        || !dispatcher
                            .bytes()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == b'_')
                        || args.chars().any(|ch| matches!(ch, '\n' | '\r' | '\0'))
                    {
                        bail!("Invalid Hyprland dispatcher or arguments for {key}");
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
            if !seen.iter().any(|sequence| sequence.starts_with(&key)) {
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
        for key in typed.chars() {
            if !key.is_ascii_alphanumeric() {
                continue;
            }
            prefix.push(key.to_ascii_uppercase());
            if let Some(index) = self.completed(&prefix) {
                return Resolution::Selected(index);
            }
            if !self.matched(&prefix) {
                prefix.pop();
            }
        }
        Resolution::Continue(prefix)
    }

    fn choices(&self, prefix: &str) -> Vec<Choice> {
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
                self.groups
                    .iter()
                    .find(|group| normalized(&group.prefix).ok().as_deref() == Some(&next))
                    .map(|group| group.label.clone())
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

#[derive(Debug, PartialEq, Eq)]
pub enum Resolution {
    Continue(String),
    Selected(usize),
}

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

/// Never overwrite an existing command registry, particularly on a second
/// invocation. The item library is neither discovered nor opened.
pub fn initialize_empty(path: &Path) -> Result<()> {
    let parent = path.parent().context("Command path has no parent")?;
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
            dispatcher,
            args,
            focus_original,
        } => {
            if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
                bail!("Hyprland command requested outside a Hyprland session");
            }
            if *focus_original {
                let address = original.context(
                    "No original Hyprland window; cannot safely target a focused-window command",
                )?;
                hyprland::restore_original_focus(address)?;
            }
            let output = Command::new("hyprctl")
                .args(["dispatch", dispatcher, args])
                .output()
                .with_context(|| format!("Running Hyprland dispatcher {dispatcher}"))?;
            if !output.status.success() || !String::from_utf8_lossy(&output.stdout).trim().eq("ok")
            {
                bail!(
                    "Hyprland rejected {dispatcher}: {} {}",
                    String::from_utf8_lossy(&output.stdout).trim(),
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
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
    if !path.exists() && operation == "pick" {
        initialize_empty(&path)?;
    }
    let config = CommandConfig::load(&path)?;
    if operation == "validate" {
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
    let original_window = hyprland::focused_window_before_popup();
    let editing = operation == "edit";
    let height = if editing { 680.0 } else {
        (118.0 + ROW_HEIGHT * config.choices("").len().min(8) as f32).clamp(185.0, 460.0)
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
    hyprland::prepare_floating_popup().map_err(anyhow::Error::msg)?;
    hyprland::focus_new_popup(width.round() as i32, height.round() as i32);
    let editor = if editing {
        Some(CommandEditor::new(path.clone(), config.clone())?)
    } else {
        None
    };
    let app = CommandPicker {
        config,
        config_path: path,
        editor,
        original_window,
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

struct CommandPicker {
    config: CommandConfig,
    config_path: PathBuf,
    editor: Option<CommandEditor>,
    original_window: Option<String>,
    typed: String,
    error: Option<String>,
    pending: Option<Arc<Mutex<Option<Result<()>>>>>,
    font_load_after: Option<std::time::Instant>,
    font_load_started: bool,
}

impl CommandPicker {
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
        match CommandEditor::new(self.config_path.clone(), self.config.clone()) {
            Ok(editor) => {
                self.editor = Some(editor);
                self.typed.clear();
                self.error = None;
                Self::resize(ctx, 900, 680);
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    fn choose(&mut self, index: usize) {
        if self.pending.is_some() {
            return;
        }
        let result = Arc::new(Mutex::new(None));
        let sender = Arc::clone(&result);
        let action = self.config.commands[index].action.clone();
        let original = self.original_window.clone();
        self.pending = Some(result);
        std::thread::spawn(move || {
            let outcome = execute(&action, original.as_deref());
            if let Ok(mut slot) = sender.lock() {
                *slot = Some(outcome);
            }
        });
    }

    fn accept(&mut self) {
        match self.config.resolve(&self.typed) {
            Resolution::Selected(index) => self.choose(index),
            Resolution::Continue(prefix) => {
                self.typed = prefix.to_ascii_lowercase();
                self.error = None;
            }
        }
    }
}

impl eframe::App for CommandPicker {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
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
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            std::process::exit(0);
        }
        if let Some(editor) = self.editor.as_mut() {
            match editor.ui(ui) {
                EditorEvent::Stay => {}
                EditorEvent::Exit => std::process::exit(0),
                EditorEvent::Done => {
                    let editor = self.editor.take().unwrap();
                    self.config = editor.config;
                    self.typed.clear();
                    self.error = None;
                    let height = (118.0 + ROW_HEIGHT * self.config.choices("").len().min(8) as f32)
                        .clamp(185.0, 460.0);
                    Self::resize(ui.ctx(), 560, height.round() as i32);
                }
            }
            return;
        }
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(18, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Trievium / Commands");
                    if ui.button("Edit").clicked() {
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
                let prefix = self.typed.to_ascii_uppercase();
                let choices = self.config.choices(&prefix);
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
                        dispatcher: "fullscreen".into(),
                        args: "0".into(),
                        focus_original: true,
                    },
                },
                CommandEntry {
                    sequence: "w c".into(),
                    label: "Close".into(),
                    action: Action::Hyprland {
                        dispatcher: "closewindow".into(),
                        args: String::new(),
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
        if let Action::Hyprland { dispatcher, .. } = &mut config.commands[0].action {
            *dispatcher = "fullscreen; rm".into();
        }
        assert!(config.validate().is_err());
        assert!(serde_json::from_str::<Action>(r#"{"type":"shell","command":"echo hi"}"#).is_err());
    }
}

//! Native command-tree editor with independent, conflict-safe persistence.
//! Never accesses the prefix-copy item library.
use crate::commands::{Action, CommandConfig, CommandEntry, Group};
use anyhow::{Context, Result, bail};
use eframe::egui;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub enum EditorEvent {
    Stay,
    Done,
    Exit,
}

/// The UI relinquishes ownership of the registry while the save worker runs,
/// avoiding an expensive full-library clone on the egui render thread.
struct SaveOutcome {
    config: CommandConfig,
    original: Vec<u8>,
    result: Result<Vec<u8>>,
}

type PendingSave = Arc<Mutex<Option<SaveOutcome>>>;
type PendingReload = Arc<Mutex<Option<Result<(PathBuf, Vec<u8>, CommandConfig)>>>>;

/// Each non-final line break separates argv elements. Pressing Enter after
/// the last argument should not silently append another empty argument.
/// Interior blank lines remain meaningful explicit empty argv elements.
fn parse_argv_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split_terminator('\n').map(str::to_owned).collect()
    }
}

/// Keep one structural final newline in the editable representation.
/// A blank final argv element then has its own *additional* line break, so
/// parsing and rendering preserve existing empty argv elements exactly.
fn format_argv_lines(args: &[String]) -> String {
    if args.is_empty() {
        String::new()
    } else {
        let mut text = args.join("\n");
        text.push('\n');
        text
    }
}

pub struct CommandEditor {
    path: PathBuf,
    /// Stable, fully resolved destination. Atomic rename must update this
    /// target rather than replacing a config-store symlink at `path`.
    target: PathBuf,
    original: Vec<u8>,
    pub config: CommandConfig,
    selected: Option<usize>,
    // Retain editable argv text across frames; never join large argument
    // vectors on every egui repaint.
    args_text: String,
    // An argument with an embedded newline cannot be represented faithfully
    // by one-argument-per-line text. Preserve it without exposing a lossy edit.
    argv_has_newline: bool,
    error: Option<String>,
    dirty: bool,
    pending: Option<PendingSave>,
    pending_reload: Option<PendingReload>,
    finish_after_save: bool,
    finish_after_reload: bool,
    done_ready: bool,
    confirm_exit: bool,
}

fn write_atomic(
    path: &Path,
    target: &Path,
    config: &CommandConfig,
    expected: &[u8],
) -> Result<Vec<u8>> {
    config.validate()?;
    if fs::canonicalize(path).with_context(|| format!("Resolving {}", path.display()))? != target {
        bail!("Command registry symlink or destination changed. Discard/reopen before saving.");
    }
    if fs::read(path).with_context(|| format!("Reading {}", path.display()))? != expected {
        bail!("Commands changed outside this editor. Discard/reopen before saving.");
    }
    let mut next = serde_json::to_vec_pretty(config)?;
    next.push(b'\n');
    let parent = target
        .parent()
        .context("Command file has no parent directory")?;
    let name = target.file_name().context("Command file has no filename")?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut temp_path = None;
    let result = (|| -> Result<()> {
        use std::io::Write;
        for attempt in 0..100 {
            let candidate = parent.join(format!(
                ".{}.{}.{}.tmp",
                name.to_string_lossy(),
                std::process::id(),
                attempt
            ));
            match options.open(&candidate) {
                Ok(mut file) => {
                    temp_path = Some(candidate.clone());
                    file.write_all(&next)?;
                    file.sync_all()?;
                    if fs::canonicalize(path)? != target || fs::read(path)? != expected {
                        bail!(
                            "Command destination or contents changed during save. Reopen before editing."
                        );
                    }
                    fs::rename(&candidate, target)
                        .with_context(|| format!("Replacing {}", target.display()))?;
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error).context("Creating temporary command registry"),
            }
        }
        bail!("Unable to reserve a temporary file for command registry")
    })();
    if result.is_err()
        && let Some(path) = temp_path
    {
        let _ = fs::remove_file(path);
    }
    result?;
    Ok(next)
}

fn read_snapshot(path: &Path) -> Result<(PathBuf, Vec<u8>, CommandConfig)> {
    let target = fs::canonicalize(path)
        .with_context(|| format!("Resolving command registry {}", path.display()))?;
    let bytes =
        fs::read(path).with_context(|| format!("Reading command registry {}", path.display()))?;
    let config: CommandConfig = serde_json::from_slice(&bytes)
        .with_context(|| format!("Parsing command registry {}", path.display()))?;
    config.validate()?;
    if fs::canonicalize(path)? != target {
        bail!("Command registry link changed during load; reopen and retry.");
    }
    Ok((target, bytes, config))
}

impl CommandEditor {
    pub fn busy(&self) -> bool {
        self.pending.is_some() || self.pending_reload.is_some()
    }

    /// False means the editor is saving/reloading or has unsaved changes
    /// that require explicit confirmation before it can be dismissed.
    pub fn close_requested(&mut self) -> bool {
        if self.busy() {
            return false;
        }
        if self.dirty {
            self.confirm_exit = true;
            return false;
        }
        true
    }

    /// The destructive confirmation must check the current worker state,
    /// not merely a stale dialog flag set before Save or Discard began.
    fn confirm_discard_and_close(&mut self) -> bool {
        if self.busy() || !self.dirty || !self.confirm_exit {
            return false;
        }
        self.confirm_exit = false;
        true
    }

    fn select_command(&mut self, index: usize) {
        self.selected = Some(index);
        self.argv_has_newline = false;
        self.args_text = match &self.config.commands[index].action {
            Action::Exec { args, .. } => {
                self.argv_has_newline = args.iter().any(|arg| arg.contains('\n'));
                if self.argv_has_newline {
                    String::new()
                } else {
                    format_argv_lines(args)
                }
            }
            Action::Hyprland { .. } => String::new(),
        };
    }

    fn move_group(&mut self, from: usize, to: usize) {
        if from < self.config.groups.len() && to < self.config.groups.len() && from != to {
            self.config.groups.swap(from, to);
            self.dirty = true;
        }
    }

    pub fn new(path: PathBuf) -> Result<Self> {
        let (target, original, config) = read_snapshot(&path)?;
        Ok(Self {
            path,
            target,
            original,
            config,
            selected: None,
            args_text: String::new(),
            argv_has_newline: false,
            error: None,
            dirty: false,
            pending: None,
            pending_reload: None,
            finish_after_save: false,
            finish_after_reload: false,
            done_ready: false,
            confirm_exit: false,
        })
    }

    fn begin_save(&mut self, finish: bool) {
        if self.pending.is_some() || self.pending_reload.is_some() {
            return;
        }
        // Validation and filesystem work happen inside write_atomic on
        // the save worker, never on the egui/UI thread.
        if !self.dirty {
            if finish {
                // The registry may have changed in another process while the
                // editor was open. Never return a stale in-memory snapshot to
                // the executable picker; reload and validate off the UI thread.
                self.begin_reload();
                self.finish_after_reload = true;
            }
            return;
        }
        self.error = None;
        self.confirm_exit = false;
        self.finish_after_save = finish;
        let path = self.path.clone();
        let target = self.target.clone();
        let original = std::mem::take(&mut self.original);
        let config = std::mem::replace(&mut self.config, CommandConfig::empty());
        let answer = Arc::new(Mutex::new(None));
        let sender = Arc::clone(&answer);
        self.pending = Some(answer);
        std::thread::spawn(move || {
            let result = write_atomic(&path, &target, &config, &original);
            if let Ok(mut slot) = sender.lock() {
                *slot = Some(SaveOutcome {
                    config,
                    original,
                    result,
                });
            }
        });
    }

    fn poll_save(&mut self, ctx: &egui::Context) {
        if let Some(pending) = &self.pending {
            let finished = pending.lock().ok().and_then(|mut value| value.take());
            if let Some(outcome) = finished {
                self.pending = None;
                self.config = outcome.config;
                match outcome.result {
                    Ok(bytes) => {
                        self.original = bytes;
                        self.dirty = false;
                        self.confirm_exit = false;
                        if self.finish_after_save {
                            self.done_ready = true;
                        }
                    }
                    Err(error) => {
                        self.original = outcome.original;
                        self.error = Some(format!("{error:#}"));
                    }
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(20));
            }
        }
    }

    fn begin_reload(&mut self) {
        if self.pending.is_some() || self.pending_reload.is_some() {
            return;
        }
        self.confirm_exit = false;
        self.finish_after_reload = false;
        let result: PendingReload = Arc::new(Mutex::new(None));
        let sender = Arc::clone(&result);
        let path = self.path.clone();
        self.pending_reload = Some(result);
        std::thread::spawn(move || {
            let snapshot = read_snapshot(&path);
            if let Ok(mut slot) = sender.lock() {
                *slot = Some(snapshot);
            }
        });
    }

    fn poll_reload(&mut self, ctx: &egui::Context) {
        if let Some(pending) = &self.pending_reload {
            let finished = pending.lock().ok().and_then(|mut state| state.take());
            if let Some(result) = finished {
                self.pending_reload = None;
                match result {
                    Ok((target, bytes, config)) => {
                        self.original = bytes;
                        self.config = config;
                        self.target = target;
                        self.selected = None;
                        self.args_text.clear();
                        self.argv_has_newline = false;
                        self.error = None;
                        self.dirty = false;
                        self.confirm_exit = false;
                        self.done_ready = self.finish_after_reload;
                    }
                    Err(error) => {
                        self.error = Some(format!("{error:#}"));
                        self.done_ready = false;
                    }
                }
                self.finish_after_reload = false;
            } else {
                ctx.request_repaint_after(Duration::from_millis(20));
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> EditorEvent {
        self.poll_save(ui.ctx());
        self.poll_reload(ui.ctx());
        if self.done_ready {
            return EditorEvent::Done;
        }
        let mut finish = false;
        let mut save = false;
        let mut discard = false;
        let mut close = false;
        let can_modify = self.pending.is_none() && self.pending_reload.is_none();
        ui.horizontal(|ui| {
            ui.heading("Edit command tree");
            ui.separator();
            save = ui
                .add_enabled(can_modify, egui::Button::new("Save"))
                .clicked();
            finish = ui
                .add_enabled(can_modify, egui::Button::new("Done"))
                .clicked();
            discard = ui
                .add_enabled(can_modify, egui::Button::new("Discard"))
                .clicked();
            close = ui
                .add_enabled(can_modify, egui::Button::new("Close"))
                .clicked();
        });
        if close {
            if self.close_requested() {
                return EditorEvent::Exit;
            }
            close = false;
        }
        if self.confirm_exit {
            ui.horizontal(|ui| {
                ui.colored_label(egui::Color32::YELLOW, "Unsaved edits will be lost.");
                if ui.button("Keep editing").clicked() {
                    self.confirm_exit = false;
                }
                if ui
                    .add_enabled(
                        can_modify && self.dirty,
                        egui::Button::new("Discard edits and close"),
                    )
                    .clicked()
                {
                    close = true;
                }
            });
            if close && self.confirm_discard_and_close() {
                return EditorEvent::Exit;
            }
        }
        if discard {
            // A slow filesystem or large JSON registry must not block egui.
            self.begin_reload();
            // An action click can be the final input event. Schedule the first
            // worker-result poll rather than waiting for an incidental redraw.
            ui.ctx().request_repaint_after(Duration::from_millis(20));
            return EditorEvent::Stay;
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::RED, error);
        }
        if self.pending.is_some() {
            ui.label("Saving commands…");
            return EditorEvent::Stay;
        }
        if self.pending_reload.is_some() {
            ui.label("Reloading command registry…");
            return EditorEvent::Stay;
        }
        if save || finish {
            self.begin_save(finish);
            // begin_save may also start a reload for an unmodified Done.
            // Either worker needs an explicit first poll when input stops.
            if self.busy() {
                ui.ctx().request_repaint_after(Duration::from_millis(20));
            }
            return EditorEvent::Stay;
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("Commands");
            if ui.button("New").clicked() {
                self.config.commands.push(CommandEntry {
                    sequence: String::new(),
                    label: "New command".into(),
                    action: Action::Exec {
                        program: String::new(),
                        args: Vec::new(),
                    },
                });
                self.select_command(self.config.commands.len() - 1);
                self.dirty = true;
            }
            if let Some(index) = self.selected {
                if ui.button("Up").clicked() && index > 0 {
                    self.config.commands.swap(index, index - 1);
                    self.selected = Some(index - 1);
                    self.dirty = true;
                }
                if ui.button("Down").clicked() && index + 1 < self.config.commands.len() {
                    self.config.commands.swap(index, index + 1);
                    self.selected = Some(index + 1);
                    self.dirty = true;
                }
                if ui.button("Delete").clicked() && index < self.config.commands.len() {
                    self.config.commands.remove(index);
                    self.selected = None;
                    self.args_text.clear();
                    self.argv_has_newline = false;
                    self.dirty = true;
                }
            }
        });

        let mut chosen = None;
        egui::ScrollArea::vertical()
            .id_salt("command-library")
            .max_height(140.0)
            .show_rows(ui, 30.0, self.config.commands.len(), |ui, rows| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for index in rows {
                    let entry = &self.config.commands[index];
                    ui.horizontal(|ui| {
                        ui.set_min_height(30.0);
                        if ui
                            .selectable_label(
                                self.selected == Some(index),
                                format!("{}  ·  {}", entry.sequence, entry.label),
                            )
                            .clicked()
                        {
                            chosen = Some(index);
                        }
                    });
                }
            });
        if let Some(index) = chosen {
            self.select_command(index);
        }
        if let Some(index) = self.selected
            && let Some(entry) = self.config.commands.get_mut(index)
        {
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Sequence");
                self.dirty |= ui.text_edit_singleline(&mut entry.sequence).changed();
                ui.label("Label");
                self.dirty |= ui.text_edit_singleline(&mut entry.label).changed();
            });
            let is_exec = matches!(entry.action, Action::Exec { .. });
            ui.horizontal(|ui| {
                if ui.radio(is_exec, "Program").clicked() && !is_exec {
                    entry.action = Action::Exec {
                        program: String::new(),
                        args: Vec::new(),
                    };
                    self.args_text.clear();
                    self.argv_has_newline = false;
                    self.dirty = true;
                }
                if ui.radio(!is_exec, "Hyprland").clicked() && is_exec {
                    entry.action = Action::Hyprland {
                        expression: "hl.dsp.window.fullscreen({ mode = \"fullscreen\" })".into(),
                        focus_original: true,
                    };
                    self.args_text.clear();
                    self.argv_has_newline = false;
                    self.dirty = true;
                }
            });
            match &mut entry.action {
                Action::Exec { program, args } => {
                    ui.horizontal(|ui| {
                        ui.label("Executable");
                        self.dirty |= ui.text_edit_singleline(program).changed();
                    });
                    if self.argv_has_newline {
                        ui.label(
                            "This command has an argument containing a newline.                              Its argv is preserved; edit it directly in commands.json.",
                        );
                    } else {
                        ui.label("Arguments (one argv element per line; no shell)");
                        if ui
                            .add(egui::TextEdit::multiline(&mut self.args_text).desired_rows(2))
                            .changed()
                        {
                            *args = parse_argv_lines(&self.args_text);
                            self.dirty = true;
                        }
                    }
                }
                Action::Hyprland {
                    expression,
                    focus_original,
                } => {
                    ui.label("Hyprland 0.55+ Lua dispatcher (hl.dsp.*)");
                    self.dirty |= ui.text_edit_singleline(expression).changed();
                    self.dirty |= ui
                        .checkbox(
                            focus_original,
                            "Refocus the original window before executing",
                        )
                        .changed();
                }
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("Groups");
            if ui.button("New group").clicked() {
                self.config.groups.push(Group {
                    prefix: String::new(),
                    label: String::new(),
                });
                self.dirty = true;
            }
        });
        ui.label("Optional labels for a command-prefix branch, such as w = Windows.");
        let mut remove_group = None;
        let mut move_group = None;
        let group_count = self.config.groups.len();
        egui::ScrollArea::vertical()
            .id_salt("command-groups")
            .max_height(160.0)
            .show_rows(ui, 34.0, self.config.groups.len(), |ui, rows| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for index in rows {
                    let group = &mut self.config.groups[index];
                    ui.horizontal(|ui| {
                        ui.set_min_height(34.0);
                        ui.label("Prefix");
                        self.dirty |= ui
                            .add(egui::TextEdit::singleline(&mut group.prefix).desired_width(75.0))
                            .changed();
                        ui.label("Label");
                        self.dirty |= ui
                            .add(egui::TextEdit::singleline(&mut group.label).desired_width(210.0))
                            .changed();
                        if ui.add_enabled(index > 0, egui::Button::new("Up")).clicked() {
                            move_group = Some((index, index - 1));
                        }
                        if ui
                            .add_enabled(index + 1 < group_count, egui::Button::new("Down"))
                            .clicked()
                        {
                            move_group = Some((index, index + 1));
                        }
                        if ui.button("Remove").clicked() {
                            remove_group = Some(index);
                        }
                    });
                }
            });
        if let Some((from, to)) = move_group {
            self.move_group(from, to);
        }
        if let Some(index) = remove_group {
            self.config.groups.remove(index);
            self.dirty = true;
        }
        EditorEvent::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_reorder_is_bounded_and_persists_without_touching_copy_library() {
        let dir = std::env::temp_dir().join(format!(
            "trievium-command-group-reorder-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        let copy_path = dir.join("items.json");
        fs::write(&copy_path, b"original clipboard data").unwrap();
        let mut config = CommandConfig::empty();
        for (prefix, label, sequence) in [("a", "Applications", "a t"), ("w", "Windows", "w f")] {
            config.groups.push(Group {
                prefix: prefix.into(),
                label: label.into(),
            });
            config.commands.push(CommandEntry {
                sequence: sequence.into(),
                label: label.into(),
                action: Action::Exec {
                    program: "true".into(),
                    args: Vec::new(),
                },
            });
        }
        fs::write(&path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        let mut editor = CommandEditor::new(path.clone()).unwrap();
        editor.move_group(0, 0);
        editor.move_group(0, 20);
        assert!(!editor.dirty, "invalid moves should leave the editor clean");
        editor.move_group(1, 0);
        assert_eq!(editor.config.groups[0].prefix, "w");
        assert!(editor.dirty);
        editor.move_group(0, 1);
        assert_eq!(editor.config.groups[0].prefix, "a");
        editor.move_group(1, 0);
        let ctx = egui::Context::default();
        editor.begin_save(false);
        for _ in 0..250 {
            editor.poll_save(&ctx);
            if !editor.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!editor.busy());
        assert!(!editor.dirty);
        assert!(editor.error.is_none());
        let persisted = CommandConfig::load(&path).unwrap();
        assert_eq!(persisted.groups[0].prefix, "w");
        assert_eq!(persisted.groups[1].prefix, "a");
        assert_eq!(persisted.commands[0].sequence, "a t");
        assert_eq!(persisted.commands[1].sequence, "w f");
        assert_eq!(fs::read(&copy_path).unwrap(), b"original clipboard data");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn native_move_buttons_persist_reordering_in_both_directions() {
        let dir = std::env::temp_dir().join(format!(
            "trievium-command-reorder-ui-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        let mut config = CommandConfig::empty();
        for (sequence, label) in [("a t", "Terminal"), ("w f", "Fullscreen")] {
            config.commands.push(CommandEntry {
                sequence: sequence.into(),
                label: label.into(),
                action: Action::Exec {
                    program: "true".into(),
                    args: Vec::new(),
                },
            });
        }
        fs::write(&path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        let mut editor = CommandEditor::new(path.clone()).unwrap();
        editor.select_command(1);
        let ctx = egui::Context::default();
        let mut timestamp = 0.0;
        let click = |editor: &mut CommandEditor, x: f32, ctx: &egui::Context, time: &mut f64| {
            let target = egui::pos2(x, 46.0);
            let frame = |editor: &mut CommandEditor, events: Vec<egui::Event>, time: f64| {
                ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 720.0),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        let _ = editor.ui(ui);
                    },
                )
                .drop_without_applying_deltas();
            };
            let pointer = |pressed| egui::Event::PointerButton {
                pos: target,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frame(editor, Vec::new(), *time);
            *time += 0.1;
            frame(
                editor,
                vec![egui::Event::PointerMoved(target), pointer(true)],
                *time,
            );
            *time += 0.1;
            frame(editor, vec![pointer(false)], *time);
            *time += 0.1;
        };
        click(&mut editor, 172.0, &ctx, &mut timestamp);
        assert_eq!(
            editor.config.commands[0].sequence, "w f",
            "native Up did not move the selected command"
        );
        assert_eq!(editor.selected, Some(0));
        editor.begin_save(false);
        for _ in 0..250 {
            editor.poll_save(&ctx);
            if !editor.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!editor.busy());
        assert!(!editor.dirty);
        assert_eq!(
            CommandConfig::load(&path).unwrap().commands[0].sequence,
            "w f"
        );

        click(&mut editor, 220.0, &ctx, &mut timestamp);
        assert_eq!(
            editor.config.commands[1].sequence, "w f",
            "native Down did not move the selected command"
        );
        assert_eq!(editor.selected, Some(1));
        editor.begin_save(false);
        for _ in 0..250 {
            editor.poll_save(&ctx);
            if !editor.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!editor.busy());
        assert!(!editor.dirty);
        let persisted = CommandConfig::load(&path).unwrap();
        assert_eq!(persisted.commands[0].sequence, "a t");
        assert_eq!(persisted.commands[1].sequence, "w f");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn native_delete_command_button_removes_selected_entry_and_persists() {
        let dir = std::env::temp_dir().join(format!(
            "trievium-command-delete-ui-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        let items = dir.join("items.json");
        fs::write(&items, b"untouched clipboard library").unwrap();
        let mut config = CommandConfig::empty();
        config.commands.push(CommandEntry {
            sequence: "a t".into(),
            label: "Delete me".into(),
            action: Action::Exec {
                program: "true".into(),
                args: Vec::new(),
            },
        });
        fs::write(&path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        let mut editor = CommandEditor::new(path.clone()).unwrap();
        editor.select_command(0);
        let ctx = egui::Context::default();
        let target = egui::pos2(291.0, 46.0);
        let frame = |editor: &mut CommandEditor, events: Vec<egui::Event>, time: f64| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 720.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let _ = editor.ui(ui);
                },
            )
            .drop_without_applying_deltas();
        };
        let pointer = |pressed| egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&mut editor, Vec::new(), 0.0);
        frame(
            &mut editor,
            vec![egui::Event::PointerMoved(target), pointer(true)],
            0.1,
        );
        frame(&mut editor, vec![pointer(false)], 0.2);
        assert_eq!(
            editor.config.commands.len(),
            0,
            "native Delete button did not remove the selected command"
        );
        assert_eq!(editor.selected, None);
        assert!(editor.dirty);
        editor.begin_save(false);
        for _ in 0..250 {
            editor.poll_save(&ctx);
            if !editor.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!editor.busy());
        assert!(!editor.dirty);
        assert_eq!(CommandConfig::load(&path).unwrap().commands.len(), 0);
        assert_eq!(fs::read(&items).unwrap(), b"untouched clipboard library");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn native_new_group_button_creates_editable_group_and_saves_it() {
        let dir = std::env::temp_dir().join(format!(
            "trievium-command-group-ui-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        let mut config = CommandConfig::empty();
        config.commands.push(CommandEntry {
            sequence: "a t".into(),
            label: "Terminal".into(),
            action: Action::Exec {
                program: "true".into(),
                args: Vec::new(),
            },
        });
        fs::write(&path, serde_json::to_vec_pretty(&config).unwrap()).unwrap();
        let mut editor = CommandEditor::new(path.clone()).unwrap();
        let ctx = egui::Context::default();
        let target = egui::pos2(121.0, 118.0);
        let frame = |editor: &mut CommandEditor, events: Vec<egui::Event>, time: f64| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 720.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let _ = editor.ui(ui);
                },
            )
            .drop_without_applying_deltas();
        };
        let pointer = |pressed| egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&mut editor, Vec::new(), 0.0);
        frame(
            &mut editor,
            vec![egui::Event::PointerMoved(target), pointer(true)],
            0.1,
        );
        frame(&mut editor, vec![pointer(false)], 0.2);
        assert_eq!(
            editor.config.groups.len(),
            1,
            "native New group did not create a draft at the expected coordinate"
        );
        assert!(editor.dirty);
        editor.config.groups[0].prefix = "a".into();
        editor.config.groups[0].label = "Applications".into();
        editor.begin_save(false);
        for _ in 0..250 {
            editor.poll_save(&ctx);
            if !editor.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!editor.busy());
        assert!(!editor.dirty);
        assert!(editor.error.is_none());
        let loaded = CommandConfig::load(&path).unwrap();
        assert_eq!(loaded.groups.len(), 1);
        assert_eq!(loaded.groups[0].label, "Applications");
        assert_eq!(loaded.commands.len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn native_new_command_button_creates_draft_without_overwriting_registry() {
        let dir = std::env::temp_dir().join(format!(
            "trievium-command-new-ui-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        crate::commands::initialize_empty(&path).unwrap();
        let before = fs::read(&path).unwrap();
        let mut editor = CommandEditor::new(path.clone()).unwrap();
        let ctx = egui::Context::default();
        // The New button sits alongside the Commands heading, below the
        // Save/Done toolbar in the undecorated native editor.
        let target = egui::pos2(145.0, 46.0);
        let frame = |editor: &mut CommandEditor, events: Vec<egui::Event>, time: f64| {
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280.0, 720.0),
                    )),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let _ = editor.ui(ui);
                },
            )
            .drop_without_applying_deltas();
        };
        let pointer = |pressed| egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(&mut editor, Vec::new(), 0.0);
        frame(
            &mut editor,
            vec![egui::Event::PointerMoved(target), pointer(true)],
            0.1,
        );
        frame(&mut editor, vec![pointer(false)], 0.2);
        assert_eq!(
            editor.config.commands.len(),
            1,
            "New command button did not create a draft at the expected UI coordinate"
        );
        assert!(editor.dirty);
        assert_eq!(editor.selected, Some(0));
        // New entries are intentionally incomplete until the user supplies a
        // sequence and executable. Save must fail without changing disk.
        editor.begin_save(false);
        for _ in 0..250 {
            editor.poll_save(&ctx);
            if !editor.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!editor.busy());
        assert!(editor.error.is_some());
        assert_eq!(fs::read(&path).unwrap(), before);

        // Completing the draft should clear that validation error on Save
        // and persist the command through the real atomic worker.
        editor.config.commands[0].sequence = "a t".into();
        editor.config.commands[0].label = "Launch terminal".into();
        editor.config.commands[0].action = Action::Exec {
            program: "true".into(),
            args: Vec::new(),
        };
        editor.begin_save(false);
        for _ in 0..250 {
            editor.poll_save(&ctx);
            if !editor.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!editor.busy());
        assert!(!editor.dirty);
        assert!(editor.error.is_none());
        assert_eq!(CommandConfig::load(&path).unwrap().commands.len(), 1);
        assert_ne!(fs::read(&path).unwrap(), before);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn native_done_button_click_returns_editor_to_picker_without_disk_mutation() {
        let dir = std::env::temp_dir().join(format!(
            "trievium-command-done-ui-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        crate::commands::initialize_empty(&path).unwrap();
        let before = fs::read(&path).unwrap();
        let mut editor = CommandEditor::new(path.clone()).unwrap();
        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
        let target = egui::pos2(250.0, 15.0);
        let run = |editor: &mut CommandEditor, events: Vec<egui::Event>, time: f64| {
            let mut action = EditorEvent::Stay;
            ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen_rect),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| action = editor.ui(ui),
            )
            .drop_without_applying_deltas();
            action
        };
        let pointer = |pressed| egui::Event::PointerButton {
            pos: target,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        assert!(matches!(
            run(&mut editor, Vec::new(), 0.0),
            EditorEvent::Stay
        ));
        assert!(matches!(
            run(
                &mut editor,
                vec![egui::Event::PointerMoved(target), pointer(true)],
                0.1
            ),
            EditorEvent::Stay
        ));
        assert!(matches!(
            run(&mut editor, vec![pointer(false)], 0.2),
            EditorEvent::Stay
        ));

        // The click starts a real fresh-snapshot reload on a worker.
        assert!(editor.busy() || editor.done_ready);
        for step in 0..250 {
            if matches!(
                run(&mut editor, Vec::new(), 0.3 + f64::from(step) * 0.02),
                EditorEvent::Done
            ) {
                assert!(!editor.busy());
                assert_eq!(fs::read(&path).unwrap(), before);
                fs::remove_dir_all(dir).unwrap();
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("egui Done click did not complete the editor-to-picker transition");
    }

    #[test]
    fn argv_lines_do_not_add_an_unintended_trailing_empty_argument() {
        assert!(parse_argv_lines("").is_empty());
        assert_eq!(parse_argv_lines("--title\nMy App\n"), ["--title", "My App"]);
        assert_eq!(parse_argv_lines("--title\nMy App"), ["--title", "My App"]);
        assert_eq!(
            parse_argv_lines("one\n\nthree\n"),
            ["one", "", "three"],
            "blank interior lines must remain explicit empty argv elements"
        );
        assert_eq!(parse_argv_lines("\n"), [""]);
        assert_eq!(
            parse_argv_lines("--title\nMy App\n\n"),
            ["--title", "My App", ""]
        );
    }

    #[test]
    fn argv_render_and_parse_preserve_empty_arguments() {
        let examples: Vec<Vec<String>> = vec![
            vec![],
            vec!["".into()],
            vec!["".into(), "".into()],
            vec!["--title".into(), "two words".into()],
            vec!["--title".into(), "".into()],
            vec!["".into(), "--arg".into(), "".into()],
        ];
        for args in examples {
            let rendered = format_argv_lines(&args);
            assert_eq!(parse_argv_lines(&rendered), args);
        }
        assert_eq!(format_argv_lines(&["".into()]), "\n");
        assert_eq!(
            format_argv_lines(&["--title".into(), "".into()]),
            "--title\n\n"
        );
    }

    #[test]
    fn argv_editor_cache_only_rebuilds_when_selection_changes() {
        let dir =
            std::env::temp_dir().join(format!("trievium-command-argv-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        crate::commands::initialize_empty(&path).unwrap();
        let mut editor = CommandEditor::new(path).unwrap();
        editor.config.commands.push(CommandEntry {
            sequence: "a t".into(),
            label: "Terminal".into(),
            action: Action::Exec {
                program: "kitty".into(),
                args: vec!["--title".into(), "two words".into()],
            },
        });
        editor.config.commands.push(CommandEntry {
            sequence: "w f".into(),
            label: "Fullscreen".into(),
            action: Action::Hyprland {
                expression: "hl.dsp.window.fullscreen({ mode = \"fullscreen\" })".into(),
                focus_original: true,
            },
        });

        editor.select_command(0);
        assert_eq!(editor.args_text, "--title\ntwo words\n");
        // The cached edit buffer is not recomputed on repaint or when the
        // user edits the underlying command before explicitly reselecting.
        editor.args_text.push_str("--class");
        assert_eq!(editor.args_text, "--title\ntwo words\n--class");
        editor.select_command(1);
        assert!(editor.args_text.is_empty());
        editor.select_command(0);
        assert_eq!(editor.args_text, "--title\ntwo words\n");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn newline_argument_is_protected_from_lossy_line_edits() {
        let dir = std::env::temp_dir().join(format!(
            "trievium-command-multiline-argv-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        crate::commands::initialize_empty(&path).unwrap();
        let mut editor = CommandEditor::new(path.clone()).unwrap();
        editor.config.commands.push(CommandEntry {
            sequence: "m l".into(),
            label: "Multiline".into(),
            action: Action::Exec {
                program: "printf".into(),
                args: vec!["line 1\nline 2".into(), "".into()],
            },
        });
        editor.select_command(0);
        assert!(editor.argv_has_newline);
        assert!(editor.args_text.is_empty());

        editor.config.commands[0].label = "Renamed".into();
        editor.dirty = true;
        editor.begin_save(false);
        let ctx = egui::Context::default();
        for _ in 0..250 {
            editor.poll_save(&ctx);
            if !editor.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!editor.busy());
        assert!(editor.error.is_none());
        let reloaded = CommandConfig::load(&path).unwrap();
        assert_eq!(reloaded.commands[0].label, "Renamed");
        match &reloaded.commands[0].action {
            Action::Exec { args, .. } => {
                assert_eq!(args, &["line 1\nline 2", ""]);
            }
            Action::Hyprland { .. } => panic!("expected argv action"),
        }
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn async_save_restores_editor_state_on_success_and_conflict() {
        let dir = std::env::temp_dir().join(format!(
            "trievium-command-worker-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        crate::commands::initialize_empty(&path).unwrap();
        let mut editor = CommandEditor::new(path.clone()).unwrap();
        editor.config.commands.push(CommandEntry {
            sequence: "a t".into(),
            label: "Terminal".into(),
            action: Action::Exec {
                program: "kitty".into(),
                args: Vec::new(),
            },
        });
        editor.dirty = true;
        editor.begin_save(false);
        assert!(editor.config.commands.is_empty());
        let ctx = egui::Context::default();
        for _ in 0..250 {
            editor.poll_save(&ctx);
            if !editor.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!editor.busy(), "worker save did not complete");
        assert!(!editor.dirty);
        assert_eq!(editor.config.commands.len(), 1);
        assert_eq!(CommandConfig::load(&path).unwrap().commands.len(), 1);

        editor.config.commands[0].label = "Changed in editor".into();
        editor.dirty = true;
        let external = fs::read(&path).unwrap();
        fs::write(&path, b"external edit").unwrap();
        editor.begin_save(false);
        for _ in 0..250 {
            editor.poll_save(&ctx);
            if !editor.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!editor.busy(), "conflicting worker save did not finish");
        assert!(editor.dirty);
        assert!(editor.error.is_some());
        assert_eq!(editor.config.commands[0].label, "Changed in editor");
        assert_eq!(editor.original, external);
        assert_eq!(fs::read(&path).unwrap(), b"external edit");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unmodified_done_reads_external_changes_before_returning_to_picker() {
        let dir = std::env::temp_dir().join(format!(
            "trievium-command-done-reload-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        crate::commands::initialize_empty(&path).unwrap();
        let mut editor = CommandEditor::new(path.clone()).unwrap();

        let mut external = CommandConfig::empty();
        external.commands.push(CommandEntry {
            sequence: "w f".into(),
            label: "Changed elsewhere".into(),
            action: Action::Exec {
                program: "true".into(),
                args: Vec::new(),
            },
        });
        fs::write(&path, serde_json::to_vec_pretty(&external).unwrap()).unwrap();

        editor.begin_save(true);
        assert!(editor.busy());
        assert!(!editor.done_ready);
        let ctx = egui::Context::default();
        for _ in 0..250 {
            editor.poll_reload(&ctx);
            if !editor.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!editor.busy(), "external reload worker failed to complete");
        assert!(
            editor.done_ready,
            "Done must finish after validating the new snapshot"
        );
        assert_eq!(editor.config.commands.len(), 1);
        assert_eq!(editor.config.commands[0].label, "Changed elsewhere");
        assert_eq!(editor.original, fs::read(&path).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unmodified_done_rejects_broken_external_config_without_exiting() {
        let dir = std::env::temp_dir().join(format!(
            "trievium-command-done-invalid-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        crate::commands::initialize_empty(&path).unwrap();
        let mut editor = CommandEditor::new(path.clone()).unwrap();

        fs::write(&path, b"invalid external JSON").unwrap();
        editor.begin_save(true);
        assert!(!editor.done_ready);
        let ctx = egui::Context::default();
        for _ in 0..250 {
            editor.poll_reload(&ctx);
            if !editor.busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!editor.busy());
        assert!(!editor.done_ready);
        assert!(editor.error.is_some());
        assert!(editor.config.commands.is_empty());
        assert_eq!(fs::read(&path).unwrap(), b"invalid external JSON");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn editor_never_exits_mid_save_or_discards_unsaved_changes_silently() {
        let dir = std::env::temp_dir().join(format!(
            "trievium-command-close-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        crate::commands::initialize_empty(&path).unwrap();
        let mut editor = CommandEditor::new(path).unwrap();

        editor.dirty = true;
        assert!(!editor.close_requested());
        assert!(editor.confirm_exit);

        // Even a stale confirmation cannot exit while a save or reload owns
        // the configuration data, nor can it bypass the dirty-state guard.
        editor.pending_reload = Some(Arc::new(Mutex::new(None)));
        assert!(editor.busy());
        assert!(!editor.close_requested());
        assert!(!editor.confirm_discard_and_close());
        editor.pending_reload = None;

        editor.pending = Some(Arc::new(Mutex::new(None)));
        assert!(editor.busy());
        assert!(!editor.close_requested());
        assert!(!editor.confirm_discard_and_close());
        editor.pending = None;

        // Explicit confirmation is allowed only while unsaved edits remain.
        assert!(editor.confirm_discard_and_close());
        assert!(!editor.confirm_discard_and_close());
        editor.confirm_exit = true;
        editor.dirty = false;
        assert!(!editor.confirm_discard_and_close());
        assert!(editor.close_requested());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn new_editor_reads_current_registry_instead_of_stale_picker_state() {
        let dir = std::env::temp_dir().join(format!(
            "trievium-command-snapshot-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("commands.json");
        crate::commands::initialize_empty(&path).unwrap();
        let mut changed = CommandConfig::empty();
        changed.commands.push(CommandEntry {
            label: "Updated externally".into(),
            sequence: "a t".into(),
            action: Action::Exec {
                program: "kitty".into(),
                args: Vec::new(),
            },
        });
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        let editor = CommandEditor::new(path.clone()).unwrap();
        assert_eq!(editor.config.commands[0].label, "Updated externally");
        assert_eq!(editor.original, fs::read(&path).unwrap());
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn atomic_save_preserves_config_store_symlink() {
        use std::os::unix::fs::symlink;

        let dir = std::env::temp_dir().join(format!(
            "trievium-command-symlink-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("dotfiles")).unwrap();
        fs::create_dir_all(dir.join("config")).unwrap();
        let target = dir.join("dotfiles/commands.json");
        let link = dir.join("config/commands.json");
        fs::write(
            &target,
            serde_json::to_vec(&CommandConfig::empty()).unwrap(),
        )
        .unwrap();
        symlink(&target, &link).unwrap();
        let editor = CommandEditor::new(link.clone()).unwrap();
        assert_eq!(editor.target, fs::canonicalize(&target).unwrap());

        let mut updated = editor.config.clone();
        updated.commands.push(CommandEntry {
            sequence: "a t".into(),
            label: "Terminal".into(),
            action: Action::Exec {
                program: "kitty".into(),
                args: Vec::new(),
            },
        });
        write_atomic(&link, &editor.target, &updated, &editor.original).unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_link(&link).unwrap(), target);
        assert_eq!(CommandConfig::load(&target).unwrap().commands.len(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn changing_config_store_link_never_overwrites_new_target() {
        use std::os::unix::fs::symlink;

        let dir = std::env::temp_dir().join(format!(
            "trievium-command-retarget-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let original = dir.join("original.json");
        let replacement = dir.join("replacement.json");
        let link = dir.join("commands.json");
        let base = serde_json::to_vec(&CommandConfig::empty()).unwrap();
        fs::write(&original, &base).unwrap();
        fs::write(&replacement, &base).unwrap();
        symlink(&original, &link).unwrap();
        let editor = CommandEditor::new(link.clone()).unwrap();
        fs::remove_file(&link).unwrap();
        symlink(&replacement, &link).unwrap();

        let result = write_atomic(&link, &editor.target, &editor.config, &editor.original);
        assert!(result.is_err());
        assert_eq!(fs::read(&replacement).unwrap(), base);
        assert_eq!(fs::read(&original).unwrap(), base);
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn atomic_command_save_detects_conflicts_without_touching_copy_data() {
        let dir = std::env::temp_dir().join(format!(
            "trievium-command-editor-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let items = dir.join("items.json");
        fs::write(&items, b"secret copy items").unwrap();
        let path = dir.join("commands.json");
        crate::commands::initialize_empty(&path).unwrap();
        let before = fs::read(&path).unwrap();
        let mut config = CommandConfig::empty();
        config.commands.push(CommandEntry {
            label: "Launcher".into(),
            sequence: "a t".into(),
            action: Action::Exec {
                program: "kitty".into(),
                args: Vec::new(),
            },
        });
        let target = fs::canonicalize(&path).unwrap();
        let updated = write_atomic(&path, &target, &config, &before).unwrap();
        assert_eq!(fs::read(&path).unwrap(), updated);
        fs::write(&path, b"externally changed").unwrap();
        assert!(write_atomic(&path, &target, &config, &updated).is_err());
        assert_eq!(fs::read(&items).unwrap(), b"secret copy items");
        fs::remove_dir_all(dir).unwrap();
    }
}

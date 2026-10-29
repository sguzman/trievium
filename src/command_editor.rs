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
        self.args_text = match &self.config.commands[index].action {
            Action::Exec { args, .. } => args.join("\n"),
            Action::Hyprland { .. } => String::new(),
        };
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
            let done_button = ui.add_enabled(can_modify, egui::Button::new("Done"));
            #[cfg(debug_assertions)]
            {
                use std::sync::atomic::{AtomicUsize, Ordering};
                static QA_SAMPLES: AtomicUsize = AtomicUsize::new(0);
                if std::env::var_os("TRIEVIUM_QA_POINTER").is_some()
                    && QA_SAMPLES.fetch_add(1, Ordering::Relaxed) < 120
                {
                    let pointer = ui.ctx().input(|i| {
                        (i.pointer.hover_pos(), i.pointer.any_click(), i.pointer.any_down())
                    });
                    eprintln!(
                        "Trievium QA pointer: done={:?}, hover={:?}, click={}, down={}, hit={}",
                        done_button.rect,
                        pointer.0,
                        pointer.1,
                        pointer.2,
                        done_button.clicked()
                    );
                }
            }
            finish = done_button.clicked();
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
                    self.dirty = true;
                }
                if ui.radio(!is_exec, "Hyprland").clicked() && is_exec {
                    entry.action = Action::Hyprland {
                        expression: "hl.dsp.window.fullscreen({ mode = \"fullscreen\" })".into(),
                        focus_original: true,
                    };
                    self.args_text.clear();
                    self.dirty = true;
                }
            });
            match &mut entry.action {
                Action::Exec { program, args } => {
                    ui.horizontal(|ui| {
                        ui.label("Executable");
                        self.dirty |= ui.text_edit_singleline(program).changed();
                    });
                    ui.label("Arguments (one argv element per line; no shell)");
                    if ui
                        .add(egui::TextEdit::multiline(&mut self.args_text).desired_rows(2))
                        .changed()
                    {
                        *args = if self.args_text.is_empty() {
                            Vec::new()
                        } else {
                            self.args_text.split('\n').map(str::to_owned).collect()
                        };
                        self.dirty = true;
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
                        self.dirty |= ui.text_edit_singleline(&mut group.label).changed();
                        if ui.button("Remove").clicked() {
                            remove_group = Some(index);
                        }
                    });
                }
            });
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
        assert_eq!(editor.args_text, "--title\ntwo words");
        // The cached edit buffer is not recomputed on repaint or when the
        // user edits the underlying command before explicitly reselecting.
        editor.args_text.push_str("\n--class");
        assert_eq!(editor.args_text, "--title\ntwo words\n--class");
        editor.select_command(1);
        assert!(editor.args_text.is_empty());
        editor.select_command(0);
        assert_eq!(editor.args_text, "--title\ntwo words");
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

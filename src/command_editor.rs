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

pub struct CommandEditor {
    path: PathBuf,
    original: Vec<u8>,
    pub config: CommandConfig,
    selected: Option<usize>,
    error: Option<String>,
    dirty: bool,
    pending: Option<Arc<Mutex<Option<Result<Vec<u8>>>>>>,
    finish_after_save: bool,
    done_ready: bool,
}

fn write_atomic(path: &Path, config: &CommandConfig, expected: &[u8]) -> Result<Vec<u8>> {
    config.validate()?;
    if fs::read(path).with_context(|| format!("Reading {}", path.display()))? != expected {
        bail!("Commands changed outside this editor. Discard/reopen before saving.");
    }
    let mut next = serde_json::to_vec_pretty(config)?;
    next.push(b'\n');
    let parent = path
        .parent()
        .context("Command file has no parent directory")?;
    let name = path.file_name().context("Command file has no filename")?;
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
                    temp_path = Some(candidate);
                    file.write_all(&next)?;
                    file.sync_all()?;
                    if fs::read(path)? != expected {
                        bail!("Commands changed during the save. Reopen before editing.");
                    }
                    fs::rename(&candidate, path)
                        .with_context(|| format!("Replacing {}", path.display()))?;
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error).context("Creating temporary command registry"),
            }
        }
        bail!("Unable to reserve a temporary file for command registry")
    })();
    if result.is_err() {
        if let Some(path) = temp_path {
            let _ = fs::remove_file(path);
        }
    }
    result?;
    Ok(next)
}

impl CommandEditor {
    pub fn new(path: PathBuf, config: CommandConfig) -> Result<Self> {
        let original = fs::read(&path)
            .with_context(|| format!("Reading command registry {}", path.display()))?;
        Ok(Self {
            path,
            original,
            config,
            selected: None,
            error: None,
            dirty: false,
            pending: None,
            finish_after_save: false,
            done_ready: false,
        })
    }

    fn begin_save(&mut self, finish: bool) {
        if self.pending.is_some() {
            return;
        }
        if let Err(error) = self.config.validate() {
            self.error = Some(format!("{error:#}"));
            return;
        }
        if !self.dirty {
            if finish {
                self.done_ready = true;
            }
            return;
        }
        self.error = None;
        self.finish_after_save = finish;
        let path = self.path.clone();
        let original = self.original.clone();
        let config = self.config.clone();
        let answer = Arc::new(Mutex::new(None));
        let sender = Arc::clone(&answer);
        self.pending = Some(answer);
        std::thread::spawn(move || {
            let result = write_atomic(&path, &config, &original);
            if let Ok(mut slot) = sender.lock() {
                *slot = Some(result);
            }
        });
    }

    fn poll_save(&mut self, ctx: &egui::Context) {
        if let Some(pending) = &self.pending {
            let finished = pending.lock().ok().and_then(|mut value| value.take());
            if let Some(result) = finished {
                self.pending = None;
                match result {
                    Ok(bytes) => {
                        self.original = bytes;
                        self.dirty = false;
                        if self.finish_after_save {
                            self.done_ready = true;
                        }
                    }
                    Err(error) => self.error = Some(format!("{error:#}")),
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(20));
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> EditorEvent {
        self.poll_save(ui.ctx());
        if self.done_ready {
            return EditorEvent::Done;
        }
        let mut finish = false;
        let mut save = false;
        let mut discard = false;
        let mut close = false;
        ui.horizontal(|ui| {
            ui.heading("Edit command tree");
            ui.separator();
            save = ui
                .add_enabled(self.pending.is_none(), egui::Button::new("Save"))
                .clicked();
            finish = ui
                .add_enabled(self.pending.is_none(), egui::Button::new("Done"))
                .clicked();
            discard = ui
                .add_enabled(self.pending.is_none(), egui::Button::new("Discard"))
                .clicked();
            close = ui.button("Close").clicked();
        });
        if close {
            return EditorEvent::Exit;
        }
        if discard {
            match CommandConfig::load(&self.path) {
                Ok(config) => {
                    self.original = fs::read(&self.path).unwrap_or_default();
                    self.config = config;
                    self.selected = None;
                    self.error = None;
                    self.dirty = false;
                }
                Err(error) => self.error = Some(format!("{error:#}")),
            }
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::RED, error);
        }
        if self.pending.is_some() {
            ui.label("Saving commands…");
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
                self.selected = Some(self.config.commands.len() - 1);
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
                    self.dirty = true;
                }
            }
        });

        egui::ScrollArea::vertical()
            .id_salt("command-library")
            .max_height(140.0)
            .show(ui, |ui| {
                for (index, entry) in self.config.commands.iter().enumerate() {
                    if ui
                        .selectable_label(
                            self.selected == Some(index),
                            format!("{}  ·  {}", entry.sequence, entry.label),
                        )
                        .clicked()
                    {
                        self.selected = Some(index);
                    }
                }
            });
        if let Some(index) = self.selected {
            if let Some(entry) = self.config.commands.get_mut(index) {
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
                        self.dirty = true;
                    }
                    if ui.radio(!is_exec, "Hyprland").clicked() && is_exec {
                        entry.action = Action::Hyprland {
                            dispatcher: String::new(),
                            args: String::new(),
                            focus_original: false,
                        };
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
                        let mut value = args.join("\n");
                        if ui
                            .add(egui::TextEdit::multiline(&mut value).desired_rows(2))
                            .changed()
                        {
                            *args = if value.is_empty() {
                                Vec::new()
                            } else {
                                value.split('\n').map(str::to_owned).collect()
                            };
                            self.dirty = true;
                        }
                    }
                    Action::Hyprland {
                        dispatcher,
                        args,
                        focus_original,
                    } => {
                        ui.horizontal(|ui| {
                            ui.label("Dispatcher");
                            self.dirty |= ui.text_edit_singleline(dispatcher).changed();
                            ui.label("Arguments");
                            self.dirty |= ui.text_edit_singleline(args).changed();
                        });
                        self.dirty |= ui
                            .checkbox(
                                focus_original,
                                "Refocus the original window before executing",
                            )
                            .changed();
                    }
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
            .show(ui, |ui| {
                for (index, group) in self.config.groups.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
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
        let updated = write_atomic(&path, &config, &before).unwrap();
        assert_eq!(fs::read(&path).unwrap(), updated);
        fs::write(&path, b"externally changed").unwrap();
        assert!(write_atomic(&path, &config, &updated).is_err());
        assert_eq!(fs::read(&items).unwrap(), b"secret copy items");
        fs::remove_dir_all(dir).unwrap();
    }
}

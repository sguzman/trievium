//! Native CRUD interface for local selections. The picker does not use this
//! code during the shortcut hot path. No importers or network connections.
use crate::config::{self, Config, Entry};
use crate::shortcut_cache;
use crate::trie::Keymap;
use anyhow::{Context, Result};
use eframe::egui;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Default, Clone, PartialEq, Eq)]
struct EntryForm {
    label: String,
    text: String,
    key: String,
    priority: i32,
}
impl EntryForm {
    fn from_entry(entry: &Entry) -> Self {
        Self {
            label: entry.label.clone(),
            text: entry.text.clone(),
            key: entry.key.clone().unwrap_or_default(),
            priority: entry.priority,
        }
    }
    fn to_entry(&self) -> Entry {
        Entry {
            label: self.label.trim().to_owned(),
            text: self.text.clone(),
            key: (!self.key.trim().is_empty()).then(|| self.key.trim().to_ascii_uppercase()),
            priority: self.priority,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ManagerAction {
    Stay,
    Back,
    Exit,
}

pub struct Manager {
    path: PathBuf,
    cache_path: PathBuf,
    original: Vec<u8>,
    config: Config,
    map: Keymap,
    selected: Option<usize>,
    form: EntryForm,
    confirm_delete: bool,
    message: Option<String>,
    error: Option<String>,
}

impl Manager {
    pub fn new(path: PathBuf, cache_path: PathBuf, config: Config, map: Keymap) -> Result<Self> {
        let original = fs::read(&path).with_context(|| format!("Reading {}", path.display()))?;
        Ok(Self {
            path,
            cache_path,
            original,
            config,
            map,
            selected: None,
            form: EntryForm::default(),
            confirm_delete: false,
            message: None,
            error: None,
        })
    }

    pub fn into_data(self) -> (Config, Keymap) {
        (self.config, self.map)
    }

    fn select(&mut self, index: Option<usize>) {
        self.selected = index;
        self.form = index
            .map(|i| EntryForm::from_entry(&self.config.entries[i]))
            .unwrap_or_default();
        self.confirm_delete = false;
        self.message = None;
        self.error = None;
    }

    fn dirty(&self) -> bool {
        match self.selected {
            Some(index) => self.form != EntryForm::from_entry(&self.config.entries[index]),
            None => self.form != EntryForm::default(),
        }
    }

    /// Recalculate before writing. A collision or duplicate never modifies
    /// local files, and the original configuration is not changed in memory.
    fn commit(&mut self, replacement: Config, next_selected: Option<usize>) -> Result<()> {
        replacement.validate()?;
        let previous: BTreeMap<String, String> = self
            .config
            .entries
            .iter()
            .enumerate()
            .map(|(i, entry)| (entry.label.clone(), self.map.key(i).to_owned()))
            .collect();
        let new_map = Keymap::build_with_previous(&replacement.entries, &previous)?;
        let new_bytes = config::save_atomic(&self.path, &replacement, &self.original)?;
        self.original = new_bytes;
        // The cache is best effort; failing to persist mnemonics cannot
        // roll back the successfully saved user config.
        if let Err(error) = shortcut_cache::save(&self.cache_path, &replacement.entries, &new_map) {
            eprintln!("trievium: shortcut cache could not be updated: {error:#}");
        }
        self.config = replacement;
        self.map = new_map;
        self.select(next_selected);
        self.message = Some("Saved".into());
        Ok(())
    }

    fn save_form(&mut self) -> Result<()> {
        let mut replacement = self.config.clone();
        let entry = self.form.to_entry();
        let target = if let Some(index) = self.selected {
            replacement.entries[index] = entry;
            index
        } else {
            replacement.entries.push(entry);
            replacement.entries.len() - 1
        };
        self.commit(replacement, Some(target))
    }

    fn delete_entry(&mut self) -> Result<()> {
        let index = self.selected.context("Select an entry to delete")?;
        let mut replacement = self.config.clone();
        replacement.entries.remove(index);
        self.commit(replacement, None)
    }

    /// Keep the primary actions at the top so they are available even if a
    /// compositor ignores the requested editor resize.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> ManagerAction {
        let mut action = ManagerAction::Stay;
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(18))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Trievium");
                    ui.label(egui::RichText::new("Entries").weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            action = ManagerAction::Exit;
                        }
                        // Done must NEVER trap the user. Saved changes
                        // remain on disk; unsaved form edits are discarded.
                        if ui.button("Done").clicked() {
                            action = ManagerAction::Back;
                        }
                    });
                });
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("+ New").clicked() {
                        if self.dirty() {
                            self.error = Some("Save or discard the current edits first.".into());
                        } else {
                            self.select(None);
                        }
                    }
                    if ui.button("Save").clicked()
                        && let Err(error) = self.save_form()
                    {
                        self.error = Some(format!("{error:#}"));
                        self.message = None;
                    }
                    if ui.button("Discard").clicked() {
                        self.select(self.selected);
                    }
                    if self.selected.is_some() && ui.button("Delete").clicked() {
                        self.confirm_delete = true;
                    }
                    if self.confirm_delete {
                        ui.label("Delete this entry?");
                        if ui.button("Confirm delete").clicked()
                            && let Err(error) = self.delete_entry()
                        {
                            self.error = Some(format!("{error:#}"));
                        }
                        if ui.button("Cancel").clicked() {
                            self.confirm_delete = false;
                        }
                    }
                });
                if let Some(error) = &self.error {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                } else if let Some(message) = &self.message {
                    ui.label(message);
                }
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(10.0);

                // Both narrow and fully resized layouts must remain usable.
                // A vertical scroll view is critical for the initial 560x175
                // picker surface before Hyprland applies the resize.
                egui::ScrollArea::vertical()
                    .id_salt("manager_body")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut change_selection = None;
                        if ui.available_width() >= 675.0 {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(210.0);
                                    self.entry_list(ui, &mut change_selection);
                                });
                                ui.add_space(12.0);
                                ui.separator();
                                ui.add_space(12.0);
                                ui.vertical(|ui| {
                                    ui.set_min_width(380.0);
                                    self.entry_fields(ui);
                                });
                            });
                        } else {
                            // Side-by-side min-width columns previously
                            // overflowed to the right, hiding the form.
                            self.entry_list(ui, &mut change_selection);
                            ui.add_space(14.0);
                            ui.separator();
                            ui.add_space(14.0);
                            self.entry_fields(ui);
                        }
                        if let Some(index) = change_selection {
                            if self.dirty() {
                                self.error = Some("Save or discard current edits first.".into());
                            } else {
                                self.select(Some(index));
                            }
                        }
                    });
            });
        action
    }

    fn entry_list(&mut self, ui: &mut egui::Ui, change: &mut Option<usize>) {
        ui.label(egui::RichText::new("LIBRARY").small().strong());
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .id_salt("manager_entries")
            .max_height(190.0)
            .show(ui, |ui| {
                for (index, entry) in self.config.entries.iter().enumerate() {
                    let label = format!("{}    {}", self.map.key(index), entry.label);
                    if ui
                        .selectable_label(self.selected == Some(index), label)
                        .clicked()
                    {
                        *change = Some(index);
                    }
                }
            });
    }

    fn entry_fields(&mut self, ui: &mut egui::Ui) {
        ui.heading(if self.selected.is_some() {
            "Edit entry"
        } else {
            "New entry"
        });
        ui.add_space(10.0);
        ui.label("Name");
        ui.add(egui::TextEdit::singleline(&mut self.form.label).desired_width(f32::INFINITY));
        ui.add_space(10.0);
        ui.label("Text to copy");
        ui.add(
            egui::TextEdit::multiline(&mut self.form.text)
                .desired_rows(8)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            ui.vertical(|ui| {
                ui.label("Pinned shortcut");
                ui.add(
                    egui::TextEdit::singleline(&mut self.form.key)
                        .desired_width(125.0)
                        .char_limit(4),
                );
            });
            ui.add_space(18.0);
            ui.vertical(|ui| {
                ui.label("Automatic priority");
                ui.add(egui::DragValue::new(&mut self.form.priority));
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config() -> (PathBuf, PathBuf, Config, Keymap) {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("trievium-manager-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("items.json");
        let cache = root.join("keys.json");
        let config = Config {
            version: 1,
            entries: vec![Entry {
                label: "Greeting".into(),
                text: "Hello!".into(),
                key: Some("G".into()),
                priority: 0,
            }],
        };
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let map = Keymap::build(&config.entries).unwrap();
        (path, cache, config, map)
    }

    #[test]
    fn add_edit_delete_and_reload() {
        let (path, cache, config, map) = temp_config();
        let mut mgr = Manager::new(path.clone(), cache, config, map).unwrap();
        mgr.form = EntryForm {
            label: "Address".into(),
            text: "123 Example St".into(),
            key: "a".into(),
            priority: 80,
        };
        mgr.save_form().unwrap();
        assert_eq!(
            Config::load(&path).unwrap().entries[1].key.as_deref(),
            Some("A")
        );
        mgr.form.text = "456 Example St".into();
        mgr.save_form().unwrap();
        assert_eq!(
            Config::load(&path).unwrap().entries[1].text,
            "456 Example St"
        );
        mgr.delete_entry().unwrap();
        assert_eq!(Config::load(&path).unwrap().entries.len(), 1);
        mgr.select(Some(0));
        mgr.delete_entry().unwrap();
        assert!(Config::load(&path).unwrap().entries.is_empty());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn done_and_close_are_always_available_when_form_is_dirty() {
        // Verify the policy independently of the native window: unsaved data
        // is not persisted just because the form is edited.
        let (path, cache, config, map) = temp_config();
        let mut manager = Manager::new(path.clone(), cache, config, map).unwrap();
        manager.form.label = "Unsaved".into();
        assert!(manager.dirty());
        assert_eq!(Config::load(&path).unwrap().entries[0].label, "Greeting");
        manager.select(None);
        assert!(!manager.dirty());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn invalid_prefix_collision_cannot_overwrite_config() {
        let (path, cache, config, map) = temp_config();
        let initial = fs::read(&path).unwrap();
        let mut mgr = Manager::new(path.clone(), cache, config, map).unwrap();
        mgr.form = EntryForm {
            label: "Conflicting".into(),
            text: "Text".into(),
            key: "GA".into(),
            priority: 0,
        };
        assert!(mgr.save_form().is_err());
        assert_eq!(fs::read(&path).unwrap(), initial);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn refuses_external_edits_without_overwriting() {
        let (path, cache, config, map) = temp_config();
        let mut mgr = Manager::new(path.clone(), cache, config, map).unwrap();
        mgr.form = EntryForm {
            label: "New".into(),
            text: "Text".into(),
            key: String::new(),
            priority: 0,
        };
        fs::write(&path, b"external edits").unwrap();
        assert!(mgr.save_form().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external edits");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

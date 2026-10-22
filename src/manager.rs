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
}
impl EntryForm {
    fn from_entry(entry: &Entry) -> Self {
        Self {
            label: entry.label.clone(),
            text: entry.text.clone(),
            key: entry
                .key
                .as_deref()
                .unwrap_or_default()
                .to_ascii_lowercase(),
        }
    }
    fn to_entry(&self) -> Entry {
        Entry {
            label: self.label.trim().to_owned(),
            text: self.text.clone(),
            key: (!self.key.trim().is_empty()).then(|| self.key.trim().to_ascii_uppercase()),
            // Legacy field accepted in existing JSON, no longer ranked.
            priority: 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ManagerAction {
    Stay,
    Back,
    Exit,
}

// Reserve enough width for meaningful labels while leaving the edit form
// comfortable in the default 780px window.
const LIBRARY_WIDTH: f32 = 260.0;
const SIDE_BY_SIDE_MIN_WIDTH: f32 = 715.0;
const LIBRARY_MIN_LIST_HEIGHT: f32 = 300.0;

// The LIBRARY heading and its spacing live above the scrollable entries.
// Keep the list tall enough to be useful even before Hyprland resizes the
// initial picker window into the editor.
fn library_list_height(available_body_height: f32) -> f32 {
    (available_body_height - 32.0).max(LIBRARY_MIN_LIST_HEIGHT)
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

    /// "Done" saves completed edits, rather than silently discarding them.
    /// A failed validation leaves the editor open and preserves the form.
    fn finish(&mut self) -> Result<()> {
        if self.dirty() {
            self.save_form()?;
        }
        Ok(())
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
            .inner_margin(egui::Margin::same(24))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("Trievium");
                    ui.label(egui::RichText::new("Entries").weak());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            action = ManagerAction::Exit;
                        }
                        // Done saves edits before returning to the picker.
                        // Validation errors remain visible without data loss.
                        if ui.button("Done").clicked() {
                            match self.finish() {
                                Ok(()) => action = ManagerAction::Back,
                                Err(error) => self.error = Some(format!("{error:#}")),
                            }
                        }
                    });
                });
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("+ New").clicked() {
                        match self.finish() {
                            Ok(()) => self.select(None),
                            Err(error) => self.error = Some(format!("{error:#}")),
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

                // Measure the finite editor viewport before entering the
                // outer scrolling region. Measuring inside it can instead
                // yield the unbounded content height.
                let list_height = library_list_height(ui.available_height());
                // Both narrow and fully resized layouts must remain usable.
                // A vertical scroll view is critical for the initial 560x175
                // picker surface before Hyprland applies the resize.
                egui::ScrollArea::vertical()
                    .id_salt("manager_body")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut change_selection = None;
                        if ui.available_width() >= SIDE_BY_SIDE_MIN_WIDTH {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(LIBRARY_WIDTH);
                                    self.entry_list(ui, &mut change_selection, list_height);
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
                            self.entry_list(ui, &mut change_selection, list_height);
                            ui.add_space(14.0);
                            ui.separator();
                            ui.add_space(14.0);
                            self.entry_fields(ui);
                        }
                        if let Some(index) = change_selection {
                            match self.finish() {
                                Ok(()) => self.select(Some(index)),
                                Err(error) => self.error = Some(format!("{error:#}")),
                            }
                        }
                    });
            });
        action
    }

    fn entry_list(&mut self, ui: &mut egui::Ui, change: &mut Option<usize>, height: f32) {
        ui.label(egui::RichText::new("LIBRARY").small().strong());
        ui.add_space(8.0);
        // Use the actual editor height; the old 190px cap forced users to
        // scroll through a tiny list despite ample empty space below it.
        egui::ScrollArea::vertical()
            .id_salt("manager_entries")
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, entry) in self.config.entries.iter().enumerate() {
                    let label = format!(
                        "{}    {}",
                        self.map.selection_key(index).to_ascii_lowercase(),
                        entry.label
                    );
                    if ui
                        .selectable_label(self.selected == Some(index), label)
                        .clicked()
                    {
                        *change = Some(index);
                    }
                }
            });
    }

    fn input_frame<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let light = !ui.visuals().dark_mode;
        let fill = if light {
            egui::Color32::from_rgb(246, 248, 252)
        } else {
            egui::Color32::from_rgb(35, 39, 47)
        };
        let border = if light {
            egui::Color32::from_rgb(167, 176, 190)
        } else {
            egui::Color32::from_rgb(96, 107, 122)
        };
        egui::Frame::NONE
            .fill(fill)
            .stroke(egui::Stroke::new(1.0, border))
            .corner_radius(egui::CornerRadius::same(5))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, body)
            .inner
    }

    fn entry_fields(&mut self, ui: &mut egui::Ui) {
        ui.heading(if self.selected.is_some() {
            "Edit entry"
        } else {
            "New entry"
        });
        ui.add_space(14.0);
        ui.label("Name");
        Self::input_frame(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.form.label)
                    .frame(egui::Frame::NONE)
                    .desired_width(ui.available_width().max(180.0)),
            );
        });
        ui.add_space(14.0);
        ui.label("Text to copy");
        Self::input_frame(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.form.text)
                    .frame(egui::Frame::NONE)
                    .desired_rows(6)
                    .desired_width(ui.available_width().max(180.0)),
            );
        });
        ui.add_space(14.0);
        ui.horizontal_wrapped(|ui| {
            ui.vertical(|ui| {
                ui.label("Pinned shortcut");
                Self::input_frame(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.form.key)
                            .frame(egui::Frame::NONE)
                            .desired_width(100.0)
                            .char_limit(4),
                    );
                });
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
    fn library_panel_uses_editor_space_instead_of_old_190px_cap() {
        assert_eq!(LIBRARY_WIDTH, 260.0);
        assert_eq!(library_list_height(220.0), 300.0);
        assert_eq!(library_list_height(480.0), 448.0);
        assert!(SIDE_BY_SIDE_MIN_WIDTH >= LIBRARY_WIDTH + 380.0 + 48.0);
    }

    #[test]
    fn add_edit_delete_and_reload() {
        let (path, cache, config, map) = temp_config();
        let mut mgr = Manager::new(path.clone(), cache, config, map).unwrap();
        mgr.form = EntryForm {
            label: "Address".into(),
            text: "123 Example St".into(),
            key: "a".into(),
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
    fn done_saves_new_entry_and_preserves_lowercase_shortcut_display() {
        let (path, cache, config, map) = temp_config();
        let mut mgr = Manager::new(path.clone(), cache, config, map).unwrap();
        mgr.form = EntryForm {
            label: "Quick reply".into(),
            text: "Thanks!".into(),
            key: "q".into(),
        };
        mgr.finish().unwrap();
        let saved = Config::load(&path).unwrap();
        assert_eq!(saved.entries.len(), 2);
        assert_eq!(saved.entries[1].key.as_deref(), Some("Q"));
        mgr.select(Some(1));
        assert_eq!(mgr.form.key, "q");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn invalid_done_keeps_form_and_previous_config() {
        let (path, cache, config, map) = temp_config();
        let before = std::fs::read(&path).unwrap();
        let mut mgr = Manager::new(path.clone(), cache, config, map).unwrap();
        mgr.form.label = "Incomplete".into();
        assert!(mgr.finish().is_err());
        assert_eq!(mgr.form.label, "Incomplete");
        assert_eq!(std::fs::read(&path).unwrap(), before);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
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
        };
        fs::write(&path, b"external edits").unwrap();
        assert!(mgr.save_form().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external edits");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

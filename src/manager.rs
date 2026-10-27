//! Native CRUD interface for local selections. The picker does not use this
//! code during the shortcut hot path. No importers or network connections.
use crate::colors;
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
    color: Option<String>,
}
impl EntryForm {
    fn from_entry(entry: &Entry) -> Self {
        Self {
            label: entry.label.clone(),
            text: entry.text.clone(),
            color: entry.color.clone(),
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
            color: self.color.clone(),
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
const LIBRARY_MIN_HEIGHT: f32 = 350.0;
const TEXT_EDITOR_MAX_HEIGHT: f32 = 220.0;

// A fixed-size child UI gives the library scroll area a *real* viewport.
// A max_height on its own is only a ceiling and cannot force a short list
// to fill the editor. The old nested outer ScrollArea also shrank this region.
fn library_panel_height(available_body_height: f32) -> f32 {
    available_body_height.max(LIBRARY_MIN_HEIGHT)
}

fn wide_library_layout(available_width: f32) -> bool {
    available_width >= SIDE_BY_SIDE_MIN_WIDTH
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

    /// Reorder saved entries without altering their pinned or remembered codes.
    /// Finish the current edit first so neither text nor color changes are lost.
    fn move_selected(&mut self, offset: isize) -> Result<()> {
        self.finish()?;
        let current = self.selected.context("Select an entry to reorder")?;
        let target = current
            .checked_add_signed(offset)
            .filter(|&index| index < self.config.entries.len())
            .context("Entry is already at the boundary")?;
        let mut next = self.config.clone();
        next.entries.swap(current, target);
        self.commit(next, Some(target))
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
                    let can_up = self.selected.is_some_and(|index| index > 0);
                    let can_down = self
                        .selected
                        .is_some_and(|index| index + 1 < self.config.entries.len());
                    if ui.add_enabled(can_up, egui::Button::new("↑ Up")).clicked()
                        && let Err(error) = self.move_selected(-1)
                    {
                        self.error = Some(format!("{error:#}"));
                    }
                    if ui
                        .add_enabled(can_down, egui::Button::new("↓ Down"))
                        .clicked()
                        && let Err(error) = self.move_selected(1)
                    {
                        self.error = Some(format!("{error:#}"));
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

                // On a normal-width editor, keep the content in a bounded
                // layout: nesting vertical scroll areas caused the library to
                // collapse to the height of just two rows.
                let body_height = library_panel_height(ui.available_height());
                let mut change_selection = None;
                if wide_library_layout(ui.available_width()) {
                    ui.horizontal(|ui| {
                        self.library_panel(ui, &mut change_selection, LIBRARY_WIDTH, body_height);
                        ui.add_space(12.0);
                        ui.separator();
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.set_min_width(380.0);
                            // Keep the form accessible independently of the library
                            // when a small window or long content exceeds its height.
                            egui::ScrollArea::vertical()
                                .id_salt("manager_form")
                                .max_height(body_height)
                                .auto_shrink([false, false])
                                .show(ui, |ui| self.entry_fields(ui));
                        });
                    });
                } else {
                    // Narrow / temporarily unresized windows retain an outer
                    // scroll area so the entry form can always be reached.
                    egui::ScrollArea::vertical()
                        .id_salt("manager_body_narrow")
                        .show(ui, |ui| {
                            let width = ui.available_width().clamp(180.0, LIBRARY_WIDTH);
                            self.library_panel(
                                ui,
                                &mut change_selection,
                                width,
                                LIBRARY_MIN_HEIGHT,
                            );
                            ui.add_space(14.0);
                            ui.separator();
                            ui.add_space(14.0);
                            self.entry_fields(ui);
                        });
                }
                if let Some(index) = change_selection {
                    match self.finish() {
                        Ok(()) => self.select(Some(index)),
                        Err(error) => self.error = Some(format!("{error:#}")),
                    }
                }
            });
        action
    }

    /// Reserve a real full-height UI rectangle before inserting the scroll
    /// area. Returning its viewport rect also lets the layout test catch
    /// regressions where only the column width grows but the list stays tiny.
    fn library_panel(
        &mut self,
        ui: &mut egui::Ui,
        change: &mut Option<usize>,
        width: f32,
        height: f32,
    ) -> egui::Rect {
        ui.allocate_ui_with_layout(
            egui::vec2(width, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_size(egui::vec2(width, height));
                ui.label(egui::RichText::new("LIBRARY").small().strong());
                ui.add_space(8.0);
                // No height cap: the scroll area now fills the explicitly
                // allocated column height, including with just two entries.
                egui::ScrollArea::vertical()
                    .id_salt("manager_entries")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (index, entry) in self.config.entries.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let accent =
                                    colors::for_label(&entry.label, entry.color.as_deref());
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(20.0, 24.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .rect_filled(rect, egui::CornerRadius::same(5), accent);
                                let label = format!(
                                    "{}    {}",
                                    self.map.selection_key(index).to_ascii_lowercase(),
                                    entry.label
                                );
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 30.0],
                                        egui::Button::selectable(
                                            self.selected == Some(index),
                                            label,
                                        ),
                                    )
                                    .clicked()
                                {
                                    *change = Some(index);
                                }
                            });
                        }
                    })
                    .inner_rect
            },
        )
        .inner
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

    // TextEdit grows with its content; desired_rows is only a minimum.
    // Constrain its viewport rather than letting a long snippet push the
    // shortcut field out of view. Scrolling never truncates the stored text.
    fn entry_text_editor(ui: &mut egui::Ui, text: &mut String) -> (egui::Rect, egui::Vec2) {
        let output = egui::ScrollArea::vertical()
            .id_salt("manager_entry_text")
            .max_height(TEXT_EDITOR_MAX_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(text)
                        .frame(egui::Frame::NONE)
                        .desired_rows(6)
                        .desired_width(ui.available_width().max(180.0)),
                );
            });
        (output.inner_rect, output.content_size)
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
            Self::entry_text_editor(ui, &mut self.form.text);
        });
        ui.add_space(14.0);
        ui.horizontal_wrapped(|ui| {
            ui.label("Accent color");
            let selected = colors::for_label(&self.form.label, self.form.color.as_deref());
            let mut rgb = [selected.r(), selected.g(), selected.b()];
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                self.form.color = Some(colors::to_hex(egui::Color32::from_rgb(
                    rgb[0], rgb[1], rgb[2],
                )));
            }
            ui.label(self.form.color.as_deref().unwrap_or("Automatic · OKLCH"));
            if self.form.color.is_some() && ui.button("Auto").clicked() {
                self.form.color = None;
            }
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
                color: None,
            }],
        };
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let map = Keymap::build(&config.entries).unwrap();
        (path, cache, config, map)
    }

    #[test]
    fn library_panel_gets_full_height_with_only_one_entry() {
        assert_eq!(LIBRARY_WIDTH, 260.0);
        assert_eq!(library_panel_height(200.0), LIBRARY_MIN_HEIGHT);
        assert_eq!(library_panel_height(450.0), 450.0);
        assert!(!wide_library_layout(700.0));
        assert!(wide_library_layout(732.0));

        let (path, cache, config, map) = temp_config();
        let mut manager = Manager::new(path.clone(), cache, config, map).unwrap();
        let ctx = egui::Context::default();
        let mut region = egui::Rect::NOTHING;
        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(780.0, 620.0),
                )),
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                region = manager.library_panel(ui, &mut None, 260.0, 410.0);
            })
            .drop_without_applying_deltas();
        }
        assert!(region.width() >= 245.0, "library too narrow: {region:?}");
        assert!(
            region.height() >= 350.0,
            "library scroll area still collapsed: {region:?}"
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn long_text_editor_has_bounded_scrollable_viewport() {
        let ctx = egui::Context::default();
        let mut text = (0..160)
            .map(|line| format!("Line {line}: a full paragraph of copied text."))
            .collect::<Vec<_>>()
            .join("\n");
        let mut viewport = egui::Rect::NOTHING;
        let mut content_size = egui::Vec2::ZERO;

        for _ in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 500.0),
                )),
                ..Default::default()
            };
            ctx.run_ui(input, |ui| {
                ui.set_width(400.0);
                (viewport, content_size) = Manager::entry_text_editor(ui, &mut text);
            })
            .drop_without_applying_deltas();
        }
        assert!(
            viewport.height() <= TEXT_EDITOR_MAX_HEIGHT + 1.0,
            "text editor grew beyond viewport: {viewport:?}"
        );
        assert!(
            viewport.height() > 80.0,
            "editor viewport collapsed: {viewport:?}"
        );
        assert!(
            content_size.y > viewport.height() * 2.0,
            "long text was not made scrollable: content={content_size:?}, viewport={viewport:?}"
        );
        assert_eq!(
            text.lines().count(),
            160,
            "long text was modified by layout"
        );
    }

    #[test]
    fn add_edit_delete_and_reload() {
        let (path, cache, config, map) = temp_config();
        let mut mgr = Manager::new(path.clone(), cache, config, map).unwrap();
        mgr.form = EntryForm {
            label: "Address".into(),
            text: "123 Example St".into(),
            key: "a".into(),
            color: None,
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
            color: None,
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
            color: None,
        };
        assert!(mgr.save_form().is_err());
        assert_eq!(fs::read(&path).unwrap(), initial);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn color_override_and_reorder_round_trip_preserve_content_and_shortcuts() {
        let (path, cache, config, map) = temp_config();
        let mut mgr = Manager::new(path.clone(), cache, config, map).unwrap();
        mgr.form = EntryForm {
            label: "Another".into(),
            text: "🐦 A long text survives reordering".into(),
            key: "a".into(),
            color: Some("#AA44CC".into()),
        };
        mgr.save_form().unwrap();
        assert_eq!(mgr.selected, Some(1));
        let pinned = mgr.map.key(1).to_string();
        mgr.move_selected(-1).unwrap();
        let saved = Config::load(&path).unwrap();
        assert_eq!(saved.entries[0].label, "Another");
        assert_eq!(saved.entries[0].color.as_deref(), Some("#AA44CC"));
        assert_eq!(saved.entries[0].text, "🐦 A long text survives reordering");
        assert_eq!(mgr.map.key(0), pinned);
        assert_eq!(mgr.selected, Some(0));
        assert!(mgr.move_selected(-1).is_err());
        mgr.move_selected(1).unwrap();
        assert_eq!(Config::load(&path).unwrap().entries[1].label, "Another");
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
            color: None,
        };
        fs::write(&path, b"external edits").unwrap();
        assert!(mgr.save_form().is_err());
        assert_eq!(fs::read(&path).unwrap(), b"external edits");
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

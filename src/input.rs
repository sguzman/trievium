//! Shortcut text reducer. A focused native egui TextEdit handles
//! keyboard focus, editing, Backspace, and event delivery.
use crate::trie::{Keymap, Lookup};

#[derive(Debug, PartialEq, Eq)]
pub enum TypedOutcome {
    Continue(String),
    Selected(usize),
}

pub fn resolve_typed(buffer: &str, map: &Keymap) -> TypedOutcome {
    let mut prefix = String::new();
    for ch in buffer.chars() {
        if !ch.is_ascii_alphabetic() {
            continue;
        }
        prefix.push(ch.to_ascii_uppercase());
        match map.lookup(&prefix) {
            Lookup::Selected(index) => return TypedOutcome::Selected(index),
            Lookup::Continue => {}
            Lookup::Invalid => {
                prefix.pop();
            }
        }
    }
    TypedOutcome::Continue(prefix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Entry;

    fn entry(label: &str, key: &str) -> Entry {
        Entry {
            label: label.into(),
            text: label.into(),
            key: Some(key.into()),
            priority: 0,
            color: None,
        }
    }

    #[test]
    fn selects_terminal_without_enter() {
        let map = Keymap::build(&[entry("Greeting", "G"), entry("Closing", "C")]).unwrap();
        assert_eq!(resolve_typed("g", &map), TypedOutcome::Selected(0));
        assert_eq!(resolve_typed("C", &map), TypedOutcome::Selected(1));
        assert_eq!(
            resolve_typed("f", &map),
            TypedOutcome::Continue(String::new())
        );
        assert_eq!(resolve_typed("fc", &map), TypedOutcome::Selected(1));
    }

    #[test]
    fn one_unique_match_finishes_on_first_letter() {
        let map = Keymap::build(&[entry("Greeting", "GR"), entry("Closing", "C")]).unwrap();
        assert_eq!(resolve_typed("g", &map), TypedOutcome::Selected(0));
        assert_eq!(resolve_typed("xg", &map), TypedOutcome::Selected(0));
        assert_eq!(
            resolve_typed("x", &map),
            TypedOutcome::Continue(String::new())
        );
    }

    #[test]
    fn invalid_letters_do_not_register_in_ambiguous_branch() {
        let map = Keymap::build(&[entry("Brief", "BA"), entry("Checklist", "BC")]).unwrap();
        assert_eq!(resolve_typed("b", &map), TypedOutcome::Continue("B".into()));
        assert_eq!(
            resolve_typed("bx", &map),
            TypedOutcome::Continue("B".into())
        );
        assert_eq!(resolve_typed("bxc", &map), TypedOutcome::Selected(1));
    }

    #[test]
    fn multi_key_prefixes_and_backspace_work() {
        let map = Keymap::build(&[entry("Brief", "BA"), entry("Checklist", "BC")]).unwrap();
        assert_eq!(resolve_typed("b", &map), TypedOutcome::Continue("B".into()));
        assert_eq!(
            resolve_typed("bz", &map),
            TypedOutcome::Continue("B".into())
        );
        assert_eq!(resolve_typed("bc", &map), TypedOutcome::Selected(1));
        assert_eq!(
            resolve_typed("", &map),
            TypedOutcome::Continue(String::new())
        );
        assert_eq!(resolve_typed("ba", &map), TypedOutcome::Selected(0));
    }

    #[test]
    fn first_complete_terminal_wins_in_batched_input() {
        let map = Keymap::build(&[entry("Greeting", "G"), entry("Closing", "C")]).unwrap();
        assert_eq!(resolve_typed("gc", &map), TypedOutcome::Selected(0));
        assert_eq!(
            resolve_typed("🌎é!", &map),
            TypedOutcome::Continue(String::new())
        );
    }

    #[test]
    fn native_egui_focused_text_edit_consumes_keyboard_input() {
        let ctx = eframe::egui::Context::default();
        let mut input = String::new();
        let mut first = ctx.run_ui(Default::default(), |ctx| {
            eframe::egui::CentralPanel::default().show(ctx, |ui| {
                ui.add(eframe::egui::TextEdit::singleline(&mut input))
                    .request_focus();
            });
        });
        first.textures_delta.clear();
        let mut second = ctx.run_ui(
            eframe::egui::RawInput {
                events: vec![eframe::egui::Event::Text("G".into())],
                ..Default::default()
            },
            |ctx| {
                eframe::egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add(eframe::egui::TextEdit::singleline(&mut input));
                });
            },
        );
        second.textures_delta.clear();
        assert_eq!(input, "G");
    }
}

//! Keyboard-only input handling, isolated from native-window side effects.
//!
//! Selection accepts logical A-Z key presses, not arbitrary text insertion.
//! This deliberately ignores key-repeat and modified hotkeys. Shift is fine.
use crate::trie::{Keymap, Lookup};
use eframe::egui::{Event, Key};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputAction {
    Letter(char),
    Backspace,
    Close,
    Ignore,
}

pub fn classify_key(event: &Event) -> InputAction {
    let Event::Key {
        key,
        pressed: true,
        repeat: false,
        modifiers,
        ..
    } = event
    else {
        return InputAction::Ignore;
    };
    if modifiers.command || modifiers.ctrl || modifiers.alt || modifiers.mac_cmd {
        return InputAction::Ignore;
    }
    match key {
        Key::Escape => InputAction::Close,
        Key::Backspace => InputAction::Backspace,
        Key::A => InputAction::Letter('A'),
        Key::B => InputAction::Letter('B'),
        Key::C => InputAction::Letter('C'),
        Key::D => InputAction::Letter('D'),
        Key::E => InputAction::Letter('E'),
        Key::F => InputAction::Letter('F'),
        Key::G => InputAction::Letter('G'),
        Key::H => InputAction::Letter('H'),
        Key::I => InputAction::Letter('I'),
        Key::J => InputAction::Letter('J'),
        Key::K => InputAction::Letter('K'),
        Key::L => InputAction::Letter('L'),
        Key::M => InputAction::Letter('M'),
        Key::N => InputAction::Letter('N'),
        Key::O => InputAction::Letter('O'),
        Key::P => InputAction::Letter('P'),
        Key::Q => InputAction::Letter('Q'),
        Key::R => InputAction::Letter('R'),
        Key::S => InputAction::Letter('S'),
        Key::T => InputAction::Letter('T'),
        Key::U => InputAction::Letter('U'),
        Key::V => InputAction::Letter('V'),
        Key::W => InputAction::Letter('W'),
        Key::X => InputAction::Letter('X'),
        Key::Y => InputAction::Letter('Y'),
        Key::Z => InputAction::Letter('Z'),
        _ => InputAction::Ignore,
    }
}


/// Process key-down events if present; if a platform only reports text
/// events, use single-letter text as a fallback. Never synthesize text from
/// pasted or multi-character values. The caller remembers whether the native
/// key event path has ever worked, preventing a duplicate Text from being
/// consumed in a subsequent frame.
pub fn actions(events: &[Event], key_events_observed: &mut bool) -> Vec<InputAction> {
    if events.iter().any(|event| matches!(event, Event::Key { pressed: true, .. })) {
        *key_events_observed = true;
    }
    if *key_events_observed {
        events
            .iter()
            .map(classify_key)
            .filter(|action| *action != InputAction::Ignore)
            .collect()
    } else {
        events
            .iter()
            .filter_map(|event| match event {
                Event::Text(text) if text.len() == 1 => {
                    let letter = text.as_bytes()[0];
                    letter.is_ascii_alphabetic().then_some(InputAction::Letter(
                        char::from(letter.to_ascii_uppercase()),
                    ))
                }
                _ => None,
            })
            .collect()
    }
}

#[derive(Default)]
pub struct Selection {
    prefix: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionOutcome {
    Continue,
    Selected(usize),
    Invalid,
}

impl Selection {
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn clear(&mut self) {
        self.prefix.clear();
    }

    pub fn backspace(&mut self) {
        self.prefix.pop();
    }

    pub fn push(&mut self, ch: char, map: &Keymap) -> SelectionOutcome {
        if !ch.is_ascii_alphabetic() {
            return SelectionOutcome::Invalid;
        }
        let letter = ch.to_ascii_uppercase();
        self.prefix.push(letter);
        match map.lookup(&self.prefix) {
            Lookup::Selected(index) => SelectionOutcome::Selected(index),
            Lookup::Continue => SelectionOutcome::Continue,
            Lookup::Invalid => {
                self.prefix.pop();
                SelectionOutcome::Invalid
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Entry;
    use eframe::egui::Modifiers;

    fn event(key: Key, modifiers: Modifiers, repeat: bool) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers,
        }
    }

    fn pinned(label: &str, shortcut: &str) -> Entry {
        Entry {
            label: label.into(),
            text: label.into(),
            key: Some(shortcut.into()),
            priority: 0,
        }
    }

    #[test]
    fn unmodified_letter_triggers_and_repeat_does_not() {
        assert_eq!(
            classify_key(&event(Key::A, Modifiers::NONE, false)),
            InputAction::Letter('A')
        );
        assert_eq!(
            classify_key(&event(Key::A, Modifiers::NONE, true)),
            InputAction::Ignore
        );
    }

    #[test]
    fn modifier_hotkeys_cannot_select_items() {
        for modifiers in [Modifiers::CTRL, Modifiers::ALT, Modifiers::COMMAND] {
            assert_eq!(
                classify_key(&event(Key::S, modifiers, false)),
                InputAction::Ignore
            );
        }
        assert_eq!(
            classify_key(&event(Key::A, Modifiers::SHIFT, false)),
            InputAction::Letter('A')
        );
    }

    #[test]
    fn text_only_backend_can_select_a_letter() {
        let mut observed = false;
        assert_eq!(
            actions(&[Event::Text("g".into())], &mut observed),
            vec![InputAction::Letter('G')]
        );
        assert!(!observed);
        assert!(actions(&[Event::Text("paste".into())], &mut observed).is_empty());
        assert!(actions(&[Event::Text("é".into())], &mut observed).is_empty());
    }

    #[test]
    fn native_key_events_prevent_duplicate_text_selection() {
        let mut observed = false;
        let first = vec![
            event(Key::A, Modifiers::NONE, false),
            Event::Text("a".into()),
        ];
        assert_eq!(actions(&first, &mut observed), vec![InputAction::Letter('A')]);
        assert!(observed);
        assert!(actions(&[Event::Text("a".into())], &mut observed).is_empty());
        let combo = vec![
            event(Key::G, Modifiers::CTRL, false),
            Event::Text("g".into()),
        ];
        assert!(actions(&combo, &mut observed).is_empty());
    }

    #[test]
    fn special_keys_and_text_input() {
        assert_eq!(
            classify_key(&event(Key::Escape, Modifiers::NONE, false)),
            InputAction::Close
        );
        assert_eq!(
            classify_key(&event(Key::Backspace, Modifiers::NONE, false)),
            InputAction::Backspace
        );
        assert_eq!(classify_key(&Event::Text("E".into())), InputAction::Ignore);
    }

    #[test]
    fn invalid_letter_preserves_valid_prefix() {
        let map = Keymap::build(&[pinned("Brief", "BA"), pinned("Checklist", "BC")]).unwrap();
        let mut picker = Selection::default();
        assert_eq!(picker.push('b', &map), SelectionOutcome::Continue);
        assert_eq!(picker.push('z', &map), SelectionOutcome::Invalid);
        assert_eq!(picker.prefix(), "B");
        assert_eq!(picker.push('c', &map), SelectionOutcome::Selected(1));
        picker.backspace();
        assert_eq!(picker.prefix(), "B");
        picker.backspace();
        assert_eq!(picker.prefix(), "");
    }

    #[test]
    fn clear_resets_after_clipboard_error() {
        let map = Keymap::build(&[pinned("Email", "E")]).unwrap();
        let mut selection = Selection::default();
        assert_eq!(selection.push('e', &map), SelectionOutcome::Selected(0));
        selection.clear();
        assert_eq!(selection.prefix(), "");
        assert_eq!(selection.push('E', &map), SelectionOutcome::Selected(0));
    }
}

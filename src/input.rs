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

fn letter_for_key(key: Key) -> Option<char> {
    match key {
        Key::A => Some('A'), Key::B => Some('B'), Key::C => Some('C'),
        Key::D => Some('D'), Key::E => Some('E'), Key::F => Some('F'),
        Key::G => Some('G'), Key::H => Some('H'), Key::I => Some('I'),
        Key::J => Some('J'), Key::K => Some('K'), Key::L => Some('L'),
        Key::M => Some('M'), Key::N => Some('N'), Key::O => Some('O'),
        Key::P => Some('P'), Key::Q => Some('Q'), Key::R => Some('R'),
        Key::S => Some('S'), Key::T => Some('T'), Key::U => Some('U'),
        Key::V => Some('V'), Key::W => Some('W'), Key::X => Some('X'),
        Key::Y => Some('Y'), Key::Z => Some('Z'), _ => None,
    }
}
fn modified(m: eframe::egui::Modifiers) -> bool {
    m.command || m.ctrl || m.alt || m.mac_cmd
}
pub fn classify_key(event: &Event) -> InputAction {
    let Event::Key { key, pressed: true, repeat: false, modifiers, .. } = event
    else { return InputAction::Ignore; };
    if modified(*modifiers) { return InputAction::Ignore; }
    match key {
        Key::Escape => InputAction::Close,
        Key::Backspace => InputAction::Backspace,
        _ => letter_for_key(*key).map_or(InputAction::Ignore, InputAction::Letter),
    }
}

/// Prefer native key-downs, without permanently disabling the text fallback.
/// Deduplicate Text events from the same physical press across adjacent frames.
#[derive(Default)]
pub struct KeyboardInput {
    recent_native: Vec<(char, std::time::Instant)>,
}
impl KeyboardInput {
    pub fn actions(
        &mut self,
        events: &[Event],
        modifiers: eframe::egui::Modifiers,
        now: std::time::Instant,
    ) -> Vec<InputAction> {
        use std::time::Duration;
        self.recent_native.retain(|(_, at)| now.saturating_duration_since(*at) < Duration::from_millis(180));
        let mut output = Vec::new();
        for event in events {
            if let Event::Key { key, pressed: true, .. } = event {
                if let Some(letter) = letter_for_key(*key) {
                    self.recent_native.push((letter, now));
                }
                let action = classify_key(event);
                if action != InputAction::Ignore { output.push(action); }
            }
        }
        if !modified(modifiers) {
            for event in events {
                if let Event::Text(text) = event
                    && text.len() == 1
                    && text.as_bytes()[0].is_ascii_alphabetic()
                {
                    let letter = char::from(text.as_bytes()[0].to_ascii_uppercase());
                    if !self.recent_native.iter().any(|(ch, _)| *ch == letter) {
                        output.push(InputAction::Letter(letter));
                    }
                }
            }
        }
        output
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
    fn text_only_event_selects_without_native_key() {
        let mut k = KeyboardInput::default();
        let now = std::time::Instant::now();
        assert_eq!(k.actions(&[Event::Text("g".into())], Modifiers::NONE, now), vec![InputAction::Letter('G')]);
        assert!(k.actions(&[Event::Text("paste".into())], Modifiers::NONE, now).is_empty());
        assert!(k.actions(&[Event::Text("é".into())], Modifiers::NONE, now).is_empty());
    }
    #[test]
    fn native_key_deduplicates_text_across_frames() {
        let mut k = KeyboardInput::default();
        let now = std::time::Instant::now();
        let both = vec![event(Key::A, Modifiers::NONE, false), Event::Text("a".into())];
        assert_eq!(k.actions(&both, Modifiers::NONE, now), vec![InputAction::Letter('A')]);
        assert!(k.actions(&[Event::Text("a".into())], Modifiers::NONE, now + std::time::Duration::from_millis(40)).is_empty());
    }
    #[test]
    fn unrelated_native_key_does_not_disable_text_fallback() {
        let mut k = KeyboardInput::default();
        let now = std::time::Instant::now();
        assert_eq!(k.actions(&[event(Key::Escape, Modifiers::NONE, false)], Modifiers::NONE, now), vec![InputAction::Close]);
        assert_eq!(k.actions(&[Event::Text("g".into())], Modifiers::NONE, now), vec![InputAction::Letter('G')]);
    }
    #[test]
    fn dedup_window_expires_for_text_only_keys() {
        let mut k = KeyboardInput::default();
        let now = std::time::Instant::now();
        assert_eq!(k.actions(&[event(Key::G, Modifiers::NONE, false)], Modifiers::NONE, now), vec![InputAction::Letter('G')]);
        assert_eq!(k.actions(&[Event::Text("g".into())], Modifiers::NONE, now + std::time::Duration::from_millis(200)), vec![InputAction::Letter('G')]);
    }
    #[test]
    fn modified_key_cannot_leak_via_text_fallback() {
        let mut k = KeyboardInput::default();
        let now = std::time::Instant::now();
        let both = vec![event(Key::G, Modifiers::CTRL, false), Event::Text("g".into())];
        assert!(k.actions(&both, Modifiers::CTRL, now).is_empty());
        assert!(k.actions(&[Event::Text("g".into())], Modifiers::NONE, now + std::time::Duration::from_millis(40)).is_empty());
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

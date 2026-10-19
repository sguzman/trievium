//! Prefix-free key assignment and lookup. A terminal key is never the prefix
//! of another terminal key. Explicit keys are never reassigned.
use crate::config::Entry;
use anyhow::{Result, bail};
use std::collections::BTreeMap;

const ALPHABET: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const MAX_DEPTH: usize = 4;

#[derive(Default, Debug)]
struct Node {
    terminal: Option<usize>,
    children: BTreeMap<char, Node>,
}

impl Node {
    fn conflicts(&self, code: &str) -> bool {
        let mut current = self;
        for ch in code.chars() {
            if current.terminal.is_some() {
                return true;
            }
            match current.children.get(&ch) {
                Some(next) => current = next,
                None => return false,
            }
        }
        current.terminal.is_some() || !current.children.is_empty()
    }

    fn insert(&mut self, code: &str, index: usize) -> Result<()> {
        if self.conflicts(code) {
            bail!("Key '{code}' duplicates or prefixes another key");
        }
        let mut node = self;
        for c in code.chars() {
            node = node.children.entry(c).or_default();
        }
        node.terminal = Some(index);
        Ok(())
    }

    fn get(&self, prefix: &str) -> Option<&Node> {
        let mut node = self;
        for c in prefix.chars() {
            node = node.children.get(&c)?;
        }
        Some(node)
    }
}

#[derive(Debug)]
pub struct Keymap {
    root: Node,
    keys: Vec<String>,
}

impl Keymap {
    pub fn build(entries: &[Entry]) -> Result<Self> {
        if entries.is_empty() {
            bail!("Cannot build an empty keymap");
        }
        let mut root = Node::default();
        let mut keys = vec![String::new(); entries.len()];
        let mut pinned = Vec::new();
        let mut auto = Vec::new();

        for (i, e) in entries.iter().enumerate() {
            if let Some(key) = &e.key {
                let code = key.trim().to_ascii_uppercase();
                if code.is_empty()
                    || code.len() > MAX_DEPTH
                    || !code.bytes().all(|c| c.is_ascii_uppercase())
                {
                    bail!("Invalid key for '{}': use 1-4 ASCII letters A-Z", e.label);
                }
                root.insert(&code, i)?;
                keys[i] = code.clone();
                pinned.push(code);
            } else {
                auto.push(i);
            }
        }

        auto.sort_by(|&a, &b| {
            entries[b]
                .priority
                .cmp(&entries[a].priority)
                .then_with(|| entries[a].label.cmp(&entries[b].label))
                .then_with(|| a.cmp(&b))
        });

        // Each terminal at depth d consumes 26^(L-d) slots at depth L.
        // Preserve enough free slots for every remaining entry before taking
        // a shorter mnemonic. This avoids the greedy 26-single-keys dead end.
        let min_depth = pinned.iter().map(String::len).max().unwrap_or(1);
        let depth = (min_depth..=MAX_DEPTH)
            .find(|&level| {
                let used: usize = pinned.iter().map(|c| capacity(level - c.len())).sum();
                capacity(level).saturating_sub(used) >= auto.len()
            })
            .ok_or_else(|| {
                anyhow::anyhow!("Too many entries for keys of up to {MAX_DEPTH} letters")
            })?;

        let used: usize = pinned.iter().map(|c| capacity(depth - c.len())).sum();
        let mut free = capacity(depth) - used;
        for (position, &entry_index) in auto.iter().enumerate() {
            let remaining_after = auto.len() - position - 1;
            let order = alphabet_order(&entries[entry_index].label);
            let mut chosen = None;
            for len in 1..=depth {
                let mut candidate = String::with_capacity(len);
                if find_candidate(
                    &root,
                    &order,
                    len,
                    &mut candidate,
                    free,
                    remaining_after,
                    depth,
                    &mut chosen,
                ) {
                    break;
                }
            }
            let code = chosen.ok_or_else(|| {
                anyhow::anyhow!("Cannot assign a key to '{}'", entries[entry_index].label)
            })?;
            free -= capacity(depth - code.len());
            root.insert(&code, entry_index)?;
            keys[entry_index] = code;
        }
        Ok(Self { root, keys })
    }

    pub fn key(&self, index: usize) -> &str {
        &self.keys[index]
    }

    pub fn lookup(&self, prefix: &str) -> Lookup {
        match self.root.get(prefix) {
            None => Lookup::Invalid,
            Some(node) => match node.terminal {
                Some(index) => Lookup::Selected(index),
                None => Lookup::Continue,
            },
        }
    }

    pub fn matching<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = usize> + 'a {
        self.keys
            .iter()
            .enumerate()
            .filter(move |(_, code)| code.starts_with(prefix))
            .map(|(index, _)| index)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup {
    Invalid,
    Continue,
    Selected(usize),
}

fn capacity(exponent: usize) -> usize {
    26usize.pow(exponent as u32)
}

fn alphabet_order(label: &str) -> Vec<char> {
    let mut result = Vec::with_capacity(26);
    for c in label
        .chars()
        .flat_map(char::to_uppercase)
        .chain(ALPHABET.chars())
    {
        if c.is_ascii_uppercase() && !result.contains(&c) {
            result.push(c);
        }
    }
    result
}

fn find_candidate(
    root: &Node,
    order: &[char],
    length: usize,
    candidate: &mut String,
    free: usize,
    remaining_after: usize,
    max_depth: usize,
    chosen: &mut Option<String>,
) -> bool {
    if candidate.len() == length {
        let cost = capacity(max_depth - length);
        if cost <= free && free - cost >= remaining_after && !root.conflicts(candidate) {
            *chosen = Some(candidate.clone());
            return true;
        }
        return false;
    }
    for &c in order {
        candidate.push(c);
        if find_candidate(
            root,
            order,
            length,
            candidate,
            free,
            remaining_after,
            max_depth,
            chosen,
        ) {
            return true;
        }
        candidate.pop();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(label: &str, key: Option<&str>, priority: i32) -> Entry {
        Entry {
            label: label.into(),
            text: "some text".into(),
            key: key.map(str::to_owned),
            priority,
        }
    }

    #[test]
    fn mnemonic_single_keys_and_selection() {
        let items = vec![
            entry("Email", None, 9),
            entry("Address", None, 5),
            entry("Signature", None, 1),
        ];
        let map = Keymap::build(&items).unwrap();
        assert_eq!(map.key(0), "E");
        assert_eq!(map.key(1), "A");
        assert_eq!(map.key(2), "S");
        assert_eq!(map.lookup("A"), Lookup::Selected(1));
        assert_eq!(map.lookup("Z"), Lookup::Invalid);
    }

    #[test]
    fn pinned_keys_are_never_stolen() {
        let items = vec![entry("Email", Some("e"), 0), entry("Elephant", None, 900)];
        let map = Keymap::build(&items).unwrap();
        assert_eq!(map.key(0), "E");
        assert_ne!(map.key(1), "E");
    }

    #[test]
    fn rejects_prefix_collisions() {
        let items = vec![entry("A", Some("T"), 0), entry("B", Some("TA"), 0)];
        assert!(Keymap::build(&items).is_err());
    }

    #[test]
    fn grows_beyond_single_character_capacity() {
        let items: Vec<_> = (0..60)
            .map(|i| entry(&format!("Entry {i}"), None, 0))
            .collect();
        let map = Keymap::build(&items).unwrap();
        assert_eq!(map.keys.len(), 60);
        for (i, code) in map.keys.iter().enumerate() {
            assert_eq!(map.lookup(code), Lookup::Selected(i));
            for (j, other) in map.keys.iter().enumerate() {
                if i != j {
                    assert!(!other.starts_with(code), "{code} prefixes {other}");
                }
            }
        }
        assert!(map.keys.iter().any(|s| s.len() > 1));
    }

    #[test]
    fn handles_pinned_nested_alphabet_and_available_space() {
        let items = vec![
            entry("A", Some("AB"), 0),
            entry("B", Some("Z"), 0),
            entry("C", None, 99),
            entry("D", None, 1),
        ];
        let map = Keymap::build(&items).unwrap();
        assert_eq!(map.lookup("A"), Lookup::Continue);
        assert_eq!(map.lookup("AB"), Lookup::Selected(0));
        assert_eq!(map.lookup("Z"), Lookup::Selected(1));
        assert_eq!(map.lookup(map.key(2)), Lookup::Selected(2));
    }

    #[test]
    fn explicit_key_validation() {
        let items = vec![entry("Invalid", Some("1x"), 0)];
        assert!(Keymap::build(&items).is_err());
    }
}

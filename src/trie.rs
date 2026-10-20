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
    // Occupied capacity at the maximum shortcut depth. This lets the
    // candidate search skip entire saturated subtrees in constant time.
    used_slots: usize,
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
        let cost = capacity(MAX_DEPTH - code.len());
        self.used_slots += cost;
        let mut node = self;
        for c in code.chars() {
            node = node.children.entry(c).or_default();
            node.used_slots += cost;
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
        Self::build_with_previous(entries, &BTreeMap::new())
    }

    /// Reuse locally remembered keys whenever they remain compatible.
    /// A pinned key always overrides an automatically remembered one.
    pub fn build_with_previous(
        entries: &[Entry],
        previous: &BTreeMap<String, String>,
    ) -> Result<Self> {
        if entries.is_empty() {
            return Ok(Self {
                root: Node::default(),
                keys: Vec::new(),
            });
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
        let remembered_depth = entries
            .iter()
            .filter(|entry| entry.key.is_none())
            .filter_map(|entry| previous.get(&entry.label))
            .filter(|key| valid_key(key))
            .map(String::len)
            .max()
            .unwrap_or(1);
        let min_depth = pinned
            .iter()
            .map(String::len)
            .max()
            .unwrap_or(1)
            .max(remembered_depth);
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
        // Keep existing mnemonic assignments unless they conflict with pinned
        // keys or would exhaust all available space for the remaining entries.
        // If every letter has already been used, the least-prioritized
        // remembered shortcut yields to longer sequences.
        let mut unresolved = 0_usize;
        for (position, &index) in auto.iter().enumerate() {
            let Some(code) = previous.get(&entries[index].label) else {
                unresolved += 1;
                continue;
            };
            if !valid_key(code) || code.len() > depth || root.conflicts(code) {
                unresolved += 1;
                continue;
            }
            let cost = capacity(depth - code.len());
            // Include unassigned, higher-priority entries visited earlier as
            // well as everything still to come. Otherwise a full remembered
            // alphabet could consume the last free branch before a new entry
            // can receive a shortcut.
            let remaining = unresolved + (auto.len() - position - 1);
            if cost > free || free - cost < remaining {
                unresolved += 1;
                continue;
            }
            root.insert(code, index)?;
            keys[index] = code.clone();
            free -= cost;
        }

        let remaining_entries: Vec<usize> = auto
            .iter()
            .copied()
            .filter(|index| keys[*index].is_empty())
            .collect();
        for (position, &entry_index) in remaining_entries.iter().enumerate() {
            let remaining_after = remaining_entries.len() - position - 1;
            let order = alphabet_order(&entries[entry_index].label);
            let mut chosen = None;
            for len in 1..=depth {
                let mut candidate = String::with_capacity(len);
                let search = CandidateSearch {
                    root: &root,
                    order: &order,
                    length: len,
                    free,
                    remaining_after,
                    max_depth: depth,
                };
                if find_candidate(&search, &mut candidate, &mut chosen) {
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

fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= MAX_DEPTH && key.bytes().all(|byte| byte.is_ascii_uppercase())
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

struct CandidateSearch<'a> {
    root: &'a Node,
    order: &'a [char],
    length: usize,
    free: usize,
    remaining_after: usize,
    max_depth: usize,
}

fn find_candidate(
    search: &CandidateSearch<'_>,
    candidate: &mut String,
    chosen: &mut Option<String>,
) -> bool {
    // Skip not only terminal prefixes but whole subtrees already saturated
    // by shorter keys or a complete set of longer terminal keys.
    if !candidate.is_empty()
        && search
            .root
            .get(candidate)
            .is_some_and(|node| node.used_slots == capacity(MAX_DEPTH - candidate.len()))
    {
        return false;
    }
    if candidate.len() == search.length {
        let cost = capacity(search.max_depth - search.length);
        if cost <= search.free
            && search.free - cost >= search.remaining_after
            && !search.root.conflicts(candidate)
        {
            *chosen = Some(candidate.clone());
            return true;
        }
        return false;
    }
    for &c in search.order {
        candidate.push(c);
        if find_candidate(search, candidate, chosen) {
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
    fn allows_zero_entries_and_selects_nothing() {
        let map = Keymap::build(&[]).unwrap();
        assert_eq!(map.matching("").count(), 0);
        assert_eq!(map.lookup("A"), Lookup::Invalid);
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
    fn remembers_shortcuts_after_adding_entries() {
        let initial = vec![entry("Email", None, 9), entry("Address", None, 4)];
        let previous_map = Keymap::build(&initial).unwrap();
        let previous = BTreeMap::from([
            ("Email".into(), previous_map.key(0).into()),
            ("Address".into(), previous_map.key(1).into()),
        ]);
        let expanded = vec![
            entry("Email", None, 9),
            entry("Address", None, 4),
            entry("Express", None, 100),
        ];
        let map = Keymap::build_with_previous(&expanded, &previous).unwrap();
        assert_eq!(map.key(0), previous_map.key(0));
        assert_eq!(map.key(1), previous_map.key(1));
        assert_eq!(map.lookup(map.key(2)), Lookup::Selected(2));
    }

    #[test]
    fn pinned_key_overrides_remembered_key() {
        let entries = vec![entry("Email", None, 1), entry("Address", Some("E"), 1)];
        let previous = BTreeMap::from([("Email".into(), "E".into())]);
        let map = Keymap::build_with_previous(&entries, &previous).unwrap();
        assert_eq!(map.key(1), "E");
        assert_ne!(map.key(0), "E");
    }

    #[test]
    fn recovers_when_all_single_letters_were_remembered() {
        let initial: Vec<_> = ('A'..='Z')
            .map(|c| entry(&c.to_string(), Some(&c.to_string()), 0))
            .collect();
        // A cache can contain 26 old automatic bindings. Adding an item
        // requires one of those bindings to grow; no terminal can prefix it.
        let previous: BTreeMap<_, _> = initial
            .iter()
            .map(|e| (e.label.clone(), e.key.clone().unwrap()))
            .collect();
        let mut expanded: Vec<_> = ('A'..='Z')
            .map(|c| entry(&c.to_string(), None, 0))
            .collect();
        expanded.push(entry("New", None, 99));
        let map = Keymap::build_with_previous(&expanded, &previous).unwrap();
        assert!(map.key(26).len() > 1);
        for (index, code) in (0..expanded.len()).map(|i| (i, map.key(i))) {
            assert_eq!(map.lookup(code), Lookup::Selected(index));
        }
    }

    #[test]
    fn ignores_malformed_cached_keys() {
        let entries = vec![entry("Email", None, 0)];
        let previous = BTreeMap::from([("Email".into(), "E!BAD".into())]);
        let map = Keymap::build_with_previous(&entries, &previous).unwrap();
        assert_eq!(map.key(0), "E");
    }

    #[test]
    fn occupancy_prunes_full_subtrees_without_losing_available_keys() {
        let mut root = Node::default();
        for c in 'A'..='Z' {
            root.insert(&format!("A{c}"), (c as u32 - 'A' as u32) as usize)
                .unwrap();
        }
        assert_eq!(root.get("A").unwrap().used_slots, capacity(MAX_DEPTH - 1));
        assert_eq!(root.used_slots, capacity(MAX_DEPTH - 1));
        assert!(!root.conflicts("B"));
        assert!(root.conflicts("AX"));
    }

    #[test]
    fn scales_to_large_libraries() {
        let items: Vec<_> = (0..1000)
            .map(|i| entry(&format!("Entry {i:04}"), None, 0))
            .collect();
        let map = Keymap::build(&items).unwrap();
        for (index, key) in (0..items.len()).map(|i| (i, map.key(i))) {
            assert_eq!(map.lookup(key), Lookup::Selected(index));
        }
    }

    #[test]
    fn explicit_key_validation() {
        let items = vec![entry("Invalid", Some("1x"), 0)];
        assert!(Keymap::build(&items).is_err());
    }
}

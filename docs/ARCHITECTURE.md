# Architecture and decisions

## Purpose and boundaries

Trievium is a single-process, on-demand selection UI: **keybind -> prefix-tree lookup -> clipboard -> exit**. It is not a text expander, keylogger, clipboard manager, or background daemon. It never synthesizes keys into the previously focused app. The selection/action boundary allows future non-text payloads without coupling the key algorithm to any destination.

## Modules

- `config.rs`: strict versioned JSON input; filesystem discovery; non-destructive `init`.
- `trie.rs`: deterministic, prefix-free shortcut generation and lookup; no UI dependencies.
- `shortcut_cache.rs`: per-configuration persistent mnemonic assignments, storing labels and keys only.
- `clipboard.rs`: uses `wl-copy` via direct process arguments and piped input (no shell or quoting).
- `main.rs`: small CLI and focused egui picker.

Config is read once at launch, without a watcher, daemon, or network calls. For now, changes take effect on the next invocation.

## Key assignment and invariants

1. Normalize pinned keys to uppercase; allow A-Z only, max four characters; reject duplicate and ancestor/descendant collisions.
2. Sort unpinned entries by descending `priority`, then label, then original position. Reserve old assignments from the local shortcut cache when they remain compatible with pinned shortcuts and leave enough space for the rest.
3. Find the smallest level L (max 4) with enough 26-ary leaf capacity after accounting for pinned terminals. A terminal of length d uses 26^(L-d) potential leaves.
4. At each priority step try short candidates first, mnemonic characters before alphabetical fallback. Only take a candidate if it does not collide with the trie **and** leaves enough leaf capacity for every remaining entry.
5. Insert terminal node; no terminal node can have children. Each trie node tracks occupied max-depth leaf slots so a saturated subtree is skipped in constant time during allocation rather than enumerating all of its leaves. A keypress at a terminal executes immediately. Nonterminal keypresses narrow the choices. Unknown sequences remain editable with Backspace.

This is not an optimal coding algorithm; it favors predictable mnemonic choices and short high-priority keys. It retains previously auto-assigned keys on a best-effort basis across config edits. If a former assignment blocks newly required capacity, the lower-priority cached assignment is released; explicit `key` values always protect important bindings. Labels act as cache identities and must be unique. The cache contains no snippet text and is stored separately for each canonical configuration-file path; cache writes are atomic and optional. Read-only `validate` never writes cache state.

The number of supported entries is capped at 10000 for UI practicality; the theoretical 4-character alphabet has 26^4 leaves. This is not a keyboard-layout localization system yet; typed input is intentionally ASCII.

## Clipboard reliability

On Wayland, clipboard data is normally served by its owner process. Exiting an application immediately after claiming clipboard ownership can lose the data. `wl-copy` forks and serves requests in the background by default. Trievium writes the complete UTF-8 contents to its stdin, closes the pipe, waits for the invoking process and closes the picker **only after success**. Do not change this to `egui`'s ordinary copied-text output without validating clipboard persistence.

Clipboard managers may record selected content. Users should not put passwords/secrets in plaintext config or treat this as a secure secret manager.

## Quality / future work

- Verify interactive focus, float placement, theme and latency on a real Hyprland session. The GitHub CI environment has no compositor; compiling does not establish native interactive correctness.
- Measure cold-start overhead and reduce it. The current UI limits initial window height and virtualizes large lists, but initial OpenGL/egui startup is not guaranteed instant; benchmark before promising latency.
- Extend backends only after stable text/clipboard operation, with explicit and auditable actions.
- Optional lightweight editor and importers for Lexwright / external datasets, without putting the editor on the hot path.
- Optional stable assignment cache if pinning individual keys becomes cumbersome.
- Accessibility options, keyboard-layout handling, pagination and big-dataset UI after dogfooding.

## Privacy

The public repository contains only sample strings, no private expansions. Never commit the local configuration file or personal snippets.

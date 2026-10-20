# Architecture and decisions

## Purpose and boundaries

Trievium is a single-process, on-demand selection UI: **keybind -> prefix-tree lookup -> clipboard -> exit**. It is not a text expander, keylogger, clipboard manager, or background daemon. It never synthesizes keys into the previously focused app. The selection/action boundary allows future non-text payloads without coupling the key algorithm to any destination.

## Modules

- `config.rs`: strict versioned JSON input; filesystem discovery; non-destructive `init`.
- `trie.rs`: deterministic, prefix-free shortcut generation and lookup; no UI dependencies.
- `shortcut_cache.rs`: per-configuration persistent mnemonic assignments, storing labels and keys only.
- `clipboard.rs`: uses `wl-copy` via direct process arguments and piped input (no shell or quoting).
- `input.rs`: pure shortcut-input state machine and key-event classifier, with tests for modifier suppression, repeats and invalid prefixes.
- `launch.rs`: session-local advisory lock, preventing duplicate modal instances with no signals or process kills.
- `hyprland.rs`: bounded, one-shot Hyprland window correction. Finds only a mapped `trievium` window belonging to the current process, then sets floating/size/focus and centers it. Does not touch global rules, workspaces, or other windows.
- `main.rs`: small CLI (including read-only `doctor`) and focused egui picker.

Config is read once at launch, without a watcher, daemon, or network calls. Keyboard handling prefers egui key-down events and ignores OS key-repeat and Ctrl/Alt/Super combinations. The ASCII text-event fallback remains available even after unrelated native key events. Recently received native letter events suppress matching text for 180 ms across frames, preventing duplicate selections; modified chords are ignored. Unassigned letters are silently ignored. Failed clipboard operations reset the prefix for a safe retry. The visible-match list and next-letter hints are recomputed on prefix changes, not on every UI redraw. For now, changes take effect on the next invocation.

## Key assignment and invariants

1. Normalize pinned keys to uppercase; allow A-Z only, max four characters; reject duplicate and ancestor/descendant collisions.
2. Sort unpinned entries by descending `priority`, then label, then original position. Reserve old assignments from the local shortcut cache when they remain compatible with pinned shortcuts and leave enough space for the rest.
3. Find the smallest level L (max 4) with enough 26-ary leaf capacity after accounting for pinned terminals. A terminal of length d uses 26^(L-d) potential leaves.
4. At each priority step try short candidates first, mnemonic characters before alphabetical fallback. Only take a candidate if it does not collide with the trie **and** leaves enough leaf capacity for every remaining entry.
5. Insert terminal node; no terminal node can have children. Each trie node tracks occupied max-depth leaf slots so a saturated subtree is skipped in constant time during allocation rather than enumerating all of its leaves. A keypress at a terminal executes immediately. Nonterminal keypresses narrow the choices. Unknown sequences remain editable with Backspace.

This is not an optimal coding algorithm; it favors predictable mnemonic choices and short high-priority keys. It retains previously auto-assigned keys on a best-effort basis across config edits. If a former assignment blocks newly required capacity, the lower-priority cached assignment is released; explicit `key` values always protect important bindings. Labels act as cache identities and must be unique. The cache contains no snippet text and is stored separately for each canonical configuration-file path; cache writes are atomic and optional. Read-only `validate` never writes cache state.

The number of supported entries is capped at 10000 for UI practicality; the theoretical 4-character alphabet has 26^4 leaves. This is not a keyboard-layout localization system yet; typed input is intentionally ASCII.

## Linux launch and preflight

Hyprland normally tiles Wayland top-level windows. If `HYPRLAND_INSTANCE_SIGNATURE` is present, the picker uses a short-lived helper thread that waits up to two seconds for the newly mapped client to appear in `hyprctl -j clients`. It verifies both PID and exact app class before using any dispatcher, then uses Hyprland's Lua dispatcher objects (`hl.dsp.window.float`, `resize`, `center` and `hl.dsp.focus`) targeted to that window address. It never invokes the removed legacy `setfloating` dispatcher syntax, and every operation includes an explicit window selector. Centering uses `centerwindow` only after checking that the active address is still the picker's. After requesting focus it checks `hyprctl -j activewindow` to confirm that the active address is its own, and retries focus only during the first second of startup if necessary. The UI displays a focus warning when it isn't focused. It never modifies configuration and never terminates other processes.

The picker attempts to obtain a session-local lock in `XDG_RUNTIME_DIR/trievium.lock`. The file descriptor stays alive throughout the picker window lifetime, and the kernel releases its lock on exit. A second invocation simply exits instead of opening overlapping windows. No signals or process kills. If the runtime directory variable is not provided, the guard is omitted instead of preventing launch.

`trievium doctor` checks Wayland presence, `wl-copy --version`, and the validity of configuration plus generated shortcuts without copying any content or creating configuration files. The read-only check is deliberately not considered a full integration test.

## Input troubleshooting

`--diagnose-input` provides opt-in terminal diagnostics for egui focus, key metadata, text/paste event length, routed selection action, and clipboard outcome. It never serializes expansion content or raw arbitrary text, and it does not persist logs. This is meant to tell apart compositor focus loss, ignored/modified events, invalid shortcut codes, and clipboard errors without further guessing. An attempted launch while another picker owns the session lock emits a message rather than silently exiting.

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

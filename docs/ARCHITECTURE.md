# Architecture and decisions

## Purpose and boundaries

Trievium is a single-process, on-demand selection UI: **keybind -> prefix-tree lookup -> clipboard -> exit**. It is not a text expander, keylogger, clipboard manager, or background daemon. It never synthesizes keys into the previously focused app. The selection/action boundary allows future non-text payloads without coupling the key algorithm to any destination.

## Modules

- `config.rs`: strict versioned JSON input, filesystem discovery, non-destructive initialization, and atomic on-disk saves with external-edit detection.
- `manager.rs`: in-app CRUD editor, prefix collision validation, and optional pinned-key controls, isolated from the picker hot path.
- `trie.rs`: deterministic, prefix-free shortcut generation and lookup; no UI dependencies.
- `shortcut_cache.rs`: per-configuration persistent mnemonic assignments, storing labels and keys only.
- `clipboard.rs`: uses `wl-copy` via direct process arguments and piped input (no shell or quoting).
- `appearance.rs`: system/light/dark policy, theme-aware colors and argument validation.
- `input.rs`: focused shortcut-field text reducer, with terminal and invalid-prefix tests plus simulated egui input.
- `launch.rs`: session-local advisory lock, preventing duplicate modal instances with no signals or process kills.
- `hyprland.rs`: bounded, one-shot Hyprland window correction. Finds only a mapped `trievium` window belonging to the current process, then sets floating/size/focus and centers it. Does not touch global rules, workspaces, or other windows.
- `main.rs`: small CLI (including read-only `doctor`) and focused egui picker.

Config is read at launch, without a watcher, daemon, or network calls. The native entry editor writes local JSON atomically and updates the in-memory keymap when returning to the picker; no restart is needed for in-app edits. The picker now uses a focused native egui TextEdit, following the OTPick and Passflick approach, rather than manually routing raw Key/Text events. Egui owns keyboard focus and Backspace editing; the prefix tree reduces the input buffer on each edit and immediately executes a terminal shortcut without Enter. Unassigned letters are silently ignored. Failed clipboard operations clear the field. The visible-match list is recomputed only on prefix changes, not on every UI redraw. The normal UI deliberately omits tutorial captions, prompts, counts, and focus warnings; only actual clipboard errors are displayed. In-app saves update the picker immediately when Done returns.

## Key assignment and invariants

1. Normalize pinned keys to uppercase; allow A-Z only, max four characters; reject duplicate and ancestor/descendant collisions.
2. Sort unpinned entries by descending `priority`, then label, then original position. Reserve old assignments from the local shortcut cache when they remain compatible with pinned shortcuts and leave enough space for the rest.
3. Find the smallest level L (max 4) with enough 26-ary leaf capacity after accounting for pinned terminals. A terminal of length d uses 26^(L-d) potential leaves.
4. At each priority step try short candidates first, mnemonic characters before alphabetical fallback. Only take a candidate if it does not collide with the trie **and** leaves enough leaf capacity for every remaining entry.
5. Insert terminal node; no terminal node can have children. Each trie node tracks occupied max-depth leaf slots so a saturated subtree is skipped in constant time during allocation rather than enumerating all of its leaves. A keypress at a terminal executes immediately. Nonterminal keypresses narrow the choices. Unknown sequences remain editable with Backspace.

This is not an optimal coding algorithm; it favors predictable mnemonic choices and short high-priority keys. It retains previously auto-assigned keys on a best-effort basis across config edits. If a former assignment blocks newly required capacity, the lower-priority cached assignment is released; explicit `key` values always protect important bindings. Labels act as cache identities and must be unique. The cache contains no snippet text and is stored separately for each canonical configuration-file path; cache writes are atomic and optional. Read-only `validate` never writes cache state.

The number of supported entries is capped at 10000 for UI practicality; the theoretical 4-character alphabet has 26^4 leaves. This is not a keyboard-layout localization system yet; typed input is intentionally ASCII.

## Linux launch and preflight

Hyprland normally tiles Wayland top-level windows. If `HYPRLAND_INSTANCE_SIGNATURE` is present, the picker uses a short-lived helper thread that waits up to two seconds for the newly mapped client to appear in `hyprctl -j clients`. It verifies both PID and exact app class before using any dispatcher, then uses Hyprland's Lua dispatcher objects (`hl.dsp.window.float`, `resize`, `center`, `alter_zorder`, and `hl.dsp.focus`) targeted to that window address. The z-order operation is required after converting a tiled surface into a floating popup: focus alone does not guarantee that clicks reach the uppermost window. It never invokes the removed legacy `setfloating` dispatcher syntax, and every operation includes an explicit window selector. Centering uses `centerwindow` only after checking that the active address is still the picker's. After requesting focus it checks `hyprctl -j activewindow` once after a brief mapping delay and retries at most once if the window never received focus. It no longer continually steals focus and warps the pointer during the first three seconds. The native window is opaque, is not mouse-passthrough, and requests always-on-top behavior; focus correction is time-bounded. The normal UI does not display focus warnings. It never modifies configuration and never terminates other processes.

The picker attempts to obtain a session-local lock in `XDG_RUNTIME_DIR/trievium.lock`. The file descriptor stays alive throughout the picker window lifetime, and the kernel releases its lock on exit. A second invocation simply exits instead of opening overlapping windows. No signals or process kills. If the runtime directory variable is not provided, the guard is omitted instead of preventing launch.

`trievium doctor` checks Wayland presence, `wl-copy --version`, and the validity of configuration plus generated shortcuts without copying any content or creating configuration files. The read-only check is deliberately not considered a full integration test.

## Input troubleshooting

`--diagnose-input` provides opt-in terminal diagnostics for window and focused-field state, prefix length, selected shortcut, and clipboard outcome. It never serializes expansion content or raw arbitrary text, and it does not persist logs. This is meant to tell apart compositor focus loss, ignored/modified events, invalid shortcut codes, and clipboard errors without further guessing. An attempted launch while another picker owns the session lock emits a message rather than silently exiting.

## Editor usability and typography

The manager uses 24 px outer margins, a persistent top action bar, vertically scrollable narrow-screen fallback, and two-column layout where space allows. All entry text fields have visible outlined backgrounds that contrast in both themes. `Done` validates and atomically saves any edited form before returning to the picker; validation errors retain the form for correction. `Discard` is the explicit unsaved-changes reset. `Close` and Escape terminate immediately. On Hyprland the manager uses a verified, address-targeted resize and center only if the window is already floating. Resizing a tiled window is forbidden to avoid rearranging the user's tiling layout. Native viewport resizing is used on other compositors, never concurrently with Hyprland's native resize dispatch. The compact picker uses 18 px side margins and a measured 32 px row height; the virtual scroll list no longer paints a separator/large margin into each row. Shortcut labels are lowercase in both picker and manager while the trie retains case-insensitive ASCII matching. The short-lived picker retains its original copy-and-exit behavior.

On Linux, `typography.rs` asks fontconfig for locally installed Luciole Bold as the heading face and Lexica Ultralegible / Lexica Hyperlegible for body text (Atkinson Hyperlegible as a fallback). Missing fonts use the built-in egui defaults. No font binaries are bundled or copied into the repository. The heading is 21 px, body 16 px, buttons 15 px, and shortcut monospace 14 px. Light/dark portal themes still apply.

## Appearance policy

The default `--theme system` uses `portal_theme::follow` at eframe creation to read and watch the Linux XDG Desktop Portal `org.freedesktop.appearance` color-scheme. This integrates with desktop appearance services such as Darkman without independently calculating sunrise/sunset. A live portal update changes the egui theme and repaints the popup.

`--theme light` and `--theme dark` set explicit egui theme preferences and do not start the portal follower. Theme-aware UI background and status colors replace previously hardcoded dark-only painting. On Linux without an appearance portal, the egui fallback remains dark. No additional daemon, account, permissions or private settings are introduced.

## Clipboard reliability

A successful copy or Escape explicitly terminates this short-lived native picker because a viewport close request was observed to leave the popup alive during dogfooding. `wl-copy` has already completed its fork/handoff on success, so it owns the clipboard independently, while the kernel releases the advisory session lock on exit. The selection state also clears completed trie prefixes defensively.

On Wayland, clipboard data is normally served by its owner process. Exiting an application immediately after claiming clipboard ownership can lose the data. `wl-copy` forks and serves requests in the background by default. Trievium writes the complete UTF-8 contents to the launcher's stdin, closes the pipe, and waits for that launcher to exit. It **does not capture stderr or use `wait_with_output()`**, because the background clipboard owner can inherit the pipe and prevent EOF indefinitely. The picker exits after the direct launcher reports success, while the daemon continues serving paste requests. Do not change this to `egui`'s ordinary copied-text output without validating clipboard persistence.

Clipboard managers may record selected content. Users should not put passwords/secrets in plaintext config or treat this as a secure secret manager.

## Quality / future work

- Verify interactive focus, float placement, theme and latency on a real Hyprland session. CI includes an isolated Sway Wayland compositor with real virtual-keyboard selection, clipboard persistence, and Escape-to-close coverage. Actual Hyprland layout behavior still requires testing in that compositor.
- Measure cold-start overhead and reduce it. The current UI limits initial window height and virtualizes large lists, but initial OpenGL/egui startup is not guaranteed instant; benchmark before promising latency.
- Extend backends only after stable text/clipboard operation, with explicit and auditable actions.
- Polish the native entry editor after real usage; do not introduce importer dependencies.
- Continue preserving mnemonic assignments while maintaining pinned-key compatibility.
- Accessibility options, keyboard-layout handling, pagination and big-dataset UI after dogfooding.

## Privacy

The public repository contains only sample strings, no private expansions. Never commit the local configuration file or personal snippets.

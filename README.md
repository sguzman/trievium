# Trievium

**A keyboard-driven prefix-tree selector.** Native Rust + egui for Linux/Wayland.

Open it with a Hyprland keybind, press the displayed key sequence, and Trievium copies the selected text and exits. No Enter, search box, mouse, timeout, daemon, or auto-typing into other windows is required. Trievium is a **selection launcher**, not a global text-expansion hook or a replacement for the Lexwright editor.

## Quick start (EndeavourOS / Hyprland)

Install Rust/Cargo and `wl-clipboard` using your system's package manager; you also need usual native compilation dependencies such as `pkgconf` and `libxkbcommon`. For an Arch-based environment with Rust already installed:

```sh
sudo pacman -S --needed wl-clipboard pkgconf libxkbcommon
git clone https://github.com/sguzman/trievium.git
cd trievium
cargo build --release
./target/release/trievium init
./target/release/trievium validate
./target/release/trievium
```

`init` **never overwrites an existing config**. It writes a small, deliberately generic example to `~/.config/trievium/items.json`, or `$XDG_CONFIG_HOME/trievium/items.json`. Edit that JSON to replace sample snippets with your own. Use `trievium config-path` to locate the active file. `TRIEVIUM_CONFIG` or `--config PATH` can override it.

A Hyprland binding can then launch the compiled executable, for example:

```ini
bind = SUPER, P, exec, /absolute/path/to/trievium/target/release/trievium
```

Replace the absolute path with your actual location. Choose a keybind that does not conflict with your existing binds.

**Hyprland integration:** Trievium recognizes an active Hyprland session, waits briefly for its own window to map, then uses the Hyprland Lua dispatcher API to float, resize, center and focus **only its own process's window** (required for Hyprland 0.55+ Lua configurations). No compositor configuration edits or persistent rules are required. The integration runs once when opening, not continuously. On other compositors, Trievium relies on their default window-management behavior.

## Preflight and compiled Linux builds

After creating your configuration, `trievium doctor` performs a **read-only** check for Wayland session variables, the `wl-copy` program, and valid shortcuts. It does not touch the clipboard or launch a window. A passing result does not guarantee compositor focus or paste behavior; those still require real Hyprland QA.

CI also produces a packaged Linux x86_64 binary from each successful run. Open the latest [GitHub Actions run](https://github.com/sguzman/trievium/actions/workflows/ci.yml), and retrieve the `trievium-linux-x86_64` build artifact. The archive contains the executable and license; extract it anywhere in your home directory and execute it. `wl-clipboard` must still be installed. This is an alternative to compiling with Cargo, not a system-wide installer.

## Behavior

- `G` could immediately copy your Greeting and close.
- `T` could reveal several choices, with `TA` and `TB` as terminal sequences.
- `Esc` closes without copying; `Backspace` corrects a partial sequence.
- Selection prefers fresh letter key-down events, ignoring OS key-repeat and Ctrl/Alt/Super combinations. A single-character text-event fallback supports platforms that do not deliver native key-down events. Invalid letters leave the prefix unchanged.
- Clicking a row or its key badge also copies and exits.
- The popup sizes itself to the entry count (up to a maximum), shows remaining key choices, and virtualizes long lists for responsive scrolling.
- A session-local advisory lock prevents repeated hotkey presses opening multiple windows; no existing process is terminated. If `XDG_RUNTIME_DIR` is absent, locking is skipped.
- On Hyprland, the picker floats, requests focus and verifies it against the active-window address, retrying briefly at startup if necessary. An unfocused popup displays a visible warning instead of silently ignoring your keystrokes.
- A failed clipboard operation displays an error, clears the terminal sequence for retry, and **does not exit**.
- `wl-copy` runs without `--foreground`, leaving clipboard ownership alive after the UI exits. Paste normally into the previous application.
- Headless CI verifies Unicode and multiline bytes through the same subprocess boundary using a local file writer; it does not simulate a real Wayland clipboard.

The JSON file has `version: 1` and an `entries` array. Each entry has `label` and `text`; optional `key` pins a fixed sequence; optional integer `priority` makes automatic assignment earlier for more important entries. Keys use 1-4 ASCII letters. An assigned key cannot prefix any other key. Invalid configurations (including pinned-key collisions) are rejected before launching. There is no automatic paste, command execution, network access, or telemetry.

```json
{
  "version": 1,
  "entries": [
    {"label": "Email response", "text": "Thanks for your message.", "key": "E", "priority": 100},
    {"label": "Detailed response", "text": "Here are the details...", "priority": 50}
  ]
}
```

The shortcut generator tries mnemonic letters first, reserving longer-key capacity for all entries. **Automatic key assignments are remembered between launches**, including when new entries are added. This best-effort cache lives under `$XDG_STATE_HOME/trievium/` (normally `~/.local/state/trievium/`), with a separate cache per configuration file. It stores **labels and keys only**, never clipboard text. Pinned `key` values always take precedence. If every existing single-letter key is occupied, one or more automatic keys must change to make room; pinned keys never move. `trievium validate` prints assignments without writing the cache, and opening the picker saves them. Renaming an entry changes its identity; labels must be unique.

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

See [architecture](docs/ARCHITECTURE.md) for algorithm details, constraints and next milestones.

## Status

**v0.2.0 ongoing development:** file-backed plaintext selection, deterministic prefix-free assignment, CLI validation, native picker and Wayland clipboard. This is an initial implementation, **not yet personally dogfooded on Hyprland**. No tray, background service, editor, Lexwright importer, automated typing, or generalized actions yet. Keeping those out of the picker is intentional.

MIT licensed; see [LICENSE](LICENSE).

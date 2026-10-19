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

Replace the absolute path with your actual location. Choose a keybind that does not conflict with your existing binds. The picker must receive keyboard focus; if your Hyprland rules prevent focus or tile small windows, configure a float/focus rule for the `trievium` app class in your existing local Hyprland config (do not blindly replace unrelated rules).

## Behavior

- `G` could immediately copy your Greeting and close.
- `T` could reveal several choices, with `TA` and `TB` as terminal sequences.
- `Esc` closes without copying; `Backspace` corrects a partial sequence.
- Clicking a row or its key badge also copies and exits.
- A failed clipboard operation displays an error and **does not exit**.
- `wl-copy` runs without `--foreground`, leaving clipboard ownership alive after the UI exits. Paste normally into the previous application.

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

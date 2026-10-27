# Trievium

**A keyboard-driven prefix-tree selector.** Native Rust + egui for Linux/Wayland.

Open it with a Hyprland keybind, press the displayed key sequence, and Trievium copies the selected text and exits. Click **Edit** to manage entries directly in the native application. No Enter, mouse, timeout, daemon, or auto-typing into other windows is required. A small focused shortcut input receives the keys; this is not a search box or editor. Trievium is a **selection launcher**, not a global text-expansion hook or a replacement for the Lexwright editor.

## Quick start (EndeavourOS / Hyprland)

Install Rust/Cargo and `wl-clipboard` using your system's package manager; you also need usual native compilation dependencies such as `pkgconf` and `libxkbcommon`. For an Arch-based environment with Rust already installed:

```sh
sudo pacman -S --needed wl-clipboard pkgconf libxkbcommon noto-fonts-emoji
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

**Hyprland integration:** Before creating its Wayland window, Trievium installs an exact-app-ID, named **runtime floating rule** using `hyprctl eval` (Hyprland 0.55+ Lua configuration). This prevents the workspace reflow caused by floating it *after* mapping. The named rule is reused across launches, without modifying Hyprland configuration files. After mapping, it verifies that its own PID/class window is already floating before resizing, centering, raising, and focusing it. If the pre-map rule is rejected, the popup does not open tiled. On other compositors, window management remains compositor-defined.

## Manage entries in Trievium

Click **Edit** in the picker, or launch `trievium manage` to open the larger management view immediately. The editor has a padded, scrollable, responsive layout; on Hyprland, Trievium resizes its own floating window when changing between picker and editor. This is the normal way to create and maintain your selection library. The app provides **New**, **Save**, **Up/Down** (reorder the selected entry), **Delete** (with confirmation), **Discard**, **Done**, and **Close**. **Done** validates and saves the current entry before returning to the picker. An invalid entry stays open with an error instead of losing the text. **Close** or **Escape** exits without saving the current form; **Discard** explicitly resets it. Previously saved entries remain intact. Each entry has a name, multiline text to copy, and an optional pinned 1–4 letter shortcut. Numeric automatic priority has been removed; selection is based on the shortest unique prefix. The list displays entries in your chosen order, with an OKLCH-derived color marker before each lowercase shortcut (matching how you type). Keys are normalized internally; you do not need Shift. A saved change is available as soon as you return to the picker.

The editor uses visible outlined fields for name, multiline copied text, and optional pinned shortcut. Click the **Accent color** swatch for a custom color, or click **Auto** to restore its stable, label-derived OKLCH color. The picker and library put the accent before the prefix key. **Up/Down** reorder the selected entry and save the order without changing pinned shortcuts; the picker displays this order instead of sorting by key. Existing JSON libraries gain automatic colors without a migration. egui 0.37 uses local system-font fallback for colored emoji; on Arch, install `noto-fonts-emoji` if colorful emoji are missing. The picker has 18 px side padding and compact fixed-height rows, instead of clipping entries in an undersized window. The app saves the same local `items.json` file atomically and validates duplicate labels and conflicting prefixes before writing. Errors leave your previous configuration intact. If another program edited `items.json` while Trievium was open, the editor refuses to overwrite it. You can remove every entry and start with an empty library. On a first launch with no configuration, Trievium starts with an empty library rather than requiring `init`.

The short-lived picker still has its immediate keyboard-to-clipboard exit behavior; the editor is used only when requested. No Lexwright integration or import workflow is included.

## Appearance (system, light, dark)

By default, Trievium follows the desktop's **light/dark preference** through the XDG Desktop Portal. When Darkman changes the system appearance at sunrise or sunset, Trievium follows that preference, including while the popup is open. The portal supplies the color scheme; Trievium does not need a location service, scheduler, or background daemon.

Override it for a particular launch:

```sh
trievium --theme system
trievium --theme light
trievium --theme dark
```

Use the same flag with `./target/release/trievium` or in your Hyprland keybind. `--color-mode` is also accepted as an alias for `--theme`. The default is `system`; explicit `light` and `dark` ignore later system changes for that invocation.

In system mode, a working appearance portal is required for Linux to report the preference. Without one, egui's dark fallback remains in effect. Both modes use the native egui widgets and an **opaque, theme-matched background**, so the popup remains legible in light mode rather than inheriting its old fixed dark background.

## Diagnosing missed shortcuts

Run `trievium --diagnose-input` instead of `trievium` when a key is ignored. The popup displays a diagnostic-mode indicator and the launching terminal reports window focus, shortcut-field focus, normalized input length, selected shortcut, and clipboard success/failure. It does **not** print stored snippets or typed text contents. No diagnostic file is saved or uploaded. This flag is opt-in and does not affect ordinary launches.

If another instance is already open, a new invocation prints an explicit message rather than silently returning. Close the previous picker before retesting an updated binary so the old process isn't mistaken for the new build.

## Preflight and compiled Linux builds

After creating your configuration, `trievium doctor` performs a **read-only** check for Wayland session variables, the `wl-copy` program, and valid shortcuts. It does not touch the clipboard or launch a window. A passing result does not guarantee compositor focus or paste behavior; those still require real Hyprland QA.

Stable builds are published on [GitHub Releases](https://github.com/sguzman/trievium/releases). The **v1.0.0** release includes `trievium-linux-x86_64.tar.gz` (binary plus MIT license) and `SHA256SUMS`. Keep the files together and verify with `sha256sum -c SHA256SUMS` before extracting. The executable still requires `wl-clipboard` and is not a distribution installer. Every successful CI run also carries a temporary build artifact. See the [release/compatibility policy](docs/RELEASE.md).

## Behavior

- `G` could immediately copy your Greeting and close.
- If `ta` and `tb` exist, `t` is ambiguous, so Trievium waits. `ta` or `tb` chooses immediately. If only `ta` exists under `t`, pressing `t` alone selects it.
- `Esc` closes without copying; `Backspace` corrects a partial sequence.
- A focused native egui shortcut input receives typing and Backspace. The prefix tree selects immediately at the first unique match, waits while several entries match, and ignores letters matching none; no Enter required. No raw event router or cross-frame timer is needed.
- Clicking a row or its key badge also copies and exits.
- The popup contains only its title, focused shortcut field, and selectable entries (plus an error if copying fails). No tutorials, prompts, counts, key-hint captions, or focus warnings are shown. It sizes to the entry count and virtualizes long lists.
- A session-local advisory lock prevents repeated hotkey presses opening multiple windows; no existing process is terminated. If `XDG_RUNTIME_DIR` is absent, locking is skipped.
- On Hyprland, the picker floats and focuses once on startup, with at most one delayed correction if focus never arrived. It never repeatedly reclaims focus during editing or mouse interaction. The window has an explicit opaque background and is not mouse-passthrough. Startup focus correction is bounded; no permanent focus-stealing daemon runs.
- A failed clipboard operation displays an error, clears the terminal sequence for retry, and **does not exit**.
- A successful selection or Esc terminates the one-shot picker process, instead of depending on a viewport-close request that left the window alive. Completed selections also clear their key prefix defensively.
- `wl-copy` runs without `--foreground`, leaving clipboard ownership alive after the UI exits. Paste normally into the previous application.
- Headless CI verifies Unicode and multiline bytes through the same subprocess boundary using a local file writer; it does not simulate a real Wayland clipboard.

The on-disk library JSON schema remains **`version: 1`**, independent of application v1.0.0, and has an `entries` array. Each entry has `label` and `text`; optional `key` reserves a prefix-free shortcut. The legacy `priority` field remains readable in existing JSON but is ignored. Keys use 1-4 ASCII letters. An assigned key cannot prefix any other key. Invalid configurations (including pinned-key collisions) are rejected before launching. There is no automatic paste, command execution, network access, or telemetry.

```json
{
  "version": 1,
  "entries": [
    {"label": "Email response", "text": "Thanks for your message.", "key": "E"},
    {"label": "Detailed response", "text": "Here are the details..."}
  ]
}
```

The shortcut generator tries mnemonic letters first, reserving longer-key capacity for all entries. Existing pinned and remembered codes stay stable where possible, with no numeric priority ranking. Badges display the shortest unique prefix to type, even when a reserved code is longer. **Automatic key assignments are remembered between launches**, including when new entries are added. This best-effort cache lives under `$XDG_STATE_HOME/trievium/` (normally `~/.local/state/trievium/`), with a separate cache per configuration file. It stores **labels and keys only**, never clipboard text. Pinned `key` values always take precedence. If every existing single-letter key is occupied, one or more automatic keys must change to make room; pinned keys never move. `trievium validate` prints assignments without writing the cache, and opening the picker saves them. Renaming an entry changes its identity; labels must be unique.

## Stable releases and updates

This project is maintained on `main`. Stable releases include an annotated `vX.Y.Z` tag, matching Cargo package version, [changelog](CHANGELOG.md), verified binary and checksum. Build locally with `cargo build --release --locked`, or download the versioned release archive. Existing `items.json` libraries are retained on upgrade. Read the [release and compatibility contract](docs/RELEASE.md) and [MVP acceptance record](docs/MVP_ACCEPTANCE.md).

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

See [architecture](docs/ARCHITECTURE.md) for algorithm details, constraints and next milestones.

## Status

**v1.0.0 stable MVP:** the one-shot prefix picker, clipboard copy-and-exit, native library editor, persistent data, system light/dark appearance, and non-disruptive Hyprland floating window are accepted. Owner dogfooding and Rust/Wayland integration tests support the milestone. The feature set is closed; work after graduation is maintenance and bug fixes, not new functionality. No tray, persistent daemon, Lexwright integration/importing, automated typing, or generalized actions are promised. See [ROADMAP.md](ROADMAP.md) for the closed milestone and maintenance boundary.

MIT licensed; see [LICENSE](LICENSE).

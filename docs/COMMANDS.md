# Command-tree mode (new scope)

Trievium's existing copy HUD and `items.json` are intentionally preserved.
The command HUD is an independent mode that shares the application's native
window and appearance infrastructure, **not** its copy-item schema or mnemonic
shortest-unique-prefix semantics.

## CLI and storage

| Command | Purpose |
| --- | --- |
| `trievium` or `trievium pick` | Original prefix-to-clipboard HUD |
| `trievium manage` | Original copy library editor |
| `trievium commands` | New command-tree HUD |
| `trievium commands init` | Create empty registry, refusing overwrite |
| `trievium commands edit` | Open native command/group editor |
| `trievium commands validate` | Validate registry without modifying it |
| `trievium commands config-path` | Print independent registry location |

The command registry is `$XDG_CONFIG_HOME/trievium/commands.json`, normally
`~/.config/trievium/commands.json`. The command CLI's `--config PATH`
overrides **only** the command registry. Original `TRIEVIUM_CONFIG` and the
copy CLI continue to apply to `items.json` only. A missing command registry
starts empty without sample or destructive commands. Existing `items.json`
data and its mnemonic cache are never loaded, migrated, or rewritten by this
mode.

Example `commands.json`:

```json
{
  "version": 1,
  "groups": [
    { "prefix": "w", "label": "Windows" },
    { "prefix": "s", "label": "Workspaces" },
    { "prefix": "a", "label": "Applications" }
  ],
  "commands": [
    {
      "sequence": "w f",
      "label": "Toggle fullscreen",
      "action": {
        "type": "hyprland",
        "expression": "hl.dsp.window.fullscreen({ mode = \"fullscreen\" })",
        "focus_original": true
      }
    },
    {
      "sequence": "s 6",
      "label": "QA workspace",
      "action": {
        "type": "hyprland",
        "expression": "hl.dsp.focus({ workspace = 6 })",
        "focus_original": false
      }
    },
    {
      "sequence": "a t",
      "label": "Terminal",
      "action": { "type": "exec", "program": "kitty", "args": [] }
    }
  ]
}
```

Bind `Super+Space` in Hyprland to the installed executable.
For Hyprland 0.55+ Lua configuration:

```lua
hl.bind("SUPER + SPACE", hl.dsp.exec_cmd("trievium commands"))
```

For a legacy `hyprland.conf` configuration:

```ini
bind = SUPER, SPACE, exec, trievium commands
```

Sequences consist of 1–12 ASCII alphanumeric keys; spaces in the JSON are
just for readability. `w f` means press `w`, then `f`, **not**
`Super+w` followed by `f`. A command only executes when its full
configured sequence has been entered. Unlike the copy picker, a unique
partial prefix is never automatically executed. Colliding and ancestor
sequences are rejected; groups are display labels for prefix branches.
Escape cancels, Backspace edits, and clicking a HUD branch advances it.

Execution uses direct argv-based process spawning (never `sh -c`). Hyprland
commands are dispatched through `hyprctl dispatch` using Hyprland 0.55+ Lua
expressions (`hl.dsp.*`). The `expression` field must be a valid dispatcher
expression. See [Hyprland dispatchers](https://wiki.hypr.land/configuring/core/dispatchers/).
Focus-dependent actions must
set `focus_original: true` (the default): the HUD captures the window originally focused
before mapping, checks that it remains mapped, restores focus and confirms
that focus before invoking the dispatcher. Workspace focus dispatchers use `focus_original: false`. Hyprland return codes are checked.
Commands run on worker threads, not on the egui render path. Errors leave
the HUD visible for correction, rather than pretending success.


## Local Hyprland QA before acceptance

Do not merge or publish this feature until it has been exercised in a real
Hyprland 0.55+ session. In a checkout of `feature/command-tree`, build with:

```sh
git pull --ff-only
cargo build --release --locked
./target/release/trievium commands edit
./target/release/trievium commands validate
./target/release/trievium commands
```

Opening `commands edit` on a fresh installation initializes *only*
`commands.json`; the original clipboard library is left untouched.
The editor's filesystem reads, validation, and saves run outside egui's
UI thread. Changes saved with **Done** are immediately available in the HUD.

Before binding the launcher to `Super+Space`, check that the key combination
is not already claimed. Then test these cases with harmless commands:

1. **Initial state:** no config -> editor opens empty without touching
   `items.json`. Invalid command JSON should produce an error, not reset data.
2. **Nested prefixes:** `w` then `a` must only navigate, while `w a t`
   may launch an explicitly configured action. Invalid keys are ignored;
   Backspace removes a prefix; Escape cancels without side effects.
3. **Focus-sensitive action:** from a regular tiled window, use a reversible
   command such as fullscreen. Verify it affects that original window, **not**
   the Trievium HUD, and does not tile or rearrange other windows.
4. **Global action:** explicitly set `focus_original: false` on a workspace
   switch (for example, workspace 6). Verify the target workspace activates
   and the HUD exits without stealing focus back.
5. **Editor round trip:** add/edit/reorder/delete a command or group, save,
   return to the picker, and verify the new structure. While the editor is
   open, modify the file from another process and confirm Save refuses to
   overwrite the external change.
6. **Regression:** launch the *original* `trievium` copy HUD, select a
   snippet, paste it, and verify it is unchanged. Confirm
   `trievium manage` still edits the old library.

GitHub Actions exercises the native command HUD and editor under headless
Sway, including exact selections, keyboard input, first-run initialization,
and clipboard isolation. This does **not** establish the above Hyprland
window-management behavior; only a real target-session QA run can do so.

## Acceptance and remaining work

This is a first implementation slice, **not** a completed command-mode
release. The GUI command editor and atomic external-edit-safe writes are now implemented,
but still require CI and user-facing Hyprland dogfooding. Final release checks
remain required. The
copy HUD must be regression-tested throughout.

See [ROADMAP.md](../ROADMAP.md) for evidence-weighted completion accounting.

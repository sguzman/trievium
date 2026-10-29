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

Bind `Super+Space` in Hyprland to the installed executable:

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

## Acceptance and remaining work

This is a first implementation slice, **not** a completed command-mode
release. The GUI command editor and atomic external-edit-safe writes are now implemented,
but still require CI and user-facing Hyprland dogfooding. Final release checks
remain required. The
copy HUD must be regression-tested throughout.

See [ROADMAP.md](../ROADMAP.md) for evidence-weighted completion accounting.

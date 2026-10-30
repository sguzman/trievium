# Trievium — accepted MVP and narrowly reopened feature scope

The v1.0.0 MVP was accepted on 2026-10-22. On 2026-10-27 the owner explicitly requested a **limited v1.1.0 feature scope**: color emoji, OKLCH entry accents with manual overrides, and editor reordering. This does not reopen unrelated features.

## M0 — MVP (100% accepted scope)

| Phase | Deliverable | Status | Evidence |
| --- | --- | --- | --- |
| ✅ P1 · Engine | Prefix-free trie, pinned and remembered shortcuts, shortest-unique selection | Accepted | Rust trie/input tests |
| ✅ P2 · Picker | One-shot native UI, exact Wayland clipboard handoff, close-after-copy | Accepted | Wayland integration tests and dogfooding |
| ✅ P3 · Editor | Library CRUD, atomic saves, validation, full-height list | Accepted | Editor tests and owner visual check |
| ✅ P4 · Desktop | Non-disruptive Hyprland floating popup; system-aware light/dark | Accepted | Owner Hyprland session and QA |
| ✅ P5 · Release readiness | Frozen contract, changelog, acceptance evidence, CI-gated package and checksum | Implemented; publication gated on green v1 CI | [MVP acceptance](docs/MVP_ACCEPTANCE.md) and [release policy](docs/RELEASE.md) |

## M1 — Owner-requested visual and ordering improvements (v1.1.0)

- 🖼️ Native color emoji fallback through egui 0.37, using system-installed emoji fonts
- 🎨 Deterministic OKLCH accent per label, with stored optional `#RRGGBB` override
- ↕️ Persisted library order and editor move controls, without reassigning pinned shortcuts
- 🧪 UI, configuration and compatibility tests plus Linux/Wayland validation

## M2 — Maintenance (continuous)

- 🐛 Investigate and correct reproducible bugs.
- 🧪 Preserve selection, data and clipboard regressions tests.
- 🔧 Maintain compatible Linux, Hyprland and Rust dependency support.
- 📝 Keep patch-version release notes, verified assets and documentation accurate.

A perpetual maintenance queue has no completion percentage. No additional capabilities are scheduled beyond the specifically requested v1.1.0 improvements.

## Evidence rule

Acceptance must be backed by actual behavior and tests. A subsequently discovered purpose-defeating regression is a bug to resolve, not something overridden by a prior 100% indicator.


## M3 — Command-tree HUD (owner-requested, 2026-10-29)

**Verified: 35% · last checkpoint: 35% · delta: +0 percentage points.**
This is a separate, finite expansion, not a reopening of accepted M0/M1.
The original copy HUD and `items.json` must remain independent from
`trievium commands`, whose only registry is `commands.json`.

| Gate | Weight | Acceptance evidence / remaining action | Status |
| --- | ---: | --- | --- |
| 🧱 C1 — Command CLI isolation | 15% | Existing copy picker untouched; command CLI, singleton and no-copy-mutation tests | ✅ Verified |
| 🗂️ C2 — Separate registry | 20% | Versioned `commands.json`, conflicts, safe initialization, atomic/symlink saves and tests | ✅ Verified |
| ⌨️ C3 — Native command HUD | 30% | Exact nested sequences, cancellation, execution and original-focus protection; real Hyprland fullscreen/workspace QA **still required** | 🟡 Active |
| 🛠️ C4 — Command editor | 20% | Group/command CRUD, reorder, safe save/reload, keyboard/click and visual QA on owner desktop **still required** | 🟡 Active |
| 🧪 C5 — Regression and release | 15% | Green Rust, Clippy, headless Wayland copy/commands tests and package; real-session acceptance and release decision **still required** | 🟡 Active |

### Verified checkpoint and dependencies

- [Latest green implementation CI `2692cd10`](https://github.com/sguzman/trievium/actions/runs/40979341694): **93 Rust tests, zero failures**, strict Clippy, CLI smoke, headless Wayland E2E, optimized release binary, checksum-verified package, and uploaded artifact. Command resolution now refuses to execute when a pasted text event contains trailing characters after the first complete shortcut (for example, `wf extra`). The reference reducer and compiled index agree on the result; ordinary incremental entry remains exact. Implementation commits `3156ba40` and `2692cd10`. Documentation in [docs/COMMANDS.md](docs/COMMANDS.md). Real-session Hyprland QA remains the acceptance gate.
- [Latest green implementation CI `5a0b353a`](https://github.com/sguzman/trievium/actions/runs/40978361353): **92 Rust tests, zero failures**, strict Clippy, CLI smoke, headless Wayland, optimized release build, and checksum package. Command-mode `--help`/`-h` and `--version`/`-V` are now available without initializing `commands.json` or touching the original copy registry. Test coverage includes all four flags and both files remaining absent. Implementation commits `df3752ad` and `5a0b353a`; usage is documented in [docs/COMMANDS.md](docs/COMMANDS.md). Real Hyprland acceptance remains pending.
- [Latest green implementation CI `c3d62488`](https://github.com/sguzman/trievium/actions/runs/40977301318): **92 Rust tests, zero failures**, strict Clippy, CLI smoke, headless Wayland regression, optimized release build and checksum package. The new dispatcher-response regression rejects error text even when the command exits successfully. Internal popup focus/geometry and user Hyprland actions now share the same explicit `ok` check. This does not verify live Hyprland behavior.
- [Latest green CI `4c12e689`](https://github.com/sguzman/trievium/actions/runs/40972989550): **91 Rust tests, zero failures**, strict Clippy, CLI smoke, both Wayland HUD regressions, optimized release build, and checksum-verified package. The added group-order persistence test passed. This does not replace real-session Hyprland QA.
- [Green CI `add402eb`](https://github.com/sguzman/trievium/actions/runs/40970574576):
  **90 Rust tests**, strict Clippy, original copy/command HUD Wayland E2E,
  editor first-run, optimized release build and package checksum. Native egui
  pointer tests now cover **Done**, **New command**, **New group**, **Delete**,
  **Up** and **Down**, with persisted create/delete/reorder effects and
  original `items.json` isolation. Incomplete drafts cannot be saved; valid
  drafts survive atomic save/reload. These deterministic UI checks do **not**
  substitute for live Hyprland focus, keyboard or visual acceptance.
- Prior green checks: [88 tests / native create](https://github.com/sguzman/trievium/actions/runs/40969183014),
  [86 tests / Done interaction](https://github.com/sguzman/trievium/actions/runs/40963626290),
  [85 tests / external-edit consistency](https://github.com/sguzman/trievium/actions/runs/40960139391), [83 tests / indexed HUD](https://github.com/sguzman/trievium/actions/runs/40958445466),
  [79 tests / prior round](https://github.com/sguzman/trievium/actions/runs/40954486737), [76 tests / round 3](https://github.com/sguzman/trievium/actions/runs/40951700563),
  [71 tests / round 2](https://github.com/sguzman/trievium/actions/runs/40944215754),
  [69 tests / initial expansion](https://github.com/sguzman/trievium/actions/runs/40939985417).
- Additional committed fixes include relative paths, command-editor argv caching,
  singleton creation ordering, bounded compositor focus propagation,
  prefix validation, no-render-thread I/O, symlink preservation and
  unsaved-change confirmation. Verify every later branch HEAD in CI; earlier
  green runs never prove new commits green.
- Additional verified hardening: reject missing, unmapped and own-class
  original focus targets; complete direct-editor registry reads before the
  bounded popup focus worker; prevent stale close confirmation while saving.
  Command navigation now uses an immutable prefix index compiled outside
  egui's render thread and rebuilt on a worker after editing.
- **Group editor follow-up (2026-10-29):** the editor now exposes Up/Down
  controls for group rows, in addition to command rows. The new bounded-move
  and save/reload regression checks serialized group order and preservation
  of the existing copy library. Implementation commit [`4c12e689`](https://github.com/sguzman/trievium/commit/4c12e689ca6692214ea7542a2e7f85255976dccb); [CI run](https://github.com/sguzman/trievium/actions/runs/40972989550) passed all stages, including optimized release build and checksum packaging. Group row order does not change shortcut-key sorting in the HUD.
- **Local blocker:** use the bounded [real Hyprland QA checklist](docs/COMMANDS.md)
  for reversible fullscreen on the original window, workspace switching,
  interactive command/editor CRUD, and the accepted copy picker. Headless
  Sway automation cannot establish actual Hyprland focus/float behavior.
- **Release policy:** keep [draft PR #1](https://github.com/sguzman/trievium/pull/1)
  unmerged; no tag or release until all remaining gates pass.

Historical round-by-round detail is retained in the branch's Git history;
this roadmap deliberately retains only current weighted acceptance and
durable verification links.

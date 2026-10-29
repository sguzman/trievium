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

- [Green CI `17cc5623`](https://github.com/sguzman/trievium/actions/runs/40963626290):
  **86 Rust tests**, strict Clippy, copy/command HUD keyboard E2E, editor
  first-run, release build and checksum. A deterministic egui pointer
  press/release test now verifies the native **Done** control, async reload
  and editor-to-picker completion without changing disk contents. The
  flaky compositor-level synthetic pointer attempt was removed after
  proving unreliable in headless Sway; it is **not** claimed as live
  desktop acceptance.
- Prior green checks: [85 tests / external-edit consistency](https://github.com/sguzman/trievium/actions/runs/40960139391), [83 tests / indexed HUD](https://github.com/sguzman/trievium/actions/runs/40958445466),
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
- **Local blocker:** use the bounded [real Hyprland QA checklist](docs/COMMANDS.md)
  for reversible fullscreen on the original window, workspace switching,
  interactive command/editor CRUD, and the accepted copy picker. Headless
  Sway automation cannot establish actual Hyprland focus/float behavior.
- **Release policy:** keep [draft PR #1](https://github.com/sguzman/trievium/pull/1)
  unmerged; no tag or release until all remaining gates pass.

Historical round-by-round detail is retained in the branch's Git history;
this roadmap deliberately retains only current weighted acceptance and
durable verification links.

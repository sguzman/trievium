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


## M3 — Command-tree HUD (owner-authorized expansion, 2026-10-29)

This is a **new, finite scope** explicitly requested after acceptance of the
copy-mode MVP, not a retroactive reopening of earlier milestones.
Keep the copy HUD and `items.json` entirely intact. The new command mode
must use `commands.json`. The following weights total 100%; only verification
of acceptance criteria contributes to completion.

| ID / weight | Deliverable and objective acceptance criteria | Dependencies | Status |
| --- | --- | --- | --- |
| 🧱 C1 · 15% | `trievium commands` CLI independent of original picker, session locking and no copy config reads/writes; copy CLI remains compatible | Existing copy app | Active — code committed, CI pending |
| 🗂️ C2 · 20% | Versioned separate `commands.json`; invalid and conflicting sequences rejected; safe initialization and round-trip/unit tests | C1 | Active — code committed, CI pending |
| ⌨️ C3 · 30% | Live native HUD, nested exact sequences, actionable commands, Escape/Backspace/click, Hyprland original-focus safety, nonblocking execution; automated and Hyprland QA | C2 | Active — implemented, CI + desktop QA pending |
| 🛠️ C4 · 20% | Native editor for groups and command CRUD/reorder, atomic writes with external-edit conflict detection, validation, usable picker/editor transition | C2–C3 | Pending |
| 🧪 C5 · 15% | Regression tests for original copy mode, CI, documentation, actual Hyprland dogfooding, packaging and release readiness | C1–C4 | Pending |

**Initial baseline: 0% verified** for this new scope. Committed but untested
work remains active, not complete. Do not combine this new-scope percentage with
the already accepted copy MVP. The implementation branch is
`feature/command-tree`; see [command mode specification](docs/COMMANDS.md).

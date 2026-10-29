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
| 🧱 C1 · 15% | `trievium commands` CLI independent of original picker, session locking and no copy config reads/writes; copy CLI remains compatible | Existing copy app | ✅ Verified — full green CI, copy keyboard E2E and command CLI smoke |
| 🗂️ C2 · 20% | Versioned separate `commands.json`; invalid and conflicting sequences rejected; safe initialization and round-trip/unit tests | C1 | ✅ Verified — configuration, collision, atomic persistence and no-copy-mutation tests |
| ⌨️ C3 · 30% | Live native HUD, nested exact sequences, actionable commands, Escape/Backspace/click, Hyprland original-focus safety, nonblocking execution; automated and Hyprland QA | C2 | Active — 71 Rust tests and Sway command-tree E2E green; actual Hyprland original-focus/float QA pending |
| 🛠️ C4 · 20% | Native editor for groups and command CRUD/reorder, atomic writes with external-edit conflict detection, validation, usable picker/editor transition | C2–C3 | Active — atomic-save tests, first-run Sway editor launch and regression checks green; manual editor CRUD/visual QA pending |
| 🧪 C5 · 15% | Regression tests for original copy mode, CI, documentation, actual Hyprland dogfooding, packaging and release readiness | C1–C4 | Active — build, packaging and regressions green; real-session QA and release decisions remain |

**Initial baseline: 0% verified** for this new scope. Committed but untested
work remains active, not complete. Do not combine this new-scope percentage with
the already accepted copy MVP. The implementation branch is
`feature/command-tree`; see [command mode specification](docs/COMMANDS.md).

**First verification checkpoint: 35%** (previous 0%; +35 percentage
points). Evidence: [successful CI run 40939985417](https://github.com/sguzman/trievium/actions/runs/40939985417)
validated 69 Rust unit tests, strict Clippy, CLI non-overwrite checks, copy
clipboard regression, headless Wayland exact-command execution, and package
checksum. C3 and C4 remain active because modern Lua dispatch and native
command-editor QA require the newer branch checks and real Hyprland session
verification. C5 remains pending release readiness. No main-branch merge or
release has been performed.


**Round 2 checkpoint: 35% verified** (previous 35%; **+0 percentage
points**). The additional work removes UI-thread editor loads, saves and
Discard reads; virtualizes long editor lists; caches HUD choices; and adds
three-level navigation, Backspace, invalid-key and first-run editor tests.
[Green CI run 40944215754](https://github.com/sguzman/trievium/actions/runs/40944215754)
confirmed **71 Rust tests**, strict Clippy, nested command keyboard E2E,
clipboard isolation, first-run editor initialization, and packaging.
These checks improve coverage of C3–C5 but do not satisfy the outstanding
**real Hyprland** and interactive editor acceptance criteria, so their
weights remain unearned. The follow-on non-reclaiming command-popup
focus policy is also awaiting its own CI run and local Hyprland QA.

Next: confirm the branch HEAD CI, then run the bounded manual checklist
in [docs/COMMANDS.md](docs/COMMANDS.md) inside the owner's actual Hyprland
session. Do not merge or publish before those checks pass.


**Round 3 checkpoint: 35% verified** (previous 35%; **+0 percentage
points**). This round addressed concrete correctness and responsiveness
defects within C3/C4: atomic writes now preserve configuration-store
symlinks and reject mid-edit link retargeting; the editor preserves
unsaved changes, prevents exit while saving, and transfers registry
ownership to the worker without a large egui-thread clone. Command
launching now serializes its original-window focus restoration against
initial HUD mapping so a late compositor focus operation cannot target
the wrong window. The command HUD refocuses on execution errors.
[Green CI run 40951700563](https://github.com/sguzman/trievium/actions/runs/40951700563)
verified **76 Rust tests**, strict Clippy, original copy/command HUD
keyboard E2E, command editor launch, first-run registry isolation, and
release-build checksum. The following documentation-only change does
not affect the tested implementation.

**Blocking acceptance:** C3/C4/C5 continue to require owner-session
Hyprland focus/float/fullscreen/workspace testing and interactive
command-editor CRUD verification as defined in
[docs/COMMANDS.md](docs/COMMANDS.md). None of those desktop checks can
be claimed complete from headless Sway CI. No main merge or release
has been performed.


**Round 4 checkpoint: 35% verified** (previous 35%; **+0 percentage
points**). Recovered the interrupted iteration, verified the
argv editor cache and first-run relative configuration behavior,
and reduced group-prefix validation from a per-group full-command
scan to an ordered prefix lookup. Added regression coverage for
groups that do not contain commands and groups that overlap terminals.
[Green CI run 40953760893](https://github.com/sguzman/trievium/actions/runs/40953760893)
checks the code through `7cad5645`. Subsequent commits add the
HUD singleton first-run race test, bounded Hyprland focus-state
propagation, and corrected documented workspace selector quoting;
their newer CI runs are tracked separately and must pass before
release. No new milestones were introduced.

**Next gate remains real owner-session Hyprland QA**:
reversible fullscreen on the original window, global workspace
switching, native command-editor CRUD and visual behavior, and
copy-picker regression. See [docs/COMMANDS.md](docs/COMMANDS.md).
Do not mark C3–C5 verified, merge, or release until that evidence exists.


**Round 4 checkpoint: 35% verified** (previous 35%; **+0 percentage
points**). Focused fixes within existing M3 scope: the native editor now
caches editable program-argument text instead of rebuilding it on every
paint; a bare relative `commands.json` path initializes correctly;
interactive command modes acquire the single-instance lock *before*
first-run registry creation to avoid double-launch races; and original
Hyprland window-focus verification now tolerates a bounded compositor
state-propagation delay without repeating the dispatcher. New tests cover
argv selection caching, relative registry paths, group collisions, and
second-HUD rejection. [Green CI run 40953615508](https://github.com/sguzman/trievium/actions/runs/40953615508)
verifies the relative-path CLI and existing Wayland regressions;
[green run 40953760893](https://github.com/sguzman/trievium/actions/runs/40953760893)
covers the additional group validation. At the time of this checkpoint,
the newest concurrent-launch/focus checks were still running.
Progress remains capped at 35% because C3–C5 still require actual
interactive Hyprland and editor acceptance in the owner's desktop
session. No merge, tag, or release.

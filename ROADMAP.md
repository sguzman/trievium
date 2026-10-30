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

- [Latest green code CI `f18deb36`](https://github.com/sguzman/trievium/actions/runs/41131613328): **109 Rust tests, zero failures**, formatting/strict Clippy, CLI smoke, headless Wayland copy/commands/editor integration, optimized release build, checksum-verified package and uploaded artifact. Fixed an asynchronous popup geometry race: opening the native command editor requests a larger Hyprland window and Done returns it to picker dimensions, but independent resize workers previously could dispatch out of order and leave the picker at the editor's size. Resize workers now share a serialization gate and monotonic request generation, discarding superseded requests before dispatch. A deterministic concurrent-worker regression confirms that a delayed older request cannot override the latest geometry. This is automated evidence only; final visual/editor/Hyprland acceptance still requires an owner desktop session. M3 remains **35% verified**.
- [Prior green code CI `09373391`](https://github.com/sguzman/trievium/actions/runs/41118469547): **108 Rust tests, zero failures**, format/strict Clippy, CLI smoke, headless Wayland copy/commands/editor checks, optimized release build, checksum-verified archive and artifact upload. The command editor previously guarded only LF (`\\n`) in existing argv values; a carriage return (`\\r`) or CRLF could still be passed through the one-argument-per-line UI and potentially normalized when the user edits other arguments. Both CR and LF now trigger the lossless raw-JSON editing notice, preserving exact argv bytes while other editable command fields can still be changed. Added direct CR/LF/CRLF predicate regression and expanded the existing worker save/reload preservation test to include a CR-containing argument. The standalone command registry and original copy picker are unchanged. Hyprland 0.55+ interactive desktop acceptance remains outstanding; M3 stays **35%**.
- [Prior green code CI `61e89bb8`](https://github.com/sguzman/trievium/actions/runs/41110112368): **107 Rust tests, zero failures**, strict formatting/Clippy, CLI smoke, headless Wayland copy/commands/editor regressions, optimized release compilation, checksum-verified package and uploaded artifact. The native command popup's mapped-window safety check now always rejects an unexpectedly tiled window, even if a shortcut executes before the map/focus worker finishes. Previously an early action disarmed the worker before the tiled-window guard ran, potentially leaving an unrequested workspace reflow. A four-case regression tests floating/tiled mappings with and without an already-started action. The change does not establish real Hyprland 0.55+ popup/focus behavior; C3 live-session QA is still required, and M3 remains **35% verified**.
- [Prior green code CI `de3acecd`](https://github.com/sguzman/trievium/actions/runs/41107552743): **106 Rust tests, zero failures**, format/strict Clippy, CLI smoke, headless Wayland copy/commands/editor checks, optimized release build and checksum-verified package/artifact upload. Fixed two post-failure focus races: the command HUD now regains focus only if Hyprland confirms the originally focused window is still active, refusing to interrupt an unrelated app the user switched to; the startup focus worker also remains permanently disarmed after the first action, including failures, so late window-map setup cannot steal focus back. New pure JSON focus-identity and monotonic startup-guard regressions cover those decisions. Compositor observation remains non-atomic across separate `hyprctl` calls; live Hyprland fullscreen/workspace and interactive editor QA are **still outstanding**. M3 remains **35%**.
- [Prior green code CI `fa811fa6`](https://github.com/sguzman/trievium/actions/runs/41081643484): **104 Rust tests, zero failures**, formatting/strict Clippy, CLI smoke, headless Wayland copy/command/editor regressions, optimized release, checksum-verified package and artifact upload. Fixed a loss-of-data issue in the command editor's one-argument-per-line representation: selecting an existing command now retains intentionally empty trailing argv entries (including a single empty arg) through a structural final newline, with round-trip regressions. An argv element containing an embedded newline cannot be safely edited as separate lines, so the UI preserves it and displays a warning instead of exposing a destructive text editor; a save/reload regression verifies that editing the label does not mutate those arguments. This does **not** replace live Hyprland/editor visual acceptance; M3 remains **35%**.
- [Prior green code CI `7de6a73e`](https://github.com/sguzman/trievium/actions/runs/41079281636): **102 Rust tests, zero failures**, formatting/strict Clippy, CLI smoke, headless Wayland copy/command integration, optimized release and checksum-verified package/artifact. Before executing a `focus_original: true` Hyprland action, Trievium now verifies that the original mapped non-popup client still belongs to the active workspace. If it moved, or workspace metadata is unavailable, the dispatcher refuses to refocus it, preventing an unrequested workspace jump. New pure JSON/identity regression tests cover moved windows, malformed workspace data and special-workspace IDs; this remains **automated evidence only**, not proof of live Hyprland workspace/focus behavior. C3 owner-session QA and M3 **35%** acceptance remain unchanged.
- [Prior green code CI `1af7fe06`](https://github.com/sguzman/trievium/actions/runs/41076938249): **101 Rust tests, zero failures**, formatting/strict Clippy, CLI smoke, headless Wayland copy/commands regressions, optimized release build, checksum validation and artifact upload. The native command editor now ignores the non-semantic final newline in its one-argument-per-line field, avoiding an accidental empty trailing argv element. Intentionally blank interior lines remain explicit empty arguments, with an isolated regression covering empty text, trailing newline and interior blank lines. No changes were made to the command registry format, original copy picker, or `items.json`. Real-session Hyprland/editor acceptance remains outstanding; M3 stays **35%**.
- [Prior green code CI `bc72fd0e`](https://github.com/sguzman/trievium/actions/runs/41001010048): **100 Rust tests, zero failures**, formatting/strict Clippy, command CLI smoke, headless Wayland integration, optimized release build and checksum-verified package/artifact upload. The command HUD now checks original `egui::Event::Text` and `egui::Event::Ime(Commit)` as well as clipboard paste before resolving shortcuts. The new regression protects ASCII multi-key entry while rejecting control characters, transformed input-method text and oversized IME commits that could become an executable prefix after field truncation. Physical input-method delivery, native clipboard transport and real Hyprland fullscreen/workspace/editor acceptance remain **unverified**; M3 stays **35%**.
- [Prior green code CI `31fd9dc9`](https://github.com/sguzman/trievium/actions/runs/40997905270): **99 Rust tests, zero failures**, formatting/strict Clippy, command CLI smoke, headless Wayland copy/command/editor regression, optimized release and verified package checksum/artifact upload. Conflicting command-mode verbs now fail before opening/creating a registry (including `validate ... init`), with both Rust and executable smoke coverage. The command HUD now rejects raw paste events containing unsupported characters, text transformed by single-line TextEdit, and paste text truncated by the input limit; a tested overflow slot covers the longest ordinarily spaced sequence. These checks do **not** verify physical OS clipboard delivery or Hyprland fullscreen/workspace/editor acceptance; M3 remains **35% verified**.
- [Prior green code CI `320984cf`](https://github.com/sguzman/trievium/actions/runs/40997307055): **97 Rust tests**, CLI-mode ambiguity rejected and tested, strict Clippy, headless Wayland, optimized release and verified package.
- [Latest green code CI `dfca7d95`](https://github.com/sguzman/trievium/actions/runs/40996442634): **96 Rust tests, zero failures**, strict formatting/Clippy, CLI smoke, headless Wayland integration, optimized release compilation, checksum-verified package and artifact upload. The native command editor now explicitly schedules a first worker-result poll after Save, Done, or Discard launches asynchronous persistence/reload; this covers the final-input-event case without adding disk I/O to egui's render thread. Existing native editor save/reload/Done tests passed. No changes to the original copy HUD or either registry format. Real Hyprland/editor acceptance remains required.
- [Prior green code CI `627cae30`](https://github.com/sguzman/trievium/actions/runs/40992585707): **96 Rust tests, zero failures**, strict formatting/Clippy, CLI smoke, headless Wayland copy/command/editor regressions, optimized release build, checksum-verified artifact and upload. After a failed command, starting a new action via click or navigating a branch clears the stale error label, matching keyboard entry; the new clicked-retry regression passes. Original copy UI and isolated registries were not changed. Real Hyprland/editor visual acceptance is still required.
- [Prior green CI `57e38220`](https://github.com/sguzman/trievium/actions/runs/40990190134): **95 Rust tests, zero failures**, strict Clippy, CLI smoke, headless Wayland (including new failed-executable recovery), optimized release build, checksum-verified package and artifact upload. The isolated command HUD stayed open after a nonexistent program failed to spawn, then executed a valid shortcut in the same session; original `items.json` and clipboard remained unchanged. This verifies headless error recovery, **not** live Hyprland fullscreen/workspace/editor acceptance.
- [Prior green code CI `a201d0b8`](https://github.com/sguzman/trievium/actions/runs/40988664227): **95 Rust tests, zero failures**, strict Clippy, CLI smoke, headless Wayland integration, optimized release binary, checksum verification, artifact upload. Selecting a command by either typing or clicking now explicitly schedules egui's first delayed repaint after spawning its worker; pending result polling no longer relies on unrelated later input. The speculative callback-delay test was removed after it proved incompatible with egui repaint coalescing; the source at `f8d78d77` matches green `a201d0b8` exactly. Real Hyprland QA remains required.
- [Prior green implementation CI `d2de334b`](https://github.com/sguzman/trievium/actions/runs/40986889610): **95 Rust tests, zero failures**, strict formatting/Clippy, CLI smoke, headless Wayland integration, optimized release build, checksum-verified artifact and upload. Fixed command HUD TextEdit raw character capacity: a valid 12-key sequence with one space between each key requires 23 UI characters, while the registry's `MAX_SEQUENCE = 12` validates normalized key count. The egui paste-event regression now asserts the full 23-character sequence reaches TextEdit and resolves, without weakening exact matching. Native OS clipboard transport and real Hyprland acceptance remain unverified; no change to the original copy HUD.
- [Green egui event-level regression CI `8861d39b`](https://github.com/sguzman/trievium/actions/runs/40984654999): **95 Rust tests, zero failures**, strict Clippy, CLI smoke, headless Wayland copy/command checks, optimized release compilation and checksum package. A new focused `egui::Event::Paste`/TextEdit test confirms that the widget delivers invalid pasted `zwf` to the strict command index without executing `wf`, and that a valid `wf` paste resolves correctly. This is an in-process egui event test, **not** proof that Wayland/Hyprland OS clipboard paste reaches the HUD; the OS-level QA boundary below remains open. Production code and original copy UI were unchanged.
- [Prior green implementation CI `b18d4a6e`](https://github.com/sguzman/trievium/actions/runs/40980733826): **94 Rust tests, zero failures**, strict formatting/Clippy, command and copy Wayland E2E, CLI smoke, optimized release and checksum-verified artifact. The complete command field must normalize to the selected terminal before execution, so pasted extra characters **before, inside or after** a shortcut cannot run it; incremental invalid keystrokes are still ignored with prefix retention. Implementation `57662026` through `b18d4a6e`, including refreshed tests for deliberate paste-versus-keystroke semantics. Real desktop acceptance still required.
- [Prior green implementation CI `2692cd10`](https://github.com/sguzman/trievium/actions/runs/40979341694): **93 Rust tests, zero failures**, strict Clippy, CLI smoke, headless Wayland E2E, optimized release binary, checksum-verified package, and uploaded artifact. Command resolution now refuses to execute when a pasted text event contains trailing characters after the first complete shortcut (for example, `wf extra`). The reference reducer and compiled index agree on the result; ordinary incremental entry remains exact. Implementation commits `3156ba40` and `2692cd10`. Documentation in [docs/COMMANDS.md](docs/COMMANDS.md). Real-session Hyprland QA remains the acceptance gate.
- [Prior green implementation CI `5a0b353a`](https://github.com/sguzman/trievium/actions/runs/40978361353): **92 Rust tests, zero failures**, strict Clippy, CLI smoke, headless Wayland, optimized release build, and checksum package. Command-mode `--help`/`-h` and `--version`/`-V` are now available without initializing `commands.json` or touching the original copy registry. Test coverage includes all four flags and both files remaining absent. Implementation commits `df3752ad` and `5a0b353a`; usage is documented in [docs/COMMANDS.md](docs/COMMANDS.md). Real Hyprland acceptance remains pending.
- [Prior green implementation CI `c3d62488`](https://github.com/sguzman/trievium/actions/runs/40977301318): **92 Rust tests, zero failures**, strict Clippy, CLI smoke, headless Wayland regression, optimized release build and checksum package. The new dispatcher-response regression rejects error text even when the command exits successfully. Internal popup focus/geometry and user Hyprland actions now share the same explicit `ok` check. This does not verify live Hyprland behavior.
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
- **Clipboard paste QA boundary (2026-10-29):** a nested Sway CI
  positive control could not demonstrate Ctrl+V text delivery, even after
  temporarily enabling eframe's optional OS clipboard support. Do not count
  the earlier negative paste-only smoke check as evidence: it could pass
  without delivering text. The experimental test and dependency changes were
  reverted; the known-green 94-test reducer suite still protects exact
  command matching. Native clipboard paste requires actual desktop QA before
  any support or regression claim.
- **Local blocker:** use the bounded [real Hyprland QA checklist](docs/COMMANDS.md)
  for reversible fullscreen on the original window, workspace switching,
  interactive command/editor CRUD, and the accepted copy picker. Headless
  Sway automation cannot establish actual Hyprland focus/float behavior.
- **Release policy:** keep [draft PR #1](https://github.com/sguzman/trievium/pull/1)
  unmerged; no tag or release until all remaining gates pass.

Historical round-by-round detail is retained in the branch's Git history;
this roadmap deliberately retains only current weighted acceptance and
durable verification links.

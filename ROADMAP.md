# Trievium — closed MVP roadmap

All required MVP features were accepted on 2026-10-22. After v1.0.0 the roadmap is a **maintenance ledger**, not an expanding feature wishlist.

## M0 — MVP (100% accepted scope)

| Phase | Deliverable | Status | Evidence |
| --- | --- | --- | --- |
| ✅ P1 · Engine | Prefix-free trie, pinned and remembered shortcuts, shortest-unique selection | Accepted | Rust trie/input tests |
| ✅ P2 · Picker | One-shot native UI, exact Wayland clipboard handoff, close-after-copy | Accepted | Wayland integration tests and dogfooding |
| ✅ P3 · Editor | Library CRUD, atomic saves, validation, full-height list | Accepted | Editor tests and owner visual check |
| ✅ P4 · Desktop | Non-disruptive Hyprland floating popup; system-aware light/dark | Accepted | Owner Hyprland session and QA |
| ✅ P5 · Release readiness | Frozen contract, changelog, acceptance evidence, CI-gated package and checksum | Implemented; publication gated on green v1 CI | [MVP acceptance](docs/MVP_ACCEPTANCE.md) and [release policy](docs/RELEASE.md) |

## M1 — Maintenance (continuous)

- 🐛 Investigate and correct reproducible bugs.
- 🧪 Preserve selection, data and clipboard regressions tests.
- 🔧 Maintain compatible Linux, Hyprland and Rust dependency support.
- 📝 Keep patch-version release notes, verified assets and documentation accurate.

A perpetual maintenance queue has no completion percentage. New capabilities are not scheduled.

## Evidence rule

Acceptance must be backed by actual behavior and tests. A subsequently discovered purpose-defeating regression is a bug to resolve, not something overridden by a prior 100% indicator.

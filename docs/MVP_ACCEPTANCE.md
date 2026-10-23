# Trievium v1.0.0 — MVP acceptance

**Accepted:** 2026-10-22.  
**Scope:** complete for daily use; feature additions are frozen after graduation.

## Exercised workflows

- Invoke the picker in Hyprland without disturbing existing tiled windows.
- Select by shortest unambiguous prefix; copy text to the Wayland clipboard and exit without Enter.
- Open native entry management, edit and persist entries, and navigate the full-height library.
- Use native system light and dark appearances.

These were accepted during real owner dogfooding. They do not imply that automation was run inside the owner's desktop session.

## Automated evidence

The latest pre-graduation `main` revision `fb4652c7064d3319c1aab8ba39229a615f71f594` passed [Rust CI run 39871048634](https://github.com/sguzman/trievium/actions/runs/39871048634), including:

- Rust formatting, unit tests, and rendered-height regression of the library viewport.
- Strict Clippy and CLI smoke verification.
- Nested Sway real keyboard/clipboard E2E and lifecycle tests.
- An optimized Linux build and packaged executable.

**Graduation gate:** the exact v1.0.0 release commit must independently pass these checks before its annotated tag and downloadable GitHub Release are published. A green pre-release commit does not substitute for a green release commit. The tagged commit's GitHub Actions record is the definitive final test evidence.

## Stable contract and limits

Local plaintext selection, prefix-free assignment, native editing, Wayland clipboard, system appearance and Hyprland float-on-map make up the MVP. The library stays at JSON schema `version: 1`.

The app does not provide global typing, importers, a tray service, generalized actions, network integration, or cross-platform installation. No cold-start timing or universal Wayland compositor compatibility guarantee is claimed. Future work is defect repair and maintenance, not feature expansion.

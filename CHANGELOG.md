# Changelog

All notable Trievium changes are recorded here. Versions follow Semantic Versioning.

## [1.1.0] - 2026-10-27

- Native system-font fallback with full-color emoji rendering where a color emoji font is installed (egui 0.37).
- Stable, gamut-mapped OKLCH automatic accent swatches displayed before shortcuts; optional editable hex overrides.
- Manual Up/Down reordering in the entry manager. Picker follows the saved order while pinned and remembered shortcuts retain their identity.
- JSON schema remains `version: 1`; legacy entries use automatic colors without migration. System appearance continues following the portal.
- Startup latency repair: postpone optional fonts and color-emoji fallback until after first paint, without blocking the UI on font enumeration; skip redundant shortcut-cache fsyncs on ordinary launches.
- CI headless Sway benchmark: 98.9 ms to 59.8 ms median mapped-window time (seven warm rounds each, 39.5% faster); actual Hyprland performance may differ.

## [1.0.0] - 2026-10-22

First accepted, stable MVP release. No data migration is required from 0.4.0.

### Included
- Native Linux/Wayland Rust and egui prefix-tree picker, immediate selection at the shortest unique prefix.
- Prefix-free 1–4 letter pinned shortcuts and mnemonic automatic assignments with a local label/key cache.
- Exact-text clipboard handoff to `wl-copy` and immediate picker exit.
- Native CRUD editor, persistent library, atomic save, collision validation, and external-edit protection.
- Hyprland float-on-map behavior that does not reflow other tiled windows.
- Desktop-following light/dark appearance and theme overrides; Luciole/Lexica preference with fallbacks.
- Scrollable full-height library, compact picker, diagnostics and CLI validation.
- Rust tests, strict lint, real keyboard/clipboard Wayland E2E, and verified optimized packaging.

### Compatibility
- On-disk `items.json` continues to use schema `version: 1`, independently of application v1.0.0.
- Pinned shortcuts remain authoritative. Automatically assigned codes are best-effort stable when the library changes.
- No daemon, auto-typing, importer or Lexwright integration is introduced.

See [release policy](docs/RELEASE.md) and [acceptance evidence](docs/MVP_ACCEPTANCE.md).

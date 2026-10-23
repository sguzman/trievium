# Stable release and compatibility policy

## Lifecycle

Trievium's owner accepted the v1.0.0 MVP and closed the feature set. After graduation, changes are for defects, security/reliability, dependency compatibility, and documentation; no new features are scheduled.

A stable version must have matching `Cargo.toml`, `Cargo.lock`, `trievium --version`, an annotated `vX.Y.Z` tag, a GitHub Release, and a release archive with SHA-256 checksum. A source version bump alone does not complete inauguration.

## Compatibility contract

- **Platform:** native Linux Wayland client; Hyprland's documented Lua dispatcher API enables an explicitly floating popup.
- **CLI:** default picker plus `pick`, `manage`, `init`, `validate`, `config-path`, `doctor`; existing configuration/theme/diagnostic flags.
- **User data:** JSON library schema `version: 1` is unrelated to app SemVer. A compatible update does not silently erase, overwrite or reinterpret entries. Stored content is plaintext, not password storage.
- **Selection:** pinned keys are prefix-free and must never silently move. Shortest-unique-prefix behavior remains immediate; auto-assigned shortcuts can change only when existing capacity/keys require it.
- **Clipboard:** exact bytes are handed to `wl-copy` and remain pasteable after Trievium exits; no automatic typing into another app.
- **Appearance:** system theme and explicit light/dark overrides; optional fonts have fallbacks.

SemVer PATCH means compatible fixes and maintenance. MINOR backward-compatible features would require the owner to reopen the frozen scope; breaking changes require MAJOR and explicit migration notes.

## Inauguration and subsequent releases

1. Record owner acceptance, QA evidence and scope in [MVP_ACCEPTANCE.md](MVP_ACCEPTANCE.md), [ROADMAP.md](../ROADMAP.md), changelog and versioned release notes.
2. Update `Cargo.toml` and `Cargo.lock` together and commit directly to `main` with a deliberate Conventional Commit message starting `chore(release):`.
3. GitHub Actions must pass on **that commit**: frozen-lockfile validation, formatting, Rust tests, Clippy, CLI smoke tests, nested Wayland keyboard/clipboard E2E, and optimized Linux packaging.
4. Only after green CI, the release job creates the immutable **annotated tag** for the tested commit, verifies `SHA256SUMS`, and publishes a GitHub Release with the Linux x86_64 archive, checksum and matching notes. Release reruns must not silently retarget an existing tag.
5. Verify the tag, GitHub Release, version, artifact and SHA256 checksum all agree. If publishing fails, release remains **incomplete** until corrected.

GitHub Actions and `GITHUB_TOKEN` provide zero-cost publishing on the existing repository; no user secrets, proprietary binaries or local data are committed. After MVP, commits use clear Conventional Commit prefixes (`fix:`, `test:`, `docs:`, `chore:`), and `main` remains the default branch.

## Install or upgrade

Download `trievium-linux-x86_64.tar.gz` and `SHA256SUMS` from [GitHub Releases](https://github.com/sguzman/trievium/releases), put them in one directory, then run `sha256sum -c SHA256SUMS`. Extract the archive and point the Hyprland binding to its `trievium/trievium` binary, or rebuild the existing checkout with `cargo build --release --locked`. `wl-clipboard` is required for copying. The release archive does not install a package or modify existing `items.json` data.

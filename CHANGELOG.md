# Changelog

All notable changes to Bezem are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). Before 1.0, a minor version may
change the CLI, the JSON output or the cleaner definition schema; such changes are called out here.

Besides the standard sections (Added, Changed, Deprecated, Removed, Fixed, Security), Bezem uses a
**Cleaners** section that lists every cleaner definition added or removed, or whose roots or risk
level changed.

## [Unreleased]

### Added

- `bezem-core`: validated path types (`WidePath`, `PathComponent`) for the safety layer: extended-length
  `\\?\X:\` paths built by component push, component-wise case-insensitive containment.
- Cargo workspace skeleton with the `bezem-core`, `bezem-cleaners`, `bezem-cli` and `bezem-gui`
  crates. `bezem --version` is the only working command.
- Continuous integration on GitHub Actions (formatting, clippy, tests, dependency checks).
- `cargo deny` policy: MIT-compatible licenses only, RustSec advisories, and a ban on network crates.
- Pinned Rust toolchain (`rust-toolchain.toml`, 1.99.1) and a minimum supported Rust version of 1.85.
- Dependabot configuration for Cargo dependencies, the Rust toolchain and GitHub Actions.
- `CODEOWNERS`, and repository labels in `.github/labels.yml` with a workflow that syncs them.
- Governance documents: `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md` (Contributor
  Covenant 2.1) and this changelog.
- Issue forms (bug, feature, new cleaner) and a pull request template.

### Changed

- Rewrote `README.md` to describe the project's real status (pre-alpha, nothing to run yet) and to
  remove features that are out of scope, such as prefetch clearing.

[Unreleased]: https://github.com/ReesMehorst/Bezem/commits/main

# Bezem

[![CI](https://github.com/ReesMehorst/Bezem/actions/workflows/ci.yml/badge.svg)](https://github.com/ReesMehorst/Bezem/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Bezem** (Dutch for *Broom*) is a planned lightweight, open-source Windows tool that removes
temporary data: temp files, caches, crash dumps and, if you opt in, privacy traces such as browser
history. It shows you exactly what it will remove before it removes anything.

> **Status: pre-alpha. There is nothing to run yet.**
> The repository holds an empty Rust workspace (milestone M0). The only working command is
> `bezem --version`. Nothing has been released. See the [roadmap](docs/PLAN.md#8-roadmap).

## Principles

These are non-negotiable ([PLAN.md §1](docs/PLAN.md#1-principles-non-negotiable)):

- **Safe by default**: a preview always comes first. Bezem only deletes inside known, allowlisted
  folders, never follows symbolic links or junctions, and skips files that are in use.
- **Transparent**: every cleaner is a readable TOML file, and every deletion is written to a local
  log.
- **Zero overhead**: a single portable executable. No background service, tray icon or auto-start.
- **Zero telemetry**: no network calls.
- **Honest**: no registry cleaning, no "RAM boosters", no "issues found" counters, no scare tactics.

## What it will do

The planned 1.0 scope is in [PLAN.md §4](docs/PLAN.md#4-cleaner-catalog) and, in detail, in
[FEATURES.md](docs/FEATURES.md):

- **System**: user and Windows temp folders, thumbnail cache, error reports, crash dumps,
  Windows Update and Delivery Optimization caches, the Recycle Bin.
- **Browsers**: caches for Chrome, Edge, Brave, Vivaldi, Opera and Firefox, profile-aware. Cookies,
  history and other privacy data only if you opt in.
- **Privacy** (opt-in): recent documents, jump lists, Explorer and Run history, clipboard, DNS cache.

Bezem will **not** clean the registry, clear prefetch data, "optimize" memory or run in the
background. See [what is out of scope and why](docs/FEATURES.md#14-not-doing-and-why).

It will ship first as a command-line tool (`bezem.exe`), then as a desktop app (`bezem-gui.exe`).

## Building from source

You need [Rust](https://rustup.rs) (the toolchain in `rust-toolchain.toml` is selected
automatically). On Windows you also need the Visual Studio Build Tools for the MSVC linker.

```sh
git clone https://github.com/ReesMehorst/Bezem.git
cd Bezem
cargo build
cargo run -p bezem-cli -- --version
```

## Documentation

| Document | Contents |
|---|---|
| [PLAN.md](docs/PLAN.md) | Project plan, principles, roadmap |
| [FEATURES.md](docs/FEATURES.md) | Feature specification and acceptance criteria (source of truth) |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | Technical design, cleaner schema, safety algorithm |
| [DESIGN.md](docs/DESIGN.md) | User interface, wording, command-line output |
| [QUALITY_AND_RELEASE.md](docs/QUALITY_AND_RELEASE.md) | Testing, CI, release process, governance |
| [CHANGELOG.md](CHANGELOG.md) | Changes per version |

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first, and follow the
[Code of Conduct](CODE_OF_CONDUCT.md).

To report a security problem, or a file that Bezem deleted when it should not have, **do not open a
public issue**. Follow [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE)

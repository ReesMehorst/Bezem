# Bezem — Project Plan

> Bezem (*Dutch for Broom*) is a lightweight, fast, open-source system optimization and privacy tool that sweeps redundant temporary data from Windows: safely, transparently, and with zero overhead.

## 1. Principles (non-negotiable)

These rules shape every decision below. A feature that breaks one doesn't ship.

| Principle | What it means in practice |
|---|---|
| **Safe by default** | Preview (dry run) before deleting. Never follow symlinks/junctions. Skip locked or in-use files. Only delete inside known, allowlisted roots. |
| **Transparent** | Every cleaner is a readable definition file. The UI shows exactly which paths will be removed and how much space each frees. Local log of every deletion. |
| **Zero overhead** | No background service, no tray icon, no auto-start, no scheduled scans unless the user creates one. Single portable executable. |
| **Zero telemetry** | No network calls at all. An optional update check is off by default and is the only network code. |
| **Honest** | No registry "cleaning", no "RAM boosters", no fake "issues found" counters, no scare tactics. |

## 2. Technology choice

**Recommendation: Rust.**

- Compiles to a single native `.exe` (~2–5 MB) with no runtime to install (unlike .NET or Electron).
- Memory safety matters for a tool that deletes files with elevated rights.
- Strong Windows support through the `windows` crate (Win32 APIs for the Recycle Bin, known folders, reparse points, restart manager, services).
- Fast parallel directory walking (`rayon` with a small handle-relative walker; see ARCHITECTURE.md).

**Workspace layout**

```
bezem/
├── crates/
│   ├── bezem-core/      # scanning, safety checks, deletion engine (no UI)
│   ├── bezem-cleaners/  # built-in cleaner definitions + loader/validator
│   ├── bezem-cli/       # `bezem.exe` command line
│   └── bezem-gui/       # desktop UI (egui/eframe; pure Rust, small binary)
├── cleaners/            # *.toml cleaner definitions (embedded at build time)
├── docs/
└── .github/workflows/
```

The core is a library with no UI dependencies, so the CLI and GUI share the same scanning and deletion code. That makes it testable and keeps both front-ends honest.

## 3. Architecture

### 3.1 Declarative cleaners

Each cleaner is a TOML file. Users and contributors can read exactly what it does, and nothing is hidden in code:

```toml
id          = "windows.user_temp"
name        = "User temporary files"
description = "Files apps leave in %TEMP%."
category    = "system"        # system | browser | privacy | apps
risk        = "low"           # low | medium | high (high = opt-in only)
requires_admin = false
default_enabled = true

[[target]]
path       = "%TEMP%"
recursive  = true
min_age_hours = 24            # skip files that are in active use
exclude    = ["*.lock"]
```

Special actions that aren't plain file deletes (emptying the Recycle Bin, flushing DNS, stopping a service around a cleanup) are a small, closed set of named built-in actions. Definitions can reference them, but they can't run arbitrary commands.

### 3.2 Pipeline

```
Load definitions → Validate → Scan (parallel, read-only) → Preview report
      → user confirms → Delete (with safety checks per file) → Summary + log
```

- **Scan** is read-only and always runs first. It produces a report: per cleaner, file count, bytes, sample paths.
- **Delete** re-checks every file at deletion time. It doesn't trust the scan, because the file system may have changed in between.

### 3.3 Safety layer (in `bezem-core`)

1. **Root allowlist**: every resolved target must live under a known folder (resolved through `SHGetKnownFolderPath`, not string guessing). A path that is empty, a drive root, `C:\Windows`, `C:\Windows\System32`, `Program Files` or the user profile root is rejected outright.
2. **No reparse points**: never traverse symlinks, junctions or mount points (`FILE_ATTRIBUTE_REPARSE_POINT`). This blocks the classic "junction in %TEMP% points to C:\" attack.
3. **Locked files**: on a sharing violation, skip and report the file. Never force it.
4. **Age threshold**: by default, skip files modified in the last 24 hours in temp folders.
5. **App-running check**: a browser cleaner refuses to run while that browser is open (and offers to wait).
6. **Delete modes**: permanent delete (default for caches) or move to the Recycle Bin (selectable, and the default for privacy items such as history).
7. **Least privilege**: runs unelevated. Cleaners that need admin rights are grouped, and the app relaunches elevated only for those, with a clear prompt.
8. **Deletion log**: a JSON Lines log in `%LOCALAPPDATA%\Bezem\logs` listing every removed path. It is local and never uploaded.

## 4. Cleaner catalog

### v1.0: System (low risk, on by default)
- User temp (`%TEMP%`)
- Windows temp (`C:\Windows\Temp`, admin)
- Thumbnail cache (`thumbcache_*.db`)
- Windows Error Reporting archives and queues
- Crash dumps (`%LOCALAPPDATA%\CrashDumps`, `C:\Windows\Minidump`, admin)
- Delivery Optimization cache (admin, through the service API)
- Windows Update download cache (`SoftwareDistribution\Download`, admin; stops and restarts `wuauserv`)
- Recycle Bin (`SHEmptyRecycleBin`)
- DirectX shader cache, font cache (medium risk)

### v1.0: Browsers (caches on by default; privacy data opt-in)
- Chromium family: Chrome, Edge, Brave, Vivaldi, Opera (multi-profile aware)
- Firefox (multi-profile, reads `profiles.ini`)
- Cache and code cache: on by default
- Cookies, history, session data, saved form data: **opt-in** (Firefox history is post-1.0: it shares `places.sqlite` with bookmarks), with a clear warning ("you will be logged out of websites")

### v1.0: Privacy (opt-in)
- Recent documents and jump lists
- Explorer typed paths and run history (MRU)
- Clipboard
- DNS cache flush

### Later (v1.x)
- App caches: Discord, Teams, Spotify, Slack, VS Code, npm/pip/NuGet caches (developer category)
- Old Windows installation (`Windows.old`), through the supported `cleanmgr`/DISM path only
- Community cleaner definitions (opt-in import with a validator and diff view)

### Explicitly out of scope
- **Registry cleaning**: risky, with no measurable benefit.
- **Prefetch/Superfetch clearing**: makes Windows *slower*.
- **"Memory optimizers"**, driver updaters, "PC health scores".
- Anything that runs in the background without being asked.

## 5. User interfaces

### CLI (`bezem.exe`), ships first
```
bezem list                          # show all cleaners with risk and status
bezem scan                          # dry run, prints a table of what would be freed
bezem scan --json                   # machine-readable output for scripts
bezem clean --only system,browser   # clean selected categories (asks for confirmation)
bezem clean --yes --recycle         # non-interactive; send to Recycle Bin
bezem show windows.user_temp        # print a cleaner's definition
```
Exit codes are documented, so the CLI works in scripts and Task Scheduler.

### GUI (`bezem-gui.exe`)
- One window: a checklist of cleaners grouped by category, each with a risk badge and an expandable "what does this remove?" panel showing the real paths.
- **Scan** → results with sizes → **Clean** button (disabled until a scan has run).
- Settings: delete mode, age threshold, exclusions, profiles of saved selections.
- Light and dark theme that follows Windows. Fully keyboard-accessible.
- No splash screen, no ads, no upsell.

## 6. Quality and testing

- **Unit tests**: the safety layer gets the most attention: path validation, reparse-point rejection, allowlist escapes (`..`, 8.3 short names, `\\?\` prefixes, UNC paths, environment variables that expand to empty).
- **Integration tests** on a GitHub Actions `windows-latest` runner: build a fake tree in a temp folder (including junctions pointing outside, locked files, read-only files) and assert that only the expected files disappear.
- **Definition validator**: CI rejects any cleaner TOML whose roots fall outside the allowlist.
- **Fuzzing** (`cargo-fuzz`) on path normalization and definition parsing.
- **Lints**: `cargo clippy -D warnings`, `cargo fmt --check`, `cargo deny` (licenses and advisories), `#![forbid(unsafe_code)]` in all crates except `bezem-core`, which uses `deny` and allows `unsafe` only in a small, reviewed `win32` module.
- **Benchmark**: a scan of a typical profile should finish in under 2 seconds. Track this in CI to catch regressions.

## 7. Distribution and trust

- Portable `.zip` plus an optional MSI/`winget` and `scoop` manifests.
- Code-signed binaries (SignPath.io offers free signing for open-source projects) to avoid SmartScreen warnings.
- Reproducible release builds from tagged commits on GitHub Actions, with SHA-256 checksums and build provenance (SLSA attestation).
- `SECURITY.md` with a private disclosure channel. A deletion bug is treated as a security bug.

## 8. Roadmap

| Milestone | Scope | Done when |
|---|---|---|
| **M0: Foundation** (week 1) | Cargo workspace, CI (fmt, clippy, test on Windows), license headers, CONTRIBUTING, SECURITY, issue templates | Green CI on an empty skeleton |
| **M1: Safe core** (weeks 2–3) | Known-folder resolution, path allowlist, reparse-point guard, scanner, deletion engine with re-checks, logging | Safety test suite passes, including junction-escape tests |
| **M2: Cleaner engine** (week 4) | TOML schema, loader, validator, first 5 system cleaners | `bezem scan` reports real numbers on a Windows machine |
| **M3: CLI 0.1** (week 5) | `list`/`scan`/`clean`/`show`, JSON output, confirmations, Recycle Bin mode | First public pre-release |
| **M4: Browsers and privacy** (weeks 6–7) | Chromium and Firefox profile discovery, running-app check, privacy cleaners (opt-in) | Cleans all supported browsers without logging users out unless they opted in |
| **M5: Elevation** (week 8) | Admin-only cleaners, elevated relaunch, service stop/start (Windows Update cache) | Admin cleaners work, with no elevation for the rest |
| **M6: GUI 0.5** (weeks 9–11) | egui app on top of `bezem-core`, preview panel, settings, themes | Feature parity with the CLI |
| **M7: 1.0 release** (week 12) | Code signing, winget/scoop, docs site, translations (EN and NL first) | Signed 1.0 on GitHub Releases and winget |
| **Post-1.0** | Developer/app cleaners, community definitions, saved profiles, optional Task Scheduler integration (user-created, never automatic) | Driven by issues |

## 9. Detailed plans

| Document | Contents |
|---|---|
| [FEATURES.md](FEATURES.md) | Feature specification: IDs, acceptance criteria, priorities, exact paths, CLI reference and exit codes |
| [DESIGN.md](DESIGN.md) | UI/UX and visual design: brand, tokens, wireframes, flows, microcopy, accessibility, CLI output |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Technical design: crate APIs, TOML schema, path resolution, safety algorithm, elevation, concurrency |
| [QUALITY_AND_RELEASE.md](QUALITY_AND_RELEASE.md) | Test strategy, CI, release process, governance, risk register |

## 10. Open decisions

1. **GUI toolkit**: egui (recommended: smallest, pure Rust) or Slint (more native look, declarative UI files). Decide before M6.
2. **Minimum Windows version**: Windows 10 22H2 and Windows 11 are recommended. Older versions are out of support from Microsoft anyway.
3. **License**: the project is MIT. Allow only MIT-compatible dependencies (enforced by `cargo deny`).
4. **Update check**: ship none in 1.0 (rely on winget/scoop), or ship an opt-in check against the GitHub Releases API.

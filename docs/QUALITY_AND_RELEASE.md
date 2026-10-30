# Bezem: Quality, Release and Governance

> Companion to [`PLAN.md`](PLAN.md). PLAN.md is the master plan; this document expands §6 (Quality and testing), §7 (Distribution and trust) and the M0/M7 milestones into concrete practice. If the two disagree, PLAN.md wins and this file gets fixed.

**The one rule behind everything here:** Bezem deletes files. A bug that deletes the wrong file is the worst thing this project can do. Tests, CI gates, review rules and the release process are all built around preventing it, and around noticing fast if it happens anyway.

---

## 1. Test strategy

### 1.1 Test pyramid at a glance

| Layer | Tool | Runs on | When | Blocks merge? |
|---|---|---|---|---|
| Unit | `cargo test` / `cargo nextest` | Windows + Linux (pure logic) | Every PR | Yes |
| Integration (fake trees) | `cargo nextest`, `tests/` in `bezem-core` + `bezem-cli` | Windows runners only | Every PR | Yes |
| Property tests | `proptest` | Windows + Linux | Every PR (256 cases); nightly (10 000 cases) | Yes / opens issue |
| Fuzzing | `cargo-fuzz` (libFuzzer) | Linux (nightly toolchain) | 5 min per target on PR touching parsers; 1 h nightly | No (PR smoke yes) |
| Definition validator | `bezem-cleaners` validator binary | Windows | Every PR | Yes |
| Benchmarks | `criterion` + `hyperfine` | Windows | Every PR (compare), nightly (trend) | Yes, on budget breach |
| Manual QA | Checklist (§1.7) | Real Windows 10/11 VMs | Every release candidate | Blocks release |

**Coverage target:** `bezem-core::safety` and `bezem-core::delete` at **≥ 95 % line coverage** (`cargo llvm-cov`), the rest of the workspace at ≥ 75 %. Coverage is reported on PRs; a drop in `safety`/`delete` blocks merge.

### 1.2 Unit tests

Focus on the safety layer (PLAN §3.3). Make path logic **pure functions over the `WidePath` / `PathComponent` types** (`bezem-core::path`) wherever possible, so it can be unit-, property- and fuzz-tested on Linux too; only the thin Win32 shim needs Windows.

Mandatory unit-test cases for the path validator / allowlist:

- [ ] Empty string, whitespace-only, `.`; env var that expands to empty (`%BEZEM_UNSET%\foo` → `\foo`) is rejected
- [ ] Drive roots: `C:`, `C:\`, `C:/`, `\\?\C:\`, `\\.\C:\`
- [ ] Forbidden roots and their spellings: `C:\Windows`, `c:\WINDOWS\`, `C:\Windows\System32`, `C:\Program Files`, `C:\Program Files (x86)`, the user profile root
- [ ] `..` traversal: `%TEMP%\..\..`, `%TEMP%\a\..\..\Windows`, mixed `/` and `\`
- [ ] 8.3 short names: `C:\PROGRA~1`, `C:\WINDOW~1` resolve to forbidden roots
- [ ] Prefixes: `\\?\`, `\\?\UNC\`, `\\.\`, `\??\` (NT namespace), device paths (`\\.\PhysicalDrive0`, `CON`, `NUL`, `COM1`)
- [ ] UNC and mapped network paths: rejected unless explicitly allowlisted (they are not in v1.0)
- [ ] Trailing dots and spaces (`C:\Windows.`, `C:\Windows `), which Win32 strips
- [ ] Alternate data streams: `file.txt:stream`, `file.txt::$DATA`
- [ ] Case folding including non-ASCII (Turkish İ, German ß) via `CompareStringOrdinal(ignoreCase)` semantics
- [ ] Glob/exclude matching: `*.lock`, `**`, patterns that try to escape (`..\*`)
- [ ] Age threshold boundaries (exactly 24 h, clock skew, future mtime)

Every bug fix in `safety` or `delete` **must** add a regression test named `regress_<issue-number>_<summary>`.

### 1.3 Integration tests (Windows runner, fake trees)

A test fixture builder (`bezem-core/tests/support/tree.rs`) creates a throwaway tree under `%RUNNER_TEMP%\bezem-it-<uuid>` and sets env vars (`TEMP`, `LOCALAPPDATA`, …) to point into it, so tests never touch the runner's real profile. Known-folder resolution gets a test-only override trait; production code keeps using `SHGetKnownFolderPath`.

Each scenario asserts **exactly** which files disappeared, using a before/after manifest (path + size + hash), and that a **canary tree outside the sandbox is byte-identical** afterwards.

| Scenario | How it is built | Expected result |
|---|---|---|
| Junction escaping out | `CreateDirectory` + `FSCTL_SET_REPARSE_POINT` (or `junction` crate) in `%TEMP%\evil` → canary dir | Junction itself may be removed (as a link, never traversed); canary untouched; reported as "skipped: reparse point" |
| Directory symlink / file symlink | `CreateSymbolicLinkW` with `SYMBOLIC_LINK_FLAG_ALLOW_UNPRIVILEGED_CREATE` (runner has Developer Mode; else skip with notice) | Same as junction |
| Volume mount point | `SetVolumeMountPointW` onto a VHD mounted by the job (nightly only, needs admin) | Not traversed |
| Locked file | Hold a handle with `share_mode(0)` from a helper thread/process during clean | Skipped, listed in summary with "in use"; exit code reflects partial success |
| File locked mid-run | Lock acquired *after* scan, *before* delete | Re-check at delete time catches it; no crash |
| Read-only file | `FILE_ATTRIBUTE_READONLY` | Policy per PLAN: deleted only if the cleaner opts in (`clear_readonly = true`), otherwise skipped and reported |
| Hidden/system attributes | `FILE_ATTRIBUTE_HIDDEN \| SYSTEM` | Treated like normal files inside allowed roots; never widens roots |
| Long paths > 260 chars | Nested dirs to ~600 chars, created via `\\?\` prefix; test both with and without `LongPathsEnabled` | Scanned and deleted correctly; no truncation; no `MAX_PATH` errors |
| Unicode names | Names with emoji, RTL text, combining marks, NFC vs NFD duplicates, lone surrogates (invalid UTF-16) | Handled via `OsString` without lossy conversion; logged with escaping; never matched against the wrong file |
| Reserved names | Files named `CON`, `aux.txt`, `nul` (created with `\\?\`) | Deleted or skipped deterministically, never resolved as devices |
| Recent files | mtime = now − 1 h | Skipped by age threshold |
| Tree changes between scan and clean | Replace a scanned file with a junction after scan | Delete-time re-check refuses it |
| Recycle Bin mode | `--recycle` | Files land in the Recycle Bin (`SHFileOperation`/`IFileOperation`), not deleted |
| Permission denied | ACL denying the runner user `DELETE` | Skipped and reported, no elevation attempt |
| Elevation boundary (M5) | Admin cleaners on a non-admin token | Grouped and refused without relaunch in `--yes` CI mode |
| CLI contract | `bezem scan --json`, exit codes | JSON matches a checked-in schema; exit codes match docs |

Integration tests run on both `windows-2022` (Server 2022 ≈ Win10 21H2 kernel) and `windows-2025` (≈ Win11 24H2 kernel).

### 1.4 Property tests (`proptest`)

Strategies generate arbitrary path components (including `..`, `.`, separators, prefixes, reserved names, Unicode, very long segments) and arbitrary cleaner definitions.

Properties:

1. **Containment:** for any input, if `validate(p)` returns `Ok(r)`, then `r` is a strict descendant of an allowlisted root and of none of the forbidden roots.
2. **Normalization is idempotent:** `normalize(normalize(p)) == normalize(p)`.
3. **Normalization never widens:** `normalize(p)` is never an ancestor of `p`'s intended root.
4. **Parser round-trip:** `parse(serialize(def)) == def` for every valid `CleanerDef`.
5. **Planner subset:** for a random fake tree (model in memory), the delete plan is a subset of the scan result, and the scan result is a subset of the tree under allowed roots.
6. **Exclude monotonicity:** adding an exclude pattern never adds files to the plan.

PR runs use 256 cases; the nightly job sets `PROPTEST_CASES=10000`. Failing seeds are committed to `proptest-regressions/` and kept forever.

### 1.5 Fuzzing (`cargo-fuzz`)

Targets live in `fuzz/fuzz_targets/` and compile against the pure-logic modules (Linux, nightly):

| Target | Input | Oracle |
|---|---|---|
| `fuzz_path_normalize` | Arbitrary bytes → UTF-16 path | No panic; containment property holds; idempotent |
| `fuzz_env_expand` | Template + random env map | No panic; empty expansion never yields a root |
| `fuzz_definition_toml` | Arbitrary TOML text | No panic; parse ⇒ validate never accepts a forbidden root |
| `fuzz_glob` | Pattern + path | No panic, bounded time (no catastrophic backtracking) |
| `fuzz_firefox_profiles_ini` | `profiles.ini` bytes (M4) | No panic; profile paths pass the validator or are dropped |
| `fuzz_chromium_local_state` | `Local State` JSON (M4) | Same |

- Corpus seeded from `cleaners/*.toml` and the unit-test vectors; stored in a separate `bezem-fuzz-corpus` repo to keep the main repo small.
- PRs touching `safety`, `cleaners` loader or the parsers: 5-minute smoke per affected target.
- Nightly: 1 hour per target, crash → artifact + auto-opened issue labeled `security` + `fuzz` (private until triaged if it is a containment break).
- Apply to OSS-Fuzz after 1.0.

### 1.6 Definition validator in CI

`cargo run -p bezem-cleaners --bin validate -- cleaners/` must pass on every PR. It checks:

- [ ] Schema: required fields, known `category`/`risk` values, unique `id`, `id` matches filename
- [ ] Every `path` resolves (with a synthetic known-folder map) under the root allowlist; forbidden roots rejected
- [ ] No absolute drive-letter paths; only known-folder/env tokens from a closed list
- [ ] `risk = "high"` ⇒ `default_enabled = false`
- [ ] Privacy categories (`cookies`, `history`, `sessions`, `forms`) ⇒ `default_enabled = false` and `delete_mode = "recycle"`
- [ ] `requires_admin = true` iff any target is under an admin-only root
- [ ] Temp-folder targets have `min_age_hours >= 1`
- [ ] Named actions are from the closed built-in set
- [ ] `recursive = true` at a root that is itself a known folder (e.g. `%LOCALAPPDATA%` with no subpath) is rejected
- [ ] Each definition has a matching fixture test in `cleaners/tests/<id>.toml` (expected-deleted / expected-kept lists)

The validator also emits a **diff report** for PRs that change definitions (added/removed roots, risk changes), posted as a PR comment so reviewers see the effective change, not only the TOML diff.

### 1.7 Manual QA checklist (real Windows VMs)

Run on every release candidate (`vX.Y.Z-rc.N`). Use snapshot-able Hyper-V/VirtualBox VMs from Microsoft's free dev images or evaluation ISOs:

| VM | Edition / build | Locale | Extras |
|---|---|---|---|
| A | Windows 10 22H2 Pro | en-US | Chrome, Firefox (2 profiles), Edge |
| B | Windows 11 23H2 Home | nl-NL | Brave, Vivaldi, Opera; OneDrive Known Folder Move on |
| C | Windows 11 24H2/25H2 Pro | en-US, display scaling 150 % | Defender + SmartScreen on, standard (non-admin) user |
| D | Windows 11 on ARM (optional until arm64 ships) | en-GB | — |

Checklist (record results in the release tracking issue):

- [ ] Fresh download from the GitHub Release: SmartScreen behaviour noted (signed ⇒ no "unknown publisher")
- [ ] Defender full scan of the zip/MSI: no detections; VirusTotal link recorded
- [ ] `bezem --version` matches tag; Authenticode signature valid (`Get-AuthenticodeSignature`)
- [ ] `bezem list`, `scan`, `scan --json`, `clean` (confirm prompt), `clean --yes --recycle`, `show <id>`
- [ ] Browser open ⇒ browser cleaner refuses and offers to wait; closed ⇒ runs; user still logged in afterwards (opt-in privacy items off)
- [ ] Opt in to cookies ⇒ warning shown; items go to Recycle Bin
- [ ] Admin cleaners: UAC prompt appears only for that group; Windows Update service restarted afterwards (`Get-Service wuauserv`)
- [ ] Deletion log written to `%LOCALAPPDATA%\Bezem\logs`, readable, contains every removed path
- [ ] Planted junction `%TEMP%\trap → C:\Users\<me>\Documents`: Documents untouched
- [ ] OneDrive folder and files-on-demand placeholders never touched
- [ ] GUI (M6+): keyboard-only run-through, Narrator reads cleaner names and risk badges, light/dark follow Windows, 150 % scaling legible
- [ ] Process Monitor capture during a full run: **no network activity**, no writes outside `%LOCALAPPDATA%\Bezem` and the targeted roots
- [ ] No leftover processes, services, scheduled tasks, Run keys after exit (`autoruns` check)
- [ ] MSI install/uninstall leaves nothing behind except logs (documented); portable zip runs from a USB stick path with spaces and Unicode

### 1.8 Benchmarks and performance budgets

`criterion` micro-benchmarks for hot paths and a `hyperfine` macro-benchmark on a generated "typical profile" fixture (`xtask gen-profile --size typical`: ~150 k files, ~8 GB sparse, 3 Chromium profiles, 1 Firefox profile, deep `%TEMP%`).

| Budget | Target | Hard limit (CI fails) |
|---|---|---|
| `bezem scan` on typical profile, warm cache | < 2.0 s (PLAN §6) | 2.5 s, or > 20 % slower than `main` baseline |
| `bezem scan`, cold cache (nightly only, after `EmptyStandbyList`) | < 6 s | trend only |
| `bezem clean` of 50 k small files | < 5 s | 7 s |
| Path validation per file | < 2 µs | > 25 % regression |
| Definition load + validate (all built-ins) | < 20 ms | 50 ms |
| Startup to first output (`bezem --version`) | < 50 ms | 100 ms |
| Peak RSS during typical scan | < 100 MB | 150 MB |
| `bezem.exe` size (release, x64) | < 5 MB (PLAN §2) | 8 MB |
| `bezem-gui.exe` size | < 12 MB | 20 MB |
| GUI idle CPU | 0 % (no polling) | any measurable wake-ups |

Hosted runners are noisy, so the PR job runs base and head **in the same job** and compares (criterion `--save-baseline` / `--baseline`); absolute limits are checked with a 25 % tolerance. Nightly results are pushed to a `gh-pages` benchmark dashboard (`benchmark-action/github-action-benchmark`).

---

## 2. CI design (GitHub Actions)

### 2.1 Workflows

| File | Trigger | Purpose |
|---|---|---|
| `ci.yml` | `pull_request`, `push` to `main`, `merge_group` | Lint, test, validate, bench compare, build. Required checks. |
| `nightly.yml` | `schedule` 03:17 UTC, `workflow_dispatch` | Long fuzz, 10 k proptest, cold-cache bench, admin/mount-point integration, `cargo audit` |
| `release.yml` | `push` tag `v*.*.*` | Reproducible build → SignPath → checksums → SLSA attestation → draft GitHub Release |
| `publish.yml` | `release: published` | winget PR (`wingetcreate`), scoop bucket update |
| `codeql.yml` / `scorecard.yml` | weekly + PR | CodeQL (Actions + Rust), OpenSSF Scorecard |
| `labels.yml` | push to `main` touching `.github/labels.yml`, `workflow_dispatch` (dry run on PRs) | Sync labels from `.github/labels.yml`; never deletes labels missing from the file |
| `stale.yml` | schedule | Gentle stale bot (never on `security` or `data-loss`) |

### 2.2 Jobs in `ci.yml`

This is the target set of jobs. `ci.yml` grows into it milestone by milestone: M0 ships `fmt`/`clippy`/`test` on
`windows-latest` and `ubuntu-latest`, `deny`, `msrv`, `unsafe-audit`, `license-headers` and `ci-ok`. The jobs
that need code which does not exist yet (nextest profile, `xtask`, validator, fuzz targets, benches) are added in
the milestone that creates that code (see [section 6](#6-milestone-mapping)).

| Job | Runner(s) | Notes |
|---|---|---|
| `lint` | `ubuntu-latest` | `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` (Windows-only code is also linted, because `test` builds it on Windows with `-D warnings`), `typos`, license headers |
| `deny` | `ubuntu-latest` | `cargo deny check` (licenses: MIT-compatible allowlist; advisories; bans; sources = crates.io only) |
| `unsafe-audit` | `ubuntu-latest` | Fails if `unsafe` appears outside `bezem-core/src/win32/` (grep + `cargo geiger` report) |
| `test` | matrix: `windows-2022`, `windows-2025`, `windows-11-arm` × toolchain `stable`(pinned), `msrv` | `cargo nextest run --workspace --locked`, integration fake trees, proptest 256 |
| `test-logic-linux` | `ubuntu-latest` | Pure-logic crates/tests (catches accidental Win32 coupling of the safety core) |
| `validate-cleaners` | `windows-2025` | Definition validator + diff comment |
| `coverage` | `windows-2025` | `cargo llvm-cov nextest`; enforce thresholds for `safety`/`delete` |
| `fuzz-smoke` | `ubuntu-latest` | Only if `paths-filter` matches parsers/safety; 5 min per affected target |
| `bench` | `windows-2025` | Base vs head comparison, budget check |
| `build` | `windows-2025` | `--release --locked` for `x86_64` and `aarch64-pc-windows-msvc`; uploads unsigned artifacts; checks binary size |
| `ci-ok` | `ubuntu-latest` | Aggregator; the only required status check in branch protection |

Hardening for all workflows:
- `permissions: contents: read` at top level; jobs elevate individually.
- All third-party actions pinned to a **full commit SHA** (tags shown below for readability; Dependabot keeps SHAs current).
- `concurrency` cancels superseded PR runs.
- No secrets in `ci.yml`; fork PRs run with no secrets. `pull_request_target` is never used.
- `step-security/harden-runner` in audit mode, moved to `block` with an egress allowlist once stable.

### 2.3 Sample `ci.yml`

```yaml
name: CI

on:
  pull_request:
  push:
    branches: [main]
  merge_group:

permissions:
  contents: read

concurrency:
  group: ci-${{ github.workflow }}-${{ github.event.pull_request.number || github.ref }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}

env:
  CARGO_TERM_COLOR: always
  CARGO_INCREMENTAL: 0
  RUSTFLAGS: -D warnings
  RUST_BACKTRACE: 1

jobs:
  lint:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4            # pin to SHA
      - uses: dtolnay/rust-toolchain@stable  # reads rust-toolchain.toml
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets --locked -- -D warnings
      - uses: crate-ci/typos@v1

  deny:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: EmbarkStudios/cargo-deny-action@v2
        with:
          command: check licenses advisories bans sources

  unsafe-audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: unsafe only in the reviewed win32 module
        run: |
          if grep -rn --include='*.rs' -E '\bunsafe\b' crates \
               | grep -v '^crates/bezem-core/src/win32/'; then
            echo "::error::unsafe code outside bezem-core/src/win32"; exit 1
          fi

  changes:
    runs-on: ubuntu-latest
    outputs:
      parsers: ${{ steps.f.outputs.parsers }}
    steps:
      - uses: actions/checkout@v4
      - id: f
        uses: dorny/paths-filter@v3
        with:
          filters: |
            parsers:
              - 'crates/bezem-core/src/safety/**'
              - 'crates/bezem-core/src/path/**'
              - 'crates/bezem-cleaners/src/**'
              - 'fuzz/**'

  test:
    name: test (${{ matrix.os }}, ${{ matrix.toolchain }})
    strategy:
      fail-fast: false
      matrix:
        os: [windows-2022, windows-2025, windows-11-arm]
        toolchain: [pinned, msrv]
    runs-on: ${{ matrix.os }}
    steps:
      - name: Enable long paths and Developer Mode (symlinks)
        shell: pwsh
        run: |
          Set-ItemProperty HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem LongPathsEnabled 1
          reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\AppModelUnlock" /t REG_DWORD /f /v AllowDevelopmentWithoutDevLicense /d 1
          git config --system core.longpaths true
      - uses: actions/checkout@v4
      - name: Select toolchain
        shell: bash
        run: |
          if [ "${{ matrix.toolchain }}" = msrv ]; then
            v=$(grep -m1 rust-version Cargo.toml | cut -d'"' -f2)
            rustup toolchain install "$v" --profile minimal && rustup override set "$v"
          fi
          rustup show
      - uses: Swatinem/rust-cache@v2
        with:
          key: ${{ matrix.toolchain }}
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-nextest
      - run: cargo nextest run --workspace --locked --profile ci
        env:
          PROPTEST_CASES: 256
          BEZEM_IT_ROOT: ${{ runner.temp }}\bezem-it
      - name: Long-path tests with LongPathsEnabled=0
        shell: pwsh
        run: |
          Set-ItemProperty HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem LongPathsEnabled 0
          cargo nextest run -p bezem-core --locked --profile ci -E 'test(long_path)'
      - uses: actions/upload-artifact@v4
        if: failure()
        with:
          name: junit-${{ matrix.os }}-${{ matrix.toolchain }}
          path: target/nextest/ci/junit.xml

  test-logic-linux:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo test --locked -p bezem-core --lib --no-default-features --features pure-logic
      - run: cargo test --locked -p bezem-cleaners

  validate-cleaners:
    runs-on: windows-2025
    permissions:
      contents: read
      pull-requests: write   # posts the definition diff comment
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo run --locked -p bezem-cleaners --bin validate -- cleaners/ --strict
      - if: github.event_name == 'pull_request'
        run: >
          cargo run --locked -p bezem-cleaners --bin validate --
          --diff origin/${{ github.base_ref }} --markdown > def-diff.md
      - if: github.event_name == 'pull_request' && hashFiles('def-diff.md') != ''
        uses: marocchino/sticky-pull-request-comment@v2
        with:
          path: def-diff.md

  coverage:
    runs-on: windows-2025
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { components: llvm-tools-preview }
      - uses: taiki-e/install-action@v2
        with: { tool: 'cargo-llvm-cov,cargo-nextest' }
      - run: cargo llvm-cov nextest --workspace --locked --lcov --output-path lcov.info
      - run: cargo xtask coverage-gate lcov.info --module safety=95 --module delete=95 --default=75

  fuzz-smoke:
    needs: changes
    if: needs.changes.outputs.parsers == 'true'
    runs-on: ubuntu-latest
    strategy:
      matrix:
        target: [fuzz_path_normalize, fuzz_env_expand, fuzz_definition_toml, fuzz_glob]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - uses: taiki-e/install-action@v2
        with: { tool: cargo-fuzz }
      - run: cargo fuzz run ${{ matrix.target }} -- -max_total_time=300 -rss_limit_mb=2048
      - uses: actions/upload-artifact@v4
        if: failure()
        with:
          name: fuzz-crash-${{ matrix.target }}
          path: fuzz/artifacts/${{ matrix.target }}

  bench:
    if: github.event_name == 'pull_request'
    runs-on: windows-2025
    steps:
      - uses: actions/checkout@v4
        with: { fetch-depth: 0 }
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo xtask gen-profile --size typical --out "$env:RUNNER_TEMP\profile"
        shell: pwsh
      - name: Baseline (base branch)
        run: |
          git checkout ${{ github.event.pull_request.base.sha }}
          cargo bench --locked -p bezem-core -- --save-baseline base
          cargo build --release --locked -p bezem-cli && cp target/release/bezem.exe bezem-base.exe
        shell: bash
      - name: Head
        run: |
          git checkout ${{ github.event.pull_request.head.sha }}
          cargo bench --locked -p bezem-core -- --baseline base
          cargo build --release --locked -p bezem-cli
        shell: bash
      - run: cargo xtask perf-gate --base bezem-base.exe --head target/release/bezem.exe --budgets docs/perf-budgets.toml
        shell: bash

  build:
    runs-on: windows-2025
    strategy:
      matrix:
        target: [x86_64-pc-windows-msvc, aarch64-pc-windows-msvc]
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with: { targets: '${{ matrix.target }}' }
      - uses: Swatinem/rust-cache@v2
      - run: cargo build --release --locked --workspace --target ${{ matrix.target }}
      - run: cargo xtask size-gate --target ${{ matrix.target }}
      - uses: actions/upload-artifact@v4
        with:
          name: bezem-unsigned-${{ matrix.target }}
          path: |
            target/${{ matrix.target }}/release/bezem.exe
            target/${{ matrix.target }}/release/bezem-gui.exe
          retention-days: 7

  ci-ok:
    if: always()
    needs: [lint, deny, unsafe-audit, test, test-logic-linux, validate-cleaners, coverage, fuzz-smoke, bench, build]
    runs-on: ubuntu-latest
    steps:
      - name: All required jobs passed (skipped is OK)
        run: |
          echo '${{ toJSON(needs) }}' | jq -e 'all(.[]; .result == "success" or .result == "skipped")'
```

Branch protection on `main`: require `ci-ok`, 1 approving review (2 for `safety`/`delete`/`cleaners` via CODEOWNERS), linear history, signed commits encouraged (required for maintainers), merge queue on.

---

## 3. Release process

### 3.1 Versioning

- **SemVer 2.0.** Pre-1.0 (`0.y.z`): minor bumps may break CLI/JSON/definition schema; this is called out in the changelog.
- From **1.0** the public contract is: CLI flags and exit codes, `scan --json` schema, cleaner TOML schema (`schema_version` field), log format. Breaking any of these ⇒ major bump.
- Adding a new built-in cleaner or a new opt-in option ⇒ minor. Fixing/tightening a definition ⇒ patch. **Widening** an existing definition's roots ⇒ minor, never patch, so users notice it.
- Pre-releases: `vX.Y.Z-rc.N`. M3 ships as `v0.1.0`, M6 as `v0.5.0`, M7 as `v1.0.0` (PLAN §8).
- All crates in the workspace share one version (`workspace.package.version`). Crates are **not** published to crates.io before 1.0 (decide post-1.0).

### 3.2 Changelog

- `CHANGELOG.md` in [Keep a Changelog](https://keepachangelog.com) format; sections `Added / Changed / Deprecated / Removed / Fixed / Security`, plus a Bezem-specific **`Cleaners`** section listing every definition added, removed, or whose roots/risk changed.
- Every PR adds a line under `## [Unreleased]` or carries the `no-changelog` label (CI check).
- Commit messages follow Conventional Commits (`feat:`, `fix:`, `cleaner:`, `safety:`) to help generate release notes; enforcement is soft (PR title lint only).

### 3.3 Release checklist (tracked as a GitHub issue from a template)

1. [ ] Milestone issues closed or moved; no open `data-loss` or `security` issues tagged for this release
2. [ ] `cargo xtask release prep X.Y.Z`: bumps versions, rolls `Unreleased` in CHANGELOG, updates `cleaners` manifest hash
3. [ ] PR "Release X.Y.Z" merged through the normal review
4. [ ] Tag `vX.Y.Z-rc.1` (signed tag: `git tag -s`) ⇒ `release.yml` builds a **pre-release**
5. [ ] Manual QA checklist (§1.7) on all VMs; results posted in the release issue
6. [ ] Upload to VirusTotal; submit to Microsoft Defender WDSI as "software developer" if any detection
7. [ ] Tag `vX.Y.Z` on the same commit ⇒ `release.yml` builds, signs, attests, drafts release
8. [ ] Second maintainer verifies reproducibility (§3.5) and signature, then publishes the draft
9. [ ] `publish.yml` opens winget PR and updates scoop bucket; maintainer watches both to merge
10. [ ] Announce (GitHub Discussions, README badge); close milestone

### 3.4 `release.yml` pipeline

```
tag v* ──► build (windows-2025, x64 + arm64, pinned toolchain, --locked)
        ──► package: bezem-X.Y.Z-windows-x64.zip, -arm64.zip, bezem-X.Y.Z-x64.msi (WiX v4, cargo-wix)
        ──► upload unsigned artifacts ──► SignPath (signpath/github-action-submit-signing-request)
             · origin verification: SignPath checks the artifact came from this repo/workflow/tag
             · signs .exe and .msi with Authenticode (SHA-256, RFC 3161 timestamp)
             · release-signing policy requires manual approval by a maintainer in SignPath
        ──► verify signatures (signtool verify /pa /all)
        ──► SHA256SUMS (+ SHA256SUMS.sig via Sigstore/cosign keyless)
        ──► actions/attest-build-provenance  (SLSA v1 provenance, Build L2+; L3 via slsa-github-generator once stable for this setup)
        ──► SBOM: cargo cyclonedx → bezem-X.Y.Z.cdx.json (attested too)
        ──► draft GitHub Release with notes from CHANGELOG
```

Permissions are scoped per job: only the attest job gets `id-token: write` + `attestations: write`; only the release job gets `contents: write`. The SignPath API token lives in a protected **`release` environment** that requires maintainer approval and is limited to `v*` tags.

**Release assets per version:**

| Asset | Contents |
|---|---|
| `bezem-X.Y.Z-windows-x64.zip` | `bezem.exe`, `bezem-gui.exe`, `LICENSE`, `README.txt`, `THIRD_PARTY_LICENSES.html` (`cargo about`) |
| `bezem-X.Y.Z-windows-arm64.zip` | Same, arm64 |
| `bezem-X.Y.Z-x64.msi` / `-arm64.msi` | Per-user install by default (no admin), Start-menu shortcut, uninstaller; **no** services, scheduled tasks or Run keys |
| `SHA256SUMS`, `SHA256SUMS.sig`, `SHA256SUMS.pem` | Checksums and keyless signature |
| `bezem-X.Y.Z.cdx.json` | SBOM |
| Provenance | Stored as GitHub attestation; verify with `gh attestation verify <file> --repo <owner>/Bezem` |

### 3.5 Reproducible builds

- `rust-toolchain.toml` pins an exact toolchain; `Cargo.lock` committed; `--locked` everywhere.
- `RUSTFLAGS="--remap-path-prefix=$PWD=. --remap-path-prefix=$CARGO_HOME=cargo"`, `SOURCE_DATE_EPOCH` = tag commit time, `/Brepro` linker flag (MSVC deterministic PE timestamps), `codegen-units=1`, `lto = "fat"`, `panic = "abort"`, `strip = "symbols"` (PDBs published separately).
- Embedded cleaner definitions are sorted and hashed at build time; the hash is printed by `bezem --version --verbose`.
- **Signatures change bytes.** Reproducibility is checked on the **unsigned** binaries: CI publishes `SHA256SUMS-unsigned` too, and `cargo xtask verify-repro` rebuilds the tag locally and compares. Anyone can also strip the Authenticode signature (`osslsigncode remove-signature`) and compare.
- A second, independent rebuild on a fresh runner runs in `release.yml`; mismatched hashes fail the release.

### 3.6 Package managers

**winget** (`microsoft/winget-pkgs`)
- Package identifier `Bezem.Bezem` (decide final publisher id at M7). Manifests: `version`, `defaultLocale` (en-US), `locale.nl-NL`, `installer` (MSI per-user + zip `portable` installer type for `bezem.exe` with `Commands: [bezem]`).
- `publish.yml` runs `wingetcreate update Bezem.Bezem -v X.Y.Z -u <msi-url> <zip-url> --submit` using a fine-grained PAT of a bot account limited to its own fork.
- First submission done by hand at M7 (expect moderation review; signed binaries help).

**scoop**
- Own bucket repo `bezem/scoop-bezem` first (instant), then propose to the `extras` bucket after 1.0.
- Manifest uses the portable zip, `bin: bezem.exe`, `shortcuts` for the GUI, `checkver: github`, `autoupdate` with `hash.url` pointing to `SHA256SUMS`.

**Not doing:** Chocolatey before 1.1 (extra moderation load), Microsoft Store (revisit post-1.0), any bundled installer offers ever.

---

## 4. Governance and community

### 4.1 Roles

| Role | Who | Rights |
|---|---|---|
| Maintainer | Founding maintainer(s), listed in `MAINTAINERS.md` | Merge, release, SignPath approval, security triage |
| Cleaner reviewer | Trusted contributors with ≥ 5 merged definition PRs | Approve `cleaners/**` (still needs one maintainer for root changes) |
| Contributor | Anyone | PRs, issues, reviews |

Decisions: lazy consensus in issues/Discussions; maintainers decide on deadlock. Larger changes (new action type, schema change, network code) need a short **RFC** issue labeled `rfc` and a 7-day comment window. The principles in PLAN §1 are not up for an RFC.

`CODEOWNERS` (target layout once the project has an organization with `maintainers` and `cleaner-reviewers`
teams; until then [`.github/CODEOWNERS`](../.github/CODEOWNERS) assigns every path to the single maintainer):
```
/crates/bezem-core/src/safety/   @maintainers
/crates/bezem-core/src/delete/   @maintainers
/crates/bezem-core/src/win32/    @maintainers
/cleaners/                       @cleaner-reviewers @maintainers
/.github/                        @maintainers
```

### 4.2 `CONTRIBUTING.md` outline

1. Welcome + the five principles (link PLAN §1); "features that break a principle will be declined"
2. Ways to help: bug reports, cleaner definitions, translations (EN/NL first), docs, testing on real machines
3. Dev setup: Windows 10 22H2+/11, Rust via `rustup` (toolchain auto-selected), VS Build Tools; optional Linux for pure-logic work
4. Build, test, `cargo xtask` commands (`gen-profile`, `validate`, `fuzz`, `bench`)
5. **Never run `bezem clean` against your real profile while developing**: use `BEZEM_IT_ROOT` sandbox or a VM
6. Writing a cleaner definition: schema reference, fixture test requirement, evidence of what the folder contains (link to vendor docs or a screenshot), how to compute the risk level
7. Code style: `rustfmt`, clippy clean, `#![forbid(unsafe_code)]` outside `win32`, error handling (`thiserror` in libs, `anyhow` in bins), no new network code
8. Commit/PR conventions, changelog line, DCO sign-off (`git commit -s`), no CLA
9. Dependency policy: MIT-compatible licenses only (`cargo deny`), justify every new crate in the PR, prefer `windows` crate over `winapi`
10. Review expectations and response times (best effort: first response within 7 days)
11. Security issues: **do not open a public issue**, see SECURITY.md

### 4.3 Cleaner-definition review checklist

Added automatically to PRs touching `cleaners/**` (PR template section). Reviewer ticks all before approving:

- [ ] CI validator green; definition diff comment reviewed (effective roots, not just TOML)
- [ ] Target is **regenerable data** (cache, temp, logs, dumps), not user data, settings, licences, save games or credentials
- [ ] Source cited: vendor documentation, or reproducible evidence of what writes and reads the folder
- [ ] Path is as **narrow** as possible (e.g. `...\Cache\Cache_Data`, not `...\User Data`)
- [ ] Multi-profile/multi-install handled (Chromium `Profile N`, Firefox `profiles.ini`, portable installs) without globbing upward
- [ ] `risk` level justified; `high` ⇒ off by default; privacy data ⇒ opt-in + Recycle Bin + warning text
- [ ] App-running check declared if the app holds the files open
- [ ] `min_age_hours` set sensibly for anything an app might be writing right now
- [ ] `exclude` covers known lock/state files
- [ ] `requires_admin` correct; no admin needed ⇒ must not touch machine-wide roots
- [ ] OneDrive/Known Folder Move, roaming profiles and non-`C:` system drives considered
- [ ] Fixture test lists files that must be **kept**, not only deleted
- [ ] Tested by the author on a real install (Windows version + app version stated in the PR)
- [ ] Description and name are honest and plain (no "boost", "junk", "issues")
- [ ] Changelog `Cleaners` entry present; widening an existing definition ⇒ minor version

### 4.4 `SECURITY.md` policy

- **Scope:** in addition to classic vulnerabilities, **any bug that deletes or could delete a file outside what the preview showed, outside an allowlisted root, or through a reparse point is a security bug**. So is any network traffic in a build that should have none, privilege escalation via the elevated relaunch, DLL search-order hijacking of `bezem.exe`, and log injection.
- **Reporting:** GitHub Private Vulnerability Reporting (preferred) or `security@` address (to be set up at M0, forwarded to all maintainers). No bug bounty (state this plainly).
- **Response targets:** acknowledge ≤ 72 h; triage + severity (CVSS v4, plus a Bezem "data-loss" flag) ≤ 7 days; fix for data-loss bugs ≤ 14 days, others ≤ 90 days; coordinated disclosure with credit.
- **Data-loss handling:** if a released version can delete wrong files, publish a GitHub Security Advisory + CVE, yank the affected release (mark as "do not use", remove winget/scoop versions), pin a banner issue, and ship a patch release. Add a regression test and a post-mortem in `docs/incidents/`.
- **Supported versions:** latest minor of the current major; 1.x LTS policy decided at 1.0.
- **Users who lost data:** guidance on the deletion log, Recycle Bin, Windows File History/previous versions, and stopping disk writes before recovery attempts.

### 4.5 Code of conduct

Adopt **Contributor Covenant 2.1** verbatim in `CODE_OF_CONDUCT.md`. Enforcement contact: a `conduct@` address reaching all maintainers (not the same inbox as security). Enforcement ladder as in the Covenant.

### 4.6 Issue and PR templates (`.github/`)

Issue forms (`ISSUE_TEMPLATE/*.yml`), blank issues disabled, `config.yml` links Discussions and Security:

| Template | Key fields |
|---|---|
| `bug.yml` | Bezem version, install type (zip/MSI/winget/scoop), Windows build (`winver`), CLI or GUI, steps, expected/actual, `bezem scan --json` output (redact usernames), log excerpt |
| `data-loss.yml` | Big red notice: "if a file outside the preview was deleted, report privately via Security instead"; otherwise same as bug + which files were unexpectedly removed |
| `false-positive-av.yml` | AV product + version, detection name, file hash, VirusTotal link |
| `new-cleaner.yml` | App name/version, folder paths, what's inside, evidence it's regenerable, estimated size, risk suggestion |
| `feature.yml` | Problem, proposal, which principle it might touch |
| `release.yml` | The release checklist from §3.3 (maintainers only) |

`PULL_REQUEST_TEMPLATE.md`: summary, linked issue, type (code / cleaner / docs / ci), tests added, changelog line, "touches safety/delete?" checkbox, tested-on Windows version, and the §4.3 checklist folded in a `<details>` block for cleaner PRs.

### 4.7 Labels

Defined in `.github/labels.yml` and synced by `labels.yml`:

| Group | Labels |
|---|---|
| Type | `type: bug`, `type: feature`, `type: cleaner`, `type: docs`, `type: ci`, `type: refactor`, `rfc` |
| Severity | `data-loss` (top priority, never auto-closed), `security`, `crash`, `regression` |
| Area | `area: core`, `area: safety`, `area: cli`, `area: gui`, `area: cleaners`, `area: elevation`, `area: packaging`, `area: i18n` |
| Cleaner category | `cleaner: system`, `cleaner: browser`, `cleaner: privacy`, `cleaner: apps` |
| Status | `needs-triage` (default), `needs-repro`, `needs-info`, `blocked`, `accepted`, `wontfix: out-of-scope` (link PLAN §4 "out of scope") |
| Platform | `win10`, `win11`, `arm64` |
| Contributor | `good first issue`, `help wanted`, `mentor available` |
| Process | `no-changelog`, `release-blocker`, `fuzz`, `perf`, `dependencies` (Dependabot PRs) |

---

## 5. Risk register

Likelihood and impact: **L**ow / **M**edium / **H**igh.

| # | Risk | Likelihood | Impact | Mitigation | Owner |
|---|---|---|---|---|---|
| R1 | **Accidental data loss** (bug in path resolution, reparse handling, glob, or a bad definition) | M | H | Safety layer with allowlist + forbidden roots; never traverse reparse points; delete-time re-check; ≥ 95 % coverage + proptest + fuzz on `safety`; canary-tree integration tests; definition validator; 2-reviewer rule; Recycle Bin default for privacy data; deletion log; data-loss = security process (§4.4) | Maintainers |
| R2 | **Malicious or careless community definition** | M | H | Closed set of named actions, no arbitrary commands; validator rejects out-of-allowlist roots; CODEOWNERS + checklist §4.3; community defs (post-1.0) opt-in with diff view | Cleaner reviewers |
| R3 | **AV false positives** (heuristics flag "deletes many files" + unsigned/new binary) | H (early), M later | M | SignPath signing; stable publisher name; no packers/UPX; no self-modification or download-and-execute; submit each release to Microsoft WDSI and major vendors; VirusTotal check in release checklist; `false-positive-av` template; document verification for users | Release maintainer |
| R4 | **SmartScreen "unknown publisher" / low reputation** | H until reputation builds | M | Authenticode via SignPath (OV cert; reputation accrues per cert); keep same cert across releases; winget/scoop as primary channels; README explains SmartScreen and how to verify checksums/provenance | Release maintainer |
| R5 | **Windows changes paths/behaviour across feature updates** (e.g. new cache locations, Delivery Optimization changes, ACL changes) | M | M | Paths via `SHGetKnownFolderPath`, not hard-coded strings; CI on Server 2022 + 2025 runners; manual QA on 10 22H2 + latest 11; Windows Insider VM checked before each Windows feature update; definitions version-gated (`min_windows_build`); missing path = skip, never fall back to a parent | Maintainers |
| R6 | **App vendors change cache layout** (browsers update every 4 weeks) | H | L–M | Narrow paths; unknown structure ⇒ skip; app-running check; issue template for broken cleaners; quarterly definition review | Cleaner reviewers |
| R7 | **Maintainer bandwidth / bus factor** | H | H | Aim for ≥ 2 maintainers with release + SignPath rights by 1.0; automate releases end to end; small scope (PLAN §4 out-of-scope list); stale bot for non-critical issues; clear "best effort" SLAs; documented release runbook so anyone with rights can ship | Project lead |
| R8 | **Supply-chain compromise** (dependency, GitHub Action, CI token) | L–M | H | `cargo deny` sources = crates.io; `cargo audit` nightly; minimal deps; Actions pinned to SHA; least-privilege `permissions`; secrets only in protected `release` environment; SignPath origin verification + manual approval; SLSA provenance; reproducible builds verified by a second party; OpenSSF Scorecard | Maintainers |
| R9 | **Signing key or SignPath account loss/abuse** | L | H | Keys held by SignPath (HSM), never by maintainers; 2FA on GitHub + SignPath for all maintainers; approval required per release; revocation runbook | Project lead |
| R10 | **SignPath free OSS program unavailable or terms change** | L | M | Keep pipeline signer-agnostic (`sign` step behind a script); fallback: Azure Trusted Signing (low monthly cost) or donated cert; unsigned + provenance as last resort, clearly communicated | Project lead |
| R11 | **Elevated relaunch abused** (DLL hijack, argument injection into the admin process) | L | H | Relaunch with absolute path and allowlisted arguments only; `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)`; no elevated work beyond the admin cleaner group; security review of `win32` module each release | Maintainers |
| R12 | **Performance regression** misses the 2 s scan budget | M | L | PR bench comparison + hard gate; nightly trend dashboard | Maintainers |
| R13 | **Accidental network code** (e.g. via a dependency's feature) | L | M (breaks a principle) | `cargo deny` bans for HTTP crates in default features; Process Monitor check in manual QA; CI test that `bezem.exe` imports no `winhttp`/`ws2_32` (dumpbin import check) unless the opt-in update check feature is enabled | Maintainers |
| R14 | **Users treat Bezem as a "speed booster" and expect magic; reputational harm from comparisons to scareware** | M | L | Honest copy (PLAN §1), no counters or scores, clear docs on what is and isn't cleaned and why | Maintainers |
| R15 | **Legal/trademark friction** over browser/app names in cleaner definitions | L | L | Nominative use only, no logos; neutral names ("Chrome cache") | Project lead |

Review this register at every milestone exit (M0–M7) and after any `data-loss` or `security` incident.

---

## 6. Milestone mapping

| Milestone | Quality/release deliverables from this document |
|---|---|
| **M0** | `ci.yml` (lint, deny, unsafe-audit, test on Windows, `ci-ok`), CONTRIBUTING, SECURITY, CODE_OF_CONDUCT, issue/PR templates, labels, CODEOWNERS, branch protection |
| **M1** | Safety unit tests (§1.2), fake-tree integration harness (§1.3), proptest (§1.4), first fuzz targets, coverage gate |
| **M2** | Definition validator + diff comment (§1.6), cleaner review checklist, `fuzz_definition_toml` |
| **M3** | `release.yml` (unsigned pre-release, checksums, provenance, SBOM), CHANGELOG, bench job + budgets, first manual QA pass, `v0.1.0` |
| **M4** | Browser profile fuzz targets, multi-browser QA VMs |
| **M5** | Elevation tests, admin/mount-point nightly job, R11 review |
| **M6** | GUI QA items (keyboard, Narrator, scaling, idle CPU), GUI size budget, `v0.5.0` |
| **M7** | SignPath integration, reproducibility verification, MSI, winget + scoop publishing, AV/SmartScreen submissions, `v1.0.0` |

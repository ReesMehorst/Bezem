# Contributing to Bezem

Thank you for your interest in Bezem. This guide explains how to help and what we expect from a
contribution.

> **Project status: pre-alpha (milestone M0).** The repository holds an empty Rust workspace. The only
> thing that works is `bezem --version`. Several tools mentioned below are planned but do not exist
> yet; they are marked *(planned)*.

## 1. Principles

Bezem follows five non-negotiable principles, described in [PLAN.md §1](docs/PLAN.md#1-principles-non-negotiable):
**safe by default, transparent, zero overhead, zero telemetry, honest.**

A feature or change that breaks one of them will be declined, however well it is written. The
principles are not open to an RFC. The [out-of-scope list](docs/PLAN.md#explicitly-out-of-scope)
(registry cleaning, prefetch clearing, "memory optimizers" and similar) is part of that.

## 2. Ways to help

- **Bug reports**: use the bug issue form. If Bezem deleted a file it should not have, do **not**
  open a public issue; see [SECURITY.md](SECURITY.md).
- **Cleaner definitions** *(from milestone M2)*: propose one with the "New cleaner" issue form first.
- **Translations** *(planned, English and Dutch first)*.
- **Documentation**: the plans in [`docs/`](docs/) are the specification. Fixes to unclear or
  inconsistent text are welcome.
- **Testing on real machines** *(once there is something to test)*: different Windows versions,
  languages, OneDrive setups and browser installs.

## 3. Development setup

- **Windows 10 22H2 or Windows 11** is the target platform.
- **Rust via [`rustup`](https://rustup.rs)**. The toolchain is pinned in `rust-toolchain.toml` and is
  selected automatically. The minimum supported Rust version is set in `Cargo.toml`
  (`rust-version`).
- **Visual Studio Build Tools** with the "Desktop development with C++" workload (for the MSVC
  linker).
- **Optional: Linux or macOS** for pure-logic work. The M0 workspace builds there, but Windows-only
  code and the integration tests will need Windows.

```sh
git clone https://github.com/ReesMehorst/Bezem.git
cd Bezem
cargo build
cargo run -p bezem-cli -- --version
```

## 4. Build and test

These commands work today and match what CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny check            # needs `cargo install cargo-deny --locked`
```

*(Planned)* A `cargo xtask` helper with `gen-profile` (generate a fake user profile for tests),
`validate` (check cleaner definitions), `fuzz` and `bench`. It will be documented here when it exists.
See [QUALITY_AND_RELEASE.md §1](docs/QUALITY_AND_RELEASE.md#1-test-strategy) for the test strategy.

## 5. Never clean your real profile while developing

Once cleaning exists, **never run `bezem clean` against your real user profile from a development
build.** Use a virtual machine, or the sandbox root that the integration tests will use
(`BEZEM_IT_ROOT`, *planned*), which points every cleaner at a fake tree. At M0 there is no cleaning
code, so there is nothing to run yet.

## 6. Writing a cleaner definition *(from milestone M2)*

Cleaners are TOML files in [`cleaners/`](cleaners/). A definition PR needs:

- a definition that follows the schema in
  [ARCHITECTURE.md §5](docs/ARCHITECTURE.md#5-cleaner-definition-schema-toml);
- a fixture test that lists the files that must be **kept**, not only the ones deleted;
- evidence of what the folder contains and that it is regenerable: a link to vendor documentation,
  or a reproducible description of what writes and reads it;
- a justified risk level (`high` means off by default; privacy data is opt-in and goes to the Recycle
  Bin by default);
- the Windows version and app version you tested on.

Reviewers use the [cleaner review checklist](docs/QUALITY_AND_RELEASE.md#43-cleaner-definition-review-checklist),
which is also in the pull request template. Keep names and descriptions plain: no "boost", "junk" or
"issues".

## 7. Code style

- Format with `rustfmt` (settings in `rustfmt.toml`) and keep `clippy` clean with `-D warnings`.
- `#![forbid(unsafe_code)]` in every crate except `bezem-core`, which uses `deny` and allows `unsafe`
  only in its small, reviewed `win32` module. Every `unsafe` block needs a `// SAFETY:` comment.
- Errors: `thiserror` in libraries, `anyhow` only in the binaries (`bezem-cli`, `bezem-gui`).
- Every source file starts with `// SPDX-License-Identifier: MIT`.
- **No network code.** Bezem makes no network calls. The only exception ever considered is an
  opt-in update check ([PLAN.md §10](docs/PLAN.md#10-open-decisions)), and adding it needs an RFC.

## 8. Commits and pull requests

- Keep PRs small and focused. Fill in the pull request template.
- Commit messages and PR titles follow [Conventional Commits](https://www.conventionalcommits.org)
  (`feat:`, `fix:`, `docs:`, `ci:`, plus Bezem's `cleaner:` and `safety:`).
- Add a line under `## [Unreleased]` in [CHANGELOG.md](CHANGELOG.md), or ask a maintainer for the
  `no-changelog` label if the change is not user-visible.
- **Sign off every commit** under the [Developer Certificate of Origin](https://developercertificate.org)
  with `git commit -s`. This adds a `Signed-off-by:` line and certifies that you may submit the work
  under the project's MIT license. There is **no CLA**.
- Larger changes (a new action type, a schema change, anything touching the network) start as an
  issue labeled `rfc` with a 7-day comment window.

## 9. Dependencies

The policy is in [ARCHITECTURE.md §14](docs/ARCHITECTURE.md#14-dependencies).

- Only MIT-compatible licenses (MIT, Apache-2.0, BSD, Zlib, Unicode-3.0). `cargo deny check` enforces
  this, together with RustSec advisories and a ban on HTTP, TLS and socket crates.
- Justify every new crate in the PR description: what it does, why the standard library or an
  existing dependency is not enough, and its size.
- For Windows APIs use the official `windows` crate, not `winapi`.

## 10. Review

Reviews are done by volunteers on a best-effort basis. We aim to give a first response within
**7 days**. Changes to the safety and deletion code and to cleaner definitions need two approvals.
If a PR has waited longer than that, a polite ping on the PR is welcome.

## 11. Security issues

**Do not open a public issue for a security problem or for a file deleted outside what the preview
showed.** Follow [SECURITY.md](SECURITY.md) instead.

Everyone taking part in the project follows the [Code of Conduct](CODE_OF_CONDUCT.md).

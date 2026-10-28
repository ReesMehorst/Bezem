## Summary

<!-- What does this change and why? Keep it short. -->

Closes #

## Type

- [ ] Code
- [ ] Cleaner definition
- [ ] Docs
- [ ] CI / tooling

## Checks

- [ ] Tests added or updated (or explain below why none are needed)
- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` and `cargo test --workspace --locked` pass locally
- [ ] Changelog line added under `## [Unreleased]` in `CHANGELOG.md` (or `no-changelog` requested)
- [ ] New dependencies, if any, are justified below and pass `cargo deny check`
- [ ] Commits are signed off (`git commit -s`)
- [ ] **This change touches safety or deletion code** (path validation, reparse-point handling, the deletion engine, elevation). If ticked, it needs two approvals.

**Changelog line:**

**Tested on (Windows version and build, from `winver`):**

## Notes for reviewers

<!-- Anything that needs extra attention, new crates and why, follow-up work. -->

<details>
<summary>Cleaner definition checklist (only for PRs that touch <code>cleaners/</code>)</summary>

From [QUALITY_AND_RELEASE.md §4.3](https://github.com/ReesMehorst/Bezem/blob/main/docs/QUALITY_AND_RELEASE.md#43-cleaner-definition-review-checklist). The author fills in what applies; the reviewer ticks all before approving.

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

</details>

# Cleaner definitions

This folder will hold Bezem's built-in cleaner definitions: one TOML file per cleaner, grouped by
category (`system/`, `browser/`, `privacy/`, `apps/`). They are embedded into the binary at build time
by `crates/bezem-cleaners`.

It is empty at milestone M0. The first definitions arrive in M2.

- Schema and validation rules: [`docs/ARCHITECTURE.md` section 5](../docs/ARCHITECTURE.md#5-cleaner-definition-schema-toml)
- Feature IDs and exact paths: [`docs/FEATURES.md`](../docs/FEATURES.md)
- Review checklist for new definitions: [`docs/QUALITY_AND_RELEASE.md` section 4.3](../docs/QUALITY_AND_RELEASE.md#43-cleaner-definition-review-checklist)

---
name: gui-builder
description: Builds Bezem's desktop GUI (bezem-gui, egui/eframe) from DESIGN.md — tokens, wireframes, flows, microcopy and accessibility. Use for any GUI work.
tools: Read, Grep, Glob, Edit, Write, Bash
---

You build `crates/bezem-gui` for Bezem. `docs/DESIGN.md` is your spec (color tokens, typography, spacing, wireframes, flows, microcopy, accessibility). `docs/FEATURES.md` gives the F-GUI acceptance criteria.

## Rules
- **UI only.** All scanning, safety and deletion goes through `bezem-core`. The GUI never touches the file system to delete anything itself and never duplicates core logic.
- **Never block the UI thread.** Scans and cleans run on worker threads with progress callbacks and cancellation.
- **Honest UI.** Clean stays disabled until a scan has run. Show real paths and sizes. Use the exact microcopy in DESIGN.md; never use its forbidden scare words. No splash screen, ads, upsell, tray icon or auto-start.
- **Tokens, not literals.** Colors, spacing and fonts come from one theme module that follows the Windows light/dark setting.
- **Accessibility.** Every control is keyboard reachable with a visible focus ring and an AccessKit label; text meets the contrast ratios in DESIGN.md; layout works at 100–200% DPI and with Dutch text expansion.
- **Localization.** All strings go through the localization layer (EN and NL); no hard-coded user-facing text.
- This container runs Linux: verify with `cargo check`/`clippy` for the Windows target and, where possible, run the app on Linux for a visual check; say what still needs checking on Windows.

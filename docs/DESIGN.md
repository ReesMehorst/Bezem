# Bezem — UI/UX and Visual Design

> Companion to [`PLAN.md`](PLAN.md). PLAN.md sets *what* Bezem does and the rules it never breaks. This document sets *how it looks, reads and behaves* in the GUI (`bezem-gui.exe`) and the CLI (`bezem.exe`). If the two documents ever disagree, PLAN.md wins.

---

## 1. Design principles

Each product principle from PLAN.md has a direct design consequence.

| PLAN principle | Design rule | Concretely |
|---|---|---|
| **Safe by default** | Nothing destructive is one click away. | **Clean** is disabled until a scan has run. Every clean goes through a confirmation that names the totals. High-risk items are never pre-checked. |
| **Transparent** | Show the real thing, not a summary of it. | Every cleaner expands to show its real resolved paths, sizes and file counts. Every deletion is listed in the log viewer. |
| **Zero overhead** | The UI doesn't outlive its window. | No splash screen, no tray icon, no "keep running in background" option, no animations longer than 150 ms. Cold start to an interactive window in under 300 ms. |
| **Zero telemetry** | No UI needs the network. | No remote images, fonts, news feeds or "tips". The About screen says "Bezem makes no network connections." |
| **Honest** | Numbers are measured, words are plain. | Show bytes and file counts only. No "issues", "threats", "junk score", "PC health", red alarm colours for normal cache data, or countdown pressure. |

Three extra working rules:

1. **Calm by default.** Most of the screen is neutral. Colour is reserved for risk badges, the primary action, and real errors.
2. **One primary action per screen.** Scan, then Clean, then Done. The primary button is always in the same place (bottom right).
3. **The CLI and GUI tell the same story.** Same cleaner names, same categories, same risk words, same numbers, same log.

---

## 2. Brand

### 2.1 Name

*Bezem* is Dutch for **broom**. It's pronounced roughly "BAY-zum". The name implies a tidy, ordinary household job, not a rescue mission. The brand should feel like a good tool hanging on the wall: plain, sturdy, and trusted.

### 2.2 Tone of voice

| Do | Don't |
|---|---|
| "Freed 1.2 GB." | "Your PC is now 47% faster!" |
| "These files are caches. Apps rebuild them when needed." | "Junk files are slowing down your PC!" |
| "You will be signed out of most websites." | "Remove dangerous tracking cookies!" |
| "Skipped 14 files that are in use." | "14 errors found!" |
| "Bezem needs administrator rights to clean Windows Update files." | "Unlock full cleaning power!" |

- Plain, short sentences. Second person ("you"), active voice.
- Say what happens, then the consequence. No adjectives that try to sell.
- Exact numbers with units. Never round up to look better.
- No exclamation marks.
- Dutch copy uses informal *je*, not *u*, and the same plain register.

### 2.3 Logo and icon

**Concept:** a simple broom drawn from three shapes: a straight handle at about 35°, a binding band, and a fan of bristles made of 5 strokes. It sweeps a small group of 3 dots (the "data") out of the frame.

- **Construction:** geometric, 2 px stroke at 16 px, filled bristle block at 32 px and above. Rounded caps to match Fluent icons.
- **Colour:** single colour `accent` on transparent. A monochrome version (currentColor) is used in the CLI docs and the README.
- **App icon sizes:** 16, 20, 24, 32, 40, 48, 64, 256 px, in one `.ico` (PNG-compressed 256). Hand-tune 16 and 20 px: at those sizes drop the dots and keep only the broom.
- **Wordmark:** "bezem" in lowercase Segoe UI Variable Display Semibold, letter spacing -1 %, with the icon to the left at cap height × 1.4.
- **Avoid:** sparkles, shields, rockets, speedometers, magic wands. They signal "optimizer" promises Bezem doesn't make.

```
      ╲
       ╲            ·
        ╲         ·
        ▐█▌         ·
       ╱│││╲
      ╱ │││ ╲
```
*(ASCII sketch only; the real icon is an SVG in `assets/icon/`.)*

---

## 3. Visual system

### 3.1 Theme behaviour

- Default **System**: read `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize\AppsUseLightTheme` at start and listen for `WM_SETTINGCHANGE` ("ImmersiveColorSet") to switch live.
- Settings can override: **System / Light / Dark**.
- **High contrast:** when Windows High Contrast is on (`SystemParametersInfo(SPI_GETHIGHCONTRAST)`), drop the palette and use the system colours (`GetSysColor`: `COLOR_WINDOW`, `COLOR_WINDOWTEXT`, `COLOR_HIGHLIGHT`, `COLOR_HOTLIGHT`, `COLOR_GRAYTEXT`). Badges then rely on text, not colour.
- **Accent:** Bezem uses its own accent by default. A setting "Use Windows accent colour" swaps in `UISettings.GetColorValue(Accent)` (validated for ≥ 4.5:1 contrast against the surface; fall back to the brand accent if it fails).
- Window title bar follows the theme (`DwmSetWindowAttribute(DWMWA_USE_IMMERSIVE_DARK_MODE)`). Mica backdrop on Windows 11 is optional and off if it hurts contrast.

### 3.2 Colour tokens

Values follow Windows 11 Fluent neutrals so Bezem sits naturally next to Explorer and Settings. Contrast ratios are against `surface`.

| Token | Use | Light | Dark |
|---|---|---|---|
| `bg` | Window background | `#F3F3F3` | `#202020` |
| `surface` | Cards, list, dialogs | `#FFFFFF` | `#2B2B2B` |
| `surface-alt` | Expanded panel, zebra rows, code/path blocks | `#F9F9F9` | `#323232` |
| `surface-hover` | Row hover | `#F0F0F0` | `#383838` |
| `border` | Dividers, card outlines | `#E5E5E5` | `#3D3D3D` |
| `border-strong` | Inputs, checkboxes (≥ 3:1) | `#8A8A8A` | `#9A9A9A` |
| `text` | Body text | `#1B1B1B` (17.2:1) | `#FFFFFF` (14.2:1) |
| `text-muted` | Secondary text, paths, captions | `#5E5E5E` (6.5:1) | `#C8C8C8` (8.5:1) |
| `text-disabled` | Disabled labels (exempt, but kept readable) | `#A0A0A0` | `#787878` |
| `accent` | Primary button, focus ring, links, progress | `#0F6E63` (6.1:1) | `#5CC8B4` (7.0:1) |
| `accent-hover` | Primary button hover | `#0B5A51` | `#7AD6C5` |
| `on-accent` | Text on primary button | `#FFFFFF` | `#0A2622` |
| `focus` | Keyboard focus ring (2 px outer, 1 px inner `surface`) | `#1B1B1B` | `#FFFFFF` |
| `risk-low` | Low-risk badge text and dot | `#0F7B0F` (5.4:1) | `#6CCB5F` (7.0:1) |
| `risk-low-bg` | Low-risk badge fill | `#DFF6DD` | `#1F3A1C` |
| `risk-medium` | Medium-risk badge text and dot | `#8A5300` (6.3:1) | `#FCE100` (10.7:1) |
| `risk-medium-bg` | Medium-risk badge fill | `#FFF4CE` | `#433519` |
| `risk-high` | High-risk badge text and dot | `#C42B1C` (5.7:1) | `#FF99A4` (7.0:1) |
| `risk-high-bg` | High-risk badge fill | `#FDE7E9` | `#442726` |
| `info` | Info bars (e.g. "browser is running") | `#005FB8` (6.3:1) | `#99EBFF` (10.6:1) |
| `info-bg` | Info bar fill | `#E6F2FB` | `#1A2B3A` |
| `error` | Real failures only | `#C42B1C` | `#FF99A4` |

Rules:
- The accent is a calm teal ("broom-handle green"). It is **not** used for warnings.
- `risk-high` red is used for the badge and for genuine errors only. Never for sizes or totals.
- Colour is never the only signal: every badge has a word ("Low", "Medium", "High") and a shape (●, ◆, ▲).

### 3.3 Typography

**Typeface:** Segoe UI Variable (Windows 11), loaded at runtime from `C:\Windows\Fonts\SegUIVar.ttf`. It is **not** bundled (Microsoft licence). Fallback chain: Segoe UI Variable → Segoe UI (`segoeui.ttf`, Windows 10) → the toolkit's built-in font. Paths and the CLI use **Cascadia Mono** (Windows 11 / Terminal) → Consolas.

| Role | Font / optical size | Size / line height | Weight |
|---|---|---|---|
| Title (screen heading) | Segoe UI Variable Display | 28 / 36 px | Semibold |
| Subtitle (summary total) | Segoe UI Variable Display | 20 / 28 px | Semibold |
| Body strong (cleaner name, category) | Segoe UI Variable Text | 14 / 20 px | Semibold |
| Body | Segoe UI Variable Text | 14 / 20 px | Regular |
| Caption (descriptions, badges, paths) | Segoe UI Variable Small | 12 / 16 px | Regular |
| Mono (paths, log) | Cascadia Mono | 12 / 16 px | Regular |

- Numbers in tables use tabular figures (`tnum`) so sizes line up. Right-align sizes.
- Sizes use binary units with the familiar Windows labels: `1.2 GB` means 1.2 × 1024³ bytes, matching what Explorer shows. One decimal from MB up, none below. `0 B` is shown as "Nothing to clean".
- Body text never goes below 12 px at 100 % scaling.

### 3.4 Spacing, shape and motion

**Spacing scale (4 px base):** `xxs 2` · `xs 4` · `s 8` · `m 12` · `l 16` · `xl 24` · `xxl 32` · `xxxl 48`.

| Element | Value |
|---|---|
| Window padding | `xl` 24 |
| Gap between category cards | `l` 16 |
| Row height (cleaner) | 40 px (comfortable), 32 px (compact setting) |
| Row inner padding | `m` 12 horizontal |
| Checkbox / badge gap | `s` 8 |
| Corner radius | 4 px controls, 8 px cards and dialogs |
| Minimum hit target | 32 × 32 px (40 × 40 recommended) |
| Minimum window size | 720 × 520 logical px; default 960 × 680 |

**Motion:** expand/collapse 120 ms ease-out; nothing else animates except the progress bar. All motion is off when Windows "Animation effects" is off (`SPI_GETCLIENTAREAANIMATION`).

**Iconography:** Segoe Fluent Icons (system font on Windows 11; Segoe MDL2 Assets on Windows 10) for chevrons, settings, info, warning and shield-elevation glyphs. Category icons: System `\uE7F8` (PC), Browsers `\uE774` (globe), Privacy `\uE72E` (lock), Apps `\uE71D` (grid).

---

## 4. Information architecture

```
Main window
├── Header: logo · "Bezem" · [Log] [Settings] [About]
├── Cleaner list (always visible)
│   ├── System
│   ├── Browsers  (sub-grouped per browser, then per profile)
│   ├── Privacy
│   └── Apps      (v1.x)
├── Detail pane / expandable row: "What does this remove?"
└── Action bar: selection total · [Scan] · [Clean…]

Modal flows (on top of the main window)
├── Confirmation dialog
├── Cleaning progress
├── Summary
├── Elevation prompt
└── Browser-is-running prompt

Secondary screens (replace the list area; Esc returns)
├── Settings
├── Log viewer
└── About
```

**Screen inventory**

| # | Screen | Reached from | Primary action |
|---|---|---|---|
| S1 | Main window (pre-scan) | Launch | Scan |
| S2 | Main window (scan results) | After scan | Clean… |
| S3 | Confirmation dialog | Clean… | Clean now |
| S4 | Cleaning progress | Clean now | Cancel (secondary) |
| S5 | Summary | Finish | Done |
| S6 | Settings | Header | (auto-saves) |
| S7 | Log viewer | Header, Summary | Open log folder |
| S8 | About | Header, F1 | — |
| D1 | Privacy opt-in warning | Ticking a privacy/high item | Include |
| D2 | Elevation prompt | Scan/Clean with admin items | Continue as administrator |
| D3 | Browser is running | Scan/Clean of a browser cleaner | Wait / Skip |

---

## 5. Wireframes

Widths are illustrative (≈ 960 px). `[x]` checked, `[ ]` unchecked, `[-]` partial, `▸` collapsed, `▾` expanded.

### S1 — Main window, before the first scan

```
┌──────────────────────────────────────────────────────────────────────────┐
│ ⌂ bezem                                          [Log]  [Settings]  [ⓘ] │
├──────────────────────────────────────────────────────────────────────────┤
│  Choose what to clean                                                    │
│  Nothing is deleted until you review a scan and confirm.                 │
│                                                                          │
│  ┌ ▾ [-] System ─────────────────────────────────────────────────────┐   │
│  │   ▸ [x] User temporary files               ● Low                  │   │
│  │   ▸ [x] Thumbnail cache                    ● Low                  │   │
│  │   ▸ [x] Windows Error Reporting            ● Low                  │   │
│  │   ▸ [ ] Windows Update downloads   🛡       ● Low                  │   │
│  │   ▸ [ ] DirectX shader cache               ◆ Medium               │   │
│  │   ▸ [x] Recycle Bin                        ● Low                  │   │
│  └───────────────────────────────────────────────────────────────────┘   │
│  ┌ ▾ [-] Browsers ───────────────────────────────────────────────────┐   │
│  │   ▾ Microsoft Edge · Profile "Work"                               │   │
│  │       ▸ [x] Cache                          ● Low                  │   │
│  │       ▸ [ ] Cookies                        ▲ High · opt-in        │   │
│  │       ▸ [ ] History                        ◆ Medium · opt-in      │   │
│  │   ▸ Firefox · default-release                                     │   │
│  └───────────────────────────────────────────────────────────────────┘   │
│  ┌ ▸ [ ] Privacy ──────────────────────── 4 cleaners · all opt-in ───┐   │
│  └───────────────────────────────────────────────────────────────────┘   │
├──────────────────────────────────────────────────────────────────────────┤
│  6 cleaners selected · 🛡 = needs administrator      [ Scan ]  [Clean…]  │
│                                                       primary   disabled │
└──────────────────────────────────────────────────────────────────────────┘
```

- `🛡` is the Windows shield glyph (`\uEA18`), shown only on cleaners that need admin rights.
- Before a scan there is no size column. No guessed or "estimated" numbers.
- Disabled **Clean…** has a tooltip: "Run a scan first so you can see what will be removed."

### S1a — Expanded "What does this remove?" panel

```
│   ▾ [x] User temporary files                 ● Low                     │
│   ┌───────────────────────────────────────────────────────────────────┐│
│   │ Files apps leave in your temp folder. Apps recreate them when     ││
│   │ needed. Files changed in the last 24 hours are skipped.           ││
│   │                                                                   ││
│   │ Removes from                                                      ││
│   │   C:\Users\mees\AppData\Local\Temp\**          (older than 24 h)  ││
│   │ Keeps                                                             ││
│   │   *.lock, files in use                                            ││
│   │ Delete mode   Permanent  (change in Settings)                     ││
│   │                                                                   ││
│   │ [View definition]   [Open folder]                                 ││
│   └───────────────────────────────────────────────────────────────────┘│
```

- Paths are **resolved** (environment variables expanded), shown in mono, selectable and copyable.
- **View definition** shows the TOML read-only (same as `bezem show <id>`).
- After a scan, the panel adds the largest 10 items and a "Show all 1,284 files…" link to the log-style file list.

### S2 — Main window, scan results

```
┌──────────────────────────────────────────────────────────────────────────┐
│ ⌂ bezem                                          [Log]  [Settings]  [ⓘ] │
├──────────────────────────────────────────────────────────────────────────┤
│  Scan results                                       Scanned 0.9 s ago    │
│  2.4 GB can be freed from 6 cleaners. Nothing has been deleted yet.      │
│                                                                          │
│  ┌ ▾ [-] System ─────────────────────────────────── 1.9 GB  4,210 files ┐│
│  │   ▸ [x] User temporary files     ● Low          812 MB  3,902 files  ││
│  │   ▸ [x] Thumbnail cache          ● Low           96 MB     14 files  ││
│  │   ▸ [x] Windows Error Reporting  ● Low          410 MB     31 files  ││
│  │   ▸ [ ] Windows Update downloads 🛡 ● Low       Not scanned (admin)  ││
│  │   ▸ [x] Recycle Bin              ● Low          620 MB    263 files  ││
│  └──────────────────────────────────────────────────────────────────────┘│
│  ┌ ▾ [-] Browsers ─────────────────────────────── 512 MB  9,840 files ──┐│
│  │   ▸ [x] Edge · Work · Cache      ● Low          498 MB  9,811 files  ││
│  │   ▸ [x] Firefox · Cache          ● Low           14 MB     29 files  ││
│  │       ⓘ Firefox is open. Close it to clean its cache.  [Wait] [Skip] ││
│  └──────────────────────────────────────────────────────────────────────┘│
├──────────────────────────────────────────────────────────────────────────┤
│  Selected: 2.4 GB · 14,050 files        [ Rescan ]   [ Clean 2.4 GB… ]   │
└──────────────────────────────────────────────────────────────────────────┘
```

- Rows that found nothing show "Nothing to clean" in `text-muted`, not hidden.
- Sorting: category order is fixed; inside a category, the definition order (stable, so muscle memory works). An optional "Sort by size" toggle sits in the list header.
- Changing a checkbox updates the selected total live. Ticking a cleaner that wasn't in the last scan marks it "Not scanned" and the button becomes **Rescan** until it is scanned.
- Scan results older than 10 minutes show "Results may be out of date · Rescan".

### S3 — Confirmation dialog

```
        ┌──────────────────────────────────────────────────────┐
        │  Clean 2.4 GB?                                       │
        │                                                      │
        │  14,050 files from 6 cleaners will be removed.       │
        │                                                      │
        │   Permanently deleted     1.8 GB   (caches, temp)    │
        │   Moved to Recycle Bin    0 B                        │
        │   Recycle Bin emptied     620 MB   (can't be undone) │
        │                                                      │
        │  ⚠ Includes Edge cookies: you will be signed out     │
        │    of most websites in Edge (Work profile).          │
        │                                                      │
        │  Files in use are skipped, never forced.             │
        │                                                      │
        │                     [ Cancel ]   [ Clean now ]       │
        └──────────────────────────────────────────────────────┘
```

- Default focus is on **Cancel**. Enter does *not* trigger Clean; Alt+N or Tab then Enter does.
- Consequence lines appear only for selected items that have one (cookies, history, Recycle Bin).
- "Don't ask again" is offered **only** when the selection is entirely low-risk caches, and it never applies to privacy items.

### S4 — Cleaning progress

```
        ┌──────────────────────────────────────────────────────┐
        │  Cleaning…                                           │
        │                                                      │
        │  ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░   1.5 GB of 2.4 GB  │
        │                                                      │
        │  ✓ User temporary files           798 MB             │
        │  ✓ Thumbnail cache                 96 MB             │
        │  ● Windows Error Reporting        215 MB …           │
        │  ○ Recycle Bin                                       │
        │  ○ Edge · Work · Cache                               │
        │                                                      │
        │  C:\ProgramData\Microsoft\Windows\WER\ReportArchive… │
        │                                                      │
        │                                       [ Stop ]       │
        └──────────────────────────────────────────────────────┘
```

- Progress is by bytes, not files, so it doesn't jump. If totals are unknown (Recycle Bin via shell API) the bar is indeterminate for that step only.
- Current path is shown in `text-muted` mono, truncated in the middle (`C:\Program…\ReportArchive\x.wer`), updated at most 10×/s.
- **Stop** finishes the current file, then stops. The summary then says "Stopped by you".
- Closing the window during cleaning asks "Stop cleaning? Files already removed stay removed."

### S5 — Summary

```
┌──────────────────────────────────────────────────────────────────────────┐
│  Freed 2.3 GB                                                            │
│  13,912 files removed in 4.1 s.                                          │
│                                                                          │
│   ✓ User temporary files            798 MB    3,880 files                │
│   ✓ Thumbnail cache                  96 MB       14 files                │
│   ✓ Windows Error Reporting         410 MB       31 files                │
│   ✓ Recycle Bin                     620 MB      263 files                │
│   ✓ Edge · Work · Cache             412 MB    9,724 files                │
│   – Skipped 138 files (86 MB) that were in use or changed since the scan │
│                                                         [Show skipped]   │
│                                                                          │
│  A list of every removed file is in the log.                             │
│                                                                          │
│                                  [ Open log ]          [ Done ]          │
└──────────────────────────────────────────────────────────────────────────┘
```

- The headline is the **measured** bytes removed, which can be lower than the scan. That's fine and stated.
- No "Your PC is clean!" message, no confetti, no "Rate us", no share button.
- **Done** returns to S1 with the same selection kept.

### S6 — Settings

```
┌──────────────────────────────────────────────────────────────────────────┐
│ ← Settings                                                               │
├──────────────────────────────────────────────────────────────────────────┤
│  Deleting                                                                │
│   Caches and temp files    (•) Delete permanently  ( ) Move to Recycle Bin│
│   Privacy items            ( ) Delete permanently  (•) Move to Recycle Bin│
│   Skip temp files newer than   [ 24 ▾] hours                             │
│                                                                          │
│  Exclusions                                                              │
│   Paths Bezem never touches, even if a cleaner includes them.            │
│   ┌────────────────────────────────────────────────┐                     │
│   │ C:\Users\mees\AppData\Local\Temp\keep-me\      │ [Remove]            │
│   └────────────────────────────────────────────────┘ [Add folder…]       │
│                                                                          │
│  Saved selections                                                        │
│   [ Weekly tidy ▾ ]  [Save current]  [Rename]  [Delete]                  │
│                                                                          │
│  Appearance                                                              │
│   Theme        (•) Same as Windows  ( ) Light  ( ) Dark                  │
│   Accent       [ ] Use Windows accent colour                             │
│   Density      (•) Comfortable  ( ) Compact                              │
│   Language     [ Same as Windows ▾ ]   (English, Nederlands)             │
│                                                                          │
│  Updates                                                                 │
│   [ ] Check GitHub for new versions when Bezem starts                    │
│       Off by default. This is the only network request Bezem can make.  │
│                                                                          │
│  Settings are stored in %APPDATA%\Bezem\settings.toml   [Open folder]    │
└──────────────────────────────────────────────────────────────────────────┘
```

- Settings save on change; no OK/Apply. "Reset to defaults" sits at the bottom with its own confirmation.
- The Updates group only exists if PLAN open decision #4 ships an opt-in check.

### S7 — Log viewer

```
┌──────────────────────────────────────────────────────────────────────────┐
│ ← Log                                        [Open log folder]  [Copy]   │
├──────────────────────────────────────────────────────────────────────────┤
│  Session  [ 28 Oct 2026 14:02 · Freed 2.3 GB ▾ ]   Filter [ all ▾ ] [🔍 ]│
├──────────────────────────────────────────────────────────────────────────┤
│  Time      Action    Size     Path                                       │
│  14:02:11  deleted   12 MB    C:\Users\mees\AppData\Local\Temp\a1.tmp    │
│  14:02:11  deleted   3 KB     C:\Users\mees\AppData\Local\Temp\b7.log    │
│  14:02:11  skipped   —        …\Temp\setup.lock         in use           │
│  14:02:12  skipped   —        …\Temp\junction-x         reparse point    │
│  14:02:12  recycled  1.1 MB   …\Edge\User Data\Work\History              │
│  …                                                                       │
├──────────────────────────────────────────────────────────────────────────┤
│  13,912 deleted · 138 skipped · 0 failed               Page 1 of 140     │
└──────────────────────────────────────────────────────────────────────────┘
```

- Filters: all / deleted / recycled / skipped / failed. Search matches path substrings.
- Virtualized list (only visible rows rendered) so 100k-line logs stay smooth.
- Each skip shows the reason in plain words: "in use", "changed since scan", "reparse point (not followed)", "outside allowed folders", "access denied".

### S8 — About

```
        ┌──────────────────────────────────────────────────────┐
        │   [broom icon]   bezem 1.0.0                         │
        │                  Dutch for "broom".                  │
        │                                                      │
        │   Sweeps temporary data from Windows: safely,        │
        │   transparently, with zero overhead.                 │
        │                                                      │
        │   ✓ No telemetry. Bezem makes no network connections.│
        │   ✓ No background service, no auto-start.            │
        │   ✓ Open source under the MIT licence.               │
        │                                                      │
        │   Source code · Report a problem · Licences          │
        │   Build 3f9c1e2 · SHA-256 of this exe [Copy]         │
        │                                                      │
        │                                       [ Close ]      │
        └──────────────────────────────────────────────────────┘
```

- Links open in the default browser (Bezem itself makes no request). "Licences" lists third-party crates, generated by `cargo about`.

---

## 6. User flows

### F1 — First run

```
Launch ─► Main window (S1), defaults pre-checked (low risk only)
       ─► One-time info bar at the top:
          "Bezem shows you what it will remove before it removes anything.
           Start with Scan."                                   [Got it]
```

- No wizard, no EULA screen, no account, no "optimize now" auto-scan.
- Defaults: everything with `default_enabled = true` and `risk = low`. Nothing that needs admin is pre-checked unless already elevated.
- The info bar is dismissed forever once closed (stored in settings).

### F2 — Scan → review → clean

```
S1 ──Scan──► scanning (inline progress per row, list stays usable)
   ──► S2 results ──(adjust ticks; expand rows; see paths)──► Clean…
   ──► S3 confirm ──Clean now──► S4 progress ──► S5 summary ──Done──► S1
                └──Cancel──► S2 (nothing changed)
```

- Scanning shows a small spinner and live size per row; the button reads **Stop scan**.
- Unticking all items disables **Clean…** with the label "Select something to clean".

### F3 — Privacy opt-in with warning (D1)

Triggered the first time in a session the user ticks a `privacy` cleaner or any `risk = high` item.

```
        ┌──────────────────────────────────────────────────────┐
        │  Include Edge cookies?                               │
        │                                                      │
        │  Cookies keep you signed in to websites and remember │
        │  site settings. After cleaning:                      │
        │   • you will be signed out of most websites in Edge  │
        │   • sites may ask for cookie consent again           │
        │   • two-step sign-in may ask to verify this device   │
        │                                                      │
        │  Items go to the Recycle Bin (change in Settings).   │
        │                                                      │
        │                     [ Cancel ]   [ Include cookies ] │
        └──────────────────────────────────────────────────────┘
```

- Specific consequences per item, from the cleaner definition (`warning` field). No generic "Are you sure?".
- The tick stays off if cancelled. The same warning reappears in the confirmation dialog (S3).
- Saved selections that include privacy items still show the S3 consequence lines every time.

### F4 — Elevation prompt (D2)

```
User ticks a 🛡 cleaner and presses Scan
  ─► D2: "2 selected cleaners need administrator rights"
        Windows temp files, Windows Update downloads.
        Bezem will restart these two with administrator rights.
        The rest run as you.
        [ Skip these 2 ]   [ Continue as administrator 🛡 ]
  ─► Continue ─► Windows UAC prompt ─► elevated helper scans/cleans only those
  ─► results merged into S2; rows marked 🛡
  ─► UAC declined ─► rows show "Not scanned: administrator rights declined" + [Try again]
```

- Never elevate the whole app at launch. Never re-prompt on its own.
- If the app is already elevated, the header shows a small "Administrator" label so the user knows.

### F5 — Browser is running (D3)

```
Scan or Clean includes Firefox ─► Firefox process detected
  ─► inline info bar on the Firefox group (not a modal during scan):
     "Firefox is open. Close it to clean its data, or skip it this time."
     [ Wait for Firefox to close ]  [ Skip Firefox ]
  ─► Wait: row shows "Waiting for Firefox to close…" and continues automatically
  ─► Skip: row shows "Skipped: Firefox was open"
```

- Bezem **never** closes or kills a browser. No "Close it for me" button.
- Scanning a running browser's cache is allowed (read-only); only cleaning waits.

### F6 — Errors and locked files

| Situation | Where shown | Wording |
|---|---|---|
| File in use | Summary skipped line, log | "Skipped 138 files (86 MB) that were in use or changed since the scan." |
| Access denied (unelevated) | Row + summary | "Skipped 3 files: Windows didn't allow access." |
| Reparse point found | Log only (info) | "Not followed: junction pointing outside the allowed folder." |
| Path outside allowlist (bad definition) | Row error, cleaner disabled | "This cleaner was turned off: it points outside the folders Bezem is allowed to clean." |
| Definition fails validation | Row error | "Couldn't load this cleaner: line 12, unknown field 'pth'." |
| Disk read error during scan | Row | "Couldn't finish scanning this folder. Partial result shown." |
| Unexpected crash | Native dialog on next start | "Bezem closed unexpectedly last time. No files were deleted after the moment shown in the log." [Open log] |

- Skips are **normal outcomes**, shown in `text-muted`, not red. Red is for things that went wrong.
- Errors always say what Bezem did about it ("skipped", "turned off"), and what the user can do if anything.

---

## 7. Microcopy

### 7.1 Buttons and labels

| Context | EN | NL |
|---|---|---|
| Scan | Scan | Scannen |
| Clean (with total) | Clean 2.4 GB… | 2,4 GB opruimen… |
| Confirm | Clean now | Nu opruimen |
| Stop | Stop | Stoppen |
| Done | Done | Klaar |
| Expand row | What does this remove? | Wat wordt er verwijderd? |
| Risk badges | Low · Medium · High | Laag · Middel · Hoog |
| Opt-in tag | opt-in | zelf aanzetten |

### 7.2 Results

- "Freed 1.2 GB." · "Vrijgemaakt: 1,2 GB."
- "1.2 GB can be freed. Nothing has been deleted yet."
- "Nothing to clean."  (not "Your PC is spotless!")
- "Freed 412 MB. 3 files were skipped because they were in use."
- "Stopped by you after 1.1 GB. Files already removed stay removed."

### 7.3 Cleaner descriptions (pattern: *what it is* + *what happens after*)

- **Thumbnail cache:** "Small previews of pictures and videos in Explorer. Windows rebuilds them, so folders may load previews more slowly the first time."
- **Browser cache:** "Copies of web pages and images. Sites may load a little slower the first time you visit them again."
- **History:** "The list of sites you visited. Address bar suggestions based on history will be gone."
- **Windows Update downloads:** "Installer files Windows already used. Bezem pauses the Windows Update service while cleaning and starts it again afterwards."
- **Recycle Bin:** "Files you already deleted. After this, they can't be restored from the Recycle Bin."
- **DNS cache:** "Windows' list of recently looked-up website addresses. It refills automatically. Frees no disk space."

### 7.4 Forbidden words and patterns

`junk` (as a noun for user data) · `threat` · `issue(s) found` · `boost` · `speed up` · `optimize your PC` · `critical` (for non-critical things) · `infected` · `health score` · fake countdowns · "Recommended!" badges on paid things (there are none) · pre-checked privacy items.

---

## 8. Accessibility

Target: **WCAG 2.2 AA** for everything the GUI shows.

### 8.1 Keyboard

| Key | Action |
|---|---|
| Tab / Shift+Tab | Move between regions: header → list → action bar |
| ↑ / ↓ | Move between rows in the list |
| → / ← | Expand / collapse a category or "what does this remove" panel; ← on a child jumps to its parent |
| Space | Toggle the focused checkbox (category toggles all children) |
| Home / End | First / last row |
| F5 or Ctrl+R | Scan / Rescan |
| Ctrl+Enter | Clean… (opens confirmation; never cleans directly) |
| Esc | Close dialog / back from secondary screen / stop scan |
| Ctrl+, | Settings |
| Ctrl+L | Log viewer |
| F1 | About and keyboard shortcuts |
| Alt + underlined letter | Dialog buttons (mnemonics shown when Alt is held) |

- Focus is always visible: 2 px `focus` ring with a 1 px `surface` gap, never removed on mouse use for the list.
- Focus order follows visual order. Opening a dialog moves focus into it; closing it returns focus to the control that opened it.
- No keyboard traps; every dialog closes with Esc (= Cancel).

### 8.2 Screen readers (AccessKit → UI Automation)

- Every control exposes **role, name, state, value**:
  - Category: role `TreeItem`, name "System, 4 of 6 selected, 1.9 GB", state expanded/collapsed, mixed checkbox state.
  - Cleaner row: role `TreeItem` with check state, name "User temporary files, low risk, 812 megabytes, 3,902 files".
  - Badges are part of the row name, not separate focus stops.
- **Live regions** (polite): scan completion ("Scan finished. 2.4 gigabytes can be freed."), cleaning progress at 25 % steps, summary headline. Errors use assertive.
- Sizes are read with full unit words ("megabytes"), via a separate accessible label.
- Test with **Narrator** and **NVDA** before each release; the checklist is in `docs/ACCESSIBILITY-CHECKLIST.md` (to be written in M6).

### 8.3 Contrast, colour and motion

- Text ≥ 4.5:1, large text and UI component boundaries (checkbox, input, focus ring) ≥ 3:1. Token table in §3.2 lists the ratios; CI checks them with a small script over the token file.
- Risk is never colour-only (word + shape).
- Respect "Animation effects" off, and Windows "Text size" (Settings → Accessibility → Text size) as an extra text scale factor on top of DPI.

### 8.4 DPI scaling

- Per-Monitor DPI Aware v2 (manifest `dpiAwareness = PerMonitorV2`). Re-layout on `WM_DPICHANGED` when the window moves between monitors.
- All sizes in this document are logical px (100 % = 96 DPI). Verify at 100, 125, 150, 200 and 300 %.
- Icons: vector (font glyphs or SVG); the app `.ico` contains all listed sizes.
- At 200 % text scaling the layout reflows: size columns move under the cleaner name; no horizontal scrolling in the list at the minimum window size.

---

## 9. Localization

- **Languages at 1.0:** English (source) and Dutch. Language follows Windows (`GetUserPreferredUILanguages`), overridable in Settings.
- **Format:** [Project Fluent](https://projectfluent.org/) (`fluent-rs`), one `.ftl` file per language under `crates/bezem-gui/locales/` and shared CLI strings in `crates/bezem-cli/locales/`. Fluent handles plurals ("1 file" / "2 files", "1 bestand" / "2 bestanden").
- **Cleaner text** (`name`, `description`, `warning`) lives in the TOML definitions with per-language keys, e.g. `name.nl = "Tijdelijke gebruikersbestanden"`. Missing translation → English, never a key ID.
- **Numbers and units:** locale-aware separators (`1.2 GB` EN, `1,2 GB` NL; `3,902` EN, `3.902` NL). Dates use the Windows short date/time format.
- **Text expansion:** design for **+35 %** (Dutch runs ~20–35 % longer than English, and compounds can't wrap: *Foutrapportagearchieven*). Rules:
  - Buttons size to content with min width 96 px; never fixed widths.
  - Cleaner names wrap to two lines before they truncate; truncation uses an ellipsis plus a tooltip with the full text.
  - Test with a pseudo-locale (`en-XA`: accented and padded +40 %, e.g. `[Ŝçåñ ·····]`) in CI screenshots.
- **Not translated:** paths, cleaner IDs, CLI flags and subcommands, JSON keys, log file format (log *reasons* are stable English codes plus a translated display text in the viewer).
- RTL is not planned for 1.x, but don't hard-code left/right in layout code where a "start/end" helper is as easy.

---

## 10. CLI design

The CLI ships first (M3) and sets the vocabulary. It follows [clig.dev](https://clig.dev) conventions.

### 10.1 Output principles

- **stdout** is for results (tables, JSON). **stderr** is for progress, prompts, warnings and errors. Piping `bezem scan > out.txt` never captures a spinner.
- **Colour** only when stdout is a TTY and none of these is set: `NO_COLOR` (any value), `TERM=dumb`, `--color never`. `--color always|auto|never`, default `auto`. Enable VT processing on Windows consoles (`ENABLE_VIRTUAL_TERMINAL_PROCESSING`); if that fails, no colour.
- **Colour roles** (ANSI 16-colour so they follow the terminal theme): bold for headings and totals; green = Low, yellow = Medium, red = High *and* real errors; dim for paths and skipped counts; cyan for the elevation 🛡 marker substitute `[admin]`. Never colour-only: badges are words.
- **Unicode:** use `✓ – ✗` and box drawing only when the console code page is UTF-8 (default in Windows Terminal); otherwise ASCII `OK - x` and `-`/`|`.
- **Width:** detect terminal width; paths are middle-truncated to fit. `--wide` disables truncation.
- **Sizes:** same formatting as the GUI (`1.2 GB`). `--bytes` prints raw byte counts for scripts.

### 10.2 `bezem list`

```
$ bezem list
CATEGORY  ID                         NAME                         RISK    DEFAULT  ADMIN
system    windows.user_temp          User temporary files         low     on
system    windows.temp               Windows temporary files      low     on       yes
system    windows.update_cache       Windows Update downloads     low     off      yes
system    windows.shader_cache       DirectX shader cache         medium  off
browser   edge.work.cache            Edge (Work) · Cache          low     on
browser   edge.work.cookies          Edge (Work) · Cookies        high    off
privacy   windows.recent_docs        Recent documents             medium  off

7 cleaners · 3 on by default · run `bezem show <id>` for details
```

### 10.3 `bezem scan`

```
$ bezem scan
Scanning 6 cleaners… done in 0.9 s

CLEANER                          RISK        SIZE     FILES
System
  User temporary files           low       812 MB     3,902
  Thumbnail cache                low        96 MB        14
  Windows Error Reporting        low       410 MB        31
  Recycle Bin                    low       620 MB       263
  Windows Update downloads       low            –         –   skipped: needs admin
Browsers
  Edge · Work · Cache            low       498 MB     9,811
  Firefox · Cache                low        14 MB        29   note: Firefox is open
──────────────────────────────────────────────────────────────
Total                                     2.4 GB    14,050

Nothing was deleted. Run `bezem clean` to remove these files.
Skipped 1 cleaner that needs admin. Run from an elevated terminal, or `bezem clean --elevate`.
```

Progress (stderr, TTY only) while scanning — one updating line, no scrolling spam:

```
⠹ Scanning Edge · Work · Cache  1.7 GB found  (5/7)
```

### 10.4 `bezem clean` — confirmation prompt

```
$ bezem clean --only system,browser
Scanning… done in 0.8 s

  Permanently delete     1.8 GB   13,787 files   (4 cleaners)
  Empty Recycle Bin      620 MB      263 files   can't be undone

Clean 2.4 GB? [y/N] y
Cleaning  [██████████████████░░░░░░░░░░]  1.5 GB / 2.4 GB  Windows Error Reporting

✓ Freed 2.3 GB (13,912 files) in 4.1 s.
– Skipped 138 files (86 MB) that were in use or changed since the scan.
Log: C:\Users\mees\AppData\Local\Bezem\logs\2026-10-28T14-02-11.jsonl
```

Prompt rules:
- Default answer is **No** (`[y/N]`). Anything except `y`/`yes` (or `j`/`ja` in NL) cancels.
- Privacy/high-risk items get their own line-by-line consequence before the prompt:
  `! Edge (Work) · Cookies: you will be signed out of most websites in Edge.`
- **No TTY on stdin and no `--yes`** → refuse with exit code 4 and the message "Refusing to clean without confirmation. Add --yes to run unattended."
- `--yes` never enables opt-in cleaners by itself; they must be named with `--only` or `--include`.
- `--dry-run` on `clean` is equivalent to `scan` with the same selection.
- Ctrl+C during cleaning: finish the current file, print the partial summary, exit 130.

### 10.5 Exit codes (proposed; final list lives in the CLI reference)

| Code | Meaning |
|---|---|
| 0 | Success (including "nothing to clean") |
| 1 | Error (I/O, definition, internal) |
| 2 | Usage error (bad flag, unknown cleaner ID) |
| 3 | Completed with skips (locked files, browser open, admin declined) |
| 4 | Not confirmed (user said no, or no TTY without `--yes`) |
| 5 | Needs elevation and `--elevate` not given / UAC declined |
| 130 | Interrupted (Ctrl+C) |

### 10.6 `--json` schema

- One JSON document on stdout (or JSON Lines with `--json-lines` for streaming progress events). Nothing else goes to stdout in JSON mode; human messages still go to stderr unless `--quiet`.
- Sizes are always **raw bytes** (integers). Paths are absolute, with `\` escaped. Timestamps are RFC 3339 UTC.
- `schema_version` is bumped on breaking changes only; new fields may appear anytime.

```json
{
  "schema_version": 1,
  "bezem_version": "1.0.0",
  "command": "scan",
  "started_at": "2026-10-28T13:02:10Z",
  "duration_ms": 912,
  "elevated": false,
  "dry_run": true,
  "totals": { "bytes": 2582319104, "files": 14050, "cleaners": 6 },
  "cleaners": [
    {
      "id": "windows.user_temp",
      "name": "User temporary files",
      "category": "system",
      "risk": "low",
      "requires_admin": false,
      "status": "scanned",
      "delete_mode": "permanent",
      "bytes": 851443712,
      "files": 3902,
      "roots": ["C:\\Users\\mees\\AppData\\Local\\Temp"],
      "largest": [
        { "path": "C:\\Users\\mees\\AppData\\Local\\Temp\\setup_x64.msi", "bytes": 214958080 }
      ],
      "skipped": []
    },
    {
      "id": "windows.update_cache",
      "name": "Windows Update downloads",
      "category": "system",
      "risk": "low",
      "requires_admin": true,
      "status": "skipped",
      "skip_reason": "needs_admin",
      "bytes": null,
      "files": null
    },
    {
      "id": "firefox.default-release.cache",
      "name": "Firefox · Cache",
      "category": "browser",
      "risk": "low",
      "requires_admin": false,
      "status": "scanned",
      "notes": ["app_running:firefox"],
      "bytes": 14680064,
      "files": 29
    }
  ]
}
```

For `clean --json`, each cleaner adds `"removed": {"bytes", "files"}`, `"skipped": [{"path", "reason"}]` (capped at 100 with `"skipped_truncated": true`), and the top level adds `"log_path"`. `status` is one of `scanned | cleaned | partial | skipped | failed`; `skip_reason` / `reason` is one of `needs_admin | app_running | in_use | changed_since_scan | reparse_point | outside_allowlist | access_denied | user_cancelled`. The same reason codes are used in the log file and the GUI log viewer.

### 10.7 Help text

`bezem --help` fits on one screen (≤ 24 lines), lists commands with one-line summaries, and ends with two real examples. `bezem <cmd> --help` lists flags with defaults. Help is localized; flags are not.

---

## 11. GUI toolkit: egui vs Slint (design view)

PLAN.md open decision #1. This section looks only at design and UX consequences.

| Criterion | egui / eframe | Slint |
|---|---|---|
| Native Windows look | Custom; we draw everything. Needs a theme layer to match Fluent (tokens in §3.2 map directly onto `egui::Visuals`/`Style`). | Built-in `fluent` style gives Windows 11-like controls out of the box. |
| Accessibility | AccessKit is integrated and on by default (UIA on Windows). Good coverage for buttons, checkboxes, text; tree semantics need care. | Accessibility via AccessKit as well; declarative properties (`accessible-role`, `accessible-label`) make it explicit per element. |
| Text rendering | Own rasterizer; no ClearType subpixel AA. Fine at 125 %+, slightly soft at 100 %. Loading Segoe UI Variable from system fonts works; variable axes (optical size) are not used, so pick static instances. | Better text shaping and font fallback (via its renderers); also no ClearType. Handles system fonts more naturally. |
| Layout for long Dutch strings | Immediate-mode; wrapping and reflow need manual care, especially in tables. | Declarative layouts (`HorizontalLayout`, `GridLayout`) handle expansion and min-sizes more predictably. |
| Large lists (log viewer) | `ScrollArea::show_rows` virtualizes well. | `ListView` virtualizes well. |
| Iteration on visuals | Rust code only; theme tweaks need a rebuild. | `.slint` markup with live preview; designers can contribute without Rust. |
| Binary size / idle cost | Smallest; redraws only on input when configured (`reactive` mode), so idle CPU is ~0. | Slightly larger; also event-driven and idle at ~0. |
| Licence fit | MIT/Apache-2.0: clean with `cargo deny`. | GPLv3, or Slint's royalty-free licence, or commercial. **Not MIT/Apache; needs an explicit `cargo deny` exception and a licence review** before it can be chosen under PLAN decision #3. |

**Recommendation: egui**, with a thin `bezem-gui::theme` module that turns the tokens in §3.2 into `egui::Visuals` and a small set of custom widgets (risk badge, tri-state tree checkbox, info bar, primary/secondary button). Reasons:

1. The UI is one window with a tree, a few dialogs and a log table. It doesn't need a full native widget set; a consistent custom theme is enough and is easier to keep honest and calm.
2. AccessKit is first-class in egui, which covers the hard requirement in §8.
3. It keeps the dependency tree inside the MIT-compatible licence policy without exceptions.

Risks to manage if egui is chosen: (a) build the tri-state tree with explicit AccessKit roles and test with Narrator early (M6 week 1); (b) do a 100 % DPI text-sharpness check with Segoe UI Variable; (c) write layout helpers for wrap-before-truncate so Dutch strings don't overflow.

Choose **Slint** instead only if the licence review passes *and* a native Fluent look turns out to matter more than the above (for example, if user testing shows the custom theme feels "untrustworthy" or out of place next to Windows apps).

---

## 12. Design deliverables and checks

| Deliverable | Milestone |
|---|---|
| `assets/icon/bezem.svg` + `.ico` (all sizes) | M6 |
| `crates/bezem-gui/src/theme.rs` with the §3.2 tokens and a contrast test | M6 |
| CLI output snapshots (`insta`) for `list`, `scan`, `clean`, `--json`, with and without `NO_COLOR` | M3 |
| GUI screenshots of S1–S8 in light, dark, high contrast, 100 % and 200 %, EN/NL/pseudo-locale | M6–M7 |
| Narrator + NVDA pass on the full F2 flow | M6 |
| Copy review against §7.4 forbidden list (grep in CI over `.ftl` and `cleaners/*.toml`) | M3 onward |

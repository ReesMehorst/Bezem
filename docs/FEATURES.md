# Bezem: Feature Specification

> Companion to [PLAN.md](PLAN.md). PLAN.md is the master plan; if this document contradicts it, PLAN.md wins and this file gets fixed.
> Status: draft for v1.0 (milestones M0–M7) plus the committed post-1.0 backlog.

## 0. How to read this document

Every feature has a short ID, a summary row, and a detail block.

| Field | Meaning |
|---|---|
| **ID** | `F-<AREA>-<NN>`. IDs are stable; retired IDs are never reused. |
| **Cleaner ID** | For cleaners: the `id` in the TOML definition (`cleaners/*.toml`). |
| **Risk** | `low` = only regenerable data; `medium` = regenerable but with a noticeable cost (re-login, slower first run, lost UI state); `high` = user-visible data loss possible, **always opt-in**. For non-cleaner features: the impact if the feature is wrong. |
| **Admin** | Yes = needs an elevated process (see F-ELV). |
| **Default** | On = pre-selected in a fresh install / plain `bezem clean`. Off = the user must select it. High-risk items are always Off. |
| **MoSCoW** | **M**ust / **S**hould / **C**ould / **W**on't (for this release). Must = blocks the milestone. |
| **Milestone** | M0–M7 from PLAN.md §8, or `Post-1.0`. |

Path conventions: `%LOCALAPPDATA%` = `C:\Users\<user>\AppData\Local`, `%APPDATA%` = `C:\Users\<user>\AppData\Roaming`, `%SystemRoot%` = `C:\Windows`, `%ProgramData%` = `C:\ProgramData`. All roots are resolved via `SHGetKnownFolderPath` (PLAN §3.3.1), never via string guessing. `<Profile>` = a discovered browser profile directory. "Contents of X" means files and subfolders inside X are removed; X itself is kept.

### Areas

| Prefix | Area | Section |
|---|---|---|
| F-CORE | Scan, preview, safety, deletion engine | §1 |
| F-DEL | Delete modes | §2 |
| F-SYS | System cleaners | §3 |
| F-BRW | Browser cleaners and profile discovery | §4 |
| F-PRV | Privacy cleaners | §5 |
| F-APP | App and developer cleaners (post-1.0) | §6 |
| F-RUN | Running-app detection | §7 |
| F-ELV | Elevation flow | §8 |
| F-CFG | Settings, exclusions, age thresholds, saved profiles | §9 |
| F-LOG | Deletion log | §10 |
| F-CLI | Command line | §11 |
| F-GUI | Desktop UI | §12 |
| F-L10N | Localization | §13 |

---

## 1. Scan, preview and safety core (F-CORE)

| ID | Feature | Risk | Admin | Default | MoSCoW | Milestone |
|---|---|---|---|---|---|---|
| F-CORE-01 | Known-folder root resolution + allowlist | high (if wrong) | No | On (always) | Must | M1 |
| F-CORE-02 | Reparse-point guard | high (if wrong) | No | On (always) | Must | M1 |
| F-CORE-03 | Read-only parallel scan | low | No | On | Must | M1 |
| F-CORE-04 | Preview report | low | No | On | Must | M2 |
| F-CORE-05 | Deletion engine with per-file re-checks | high (if wrong) | No | On | Must | M1 |
| F-CORE-06 | Locked / in-use file handling | medium | No | On (always) | Must | M1 |
| F-CORE-07 | Built-in named actions (closed set) | medium | Varies | — | Must | M2 |
| F-CORE-08 | Definition loader + validator | high (if wrong) | No | On (always) | Must | M2 |
| F-CORE-09 | Honest size accounting | low | No | On | Should | M2 |
| F-CORE-10 | Cancellation | low | No | On | Should | M3 |

### F-CORE-01 Known-folder resolution and root allowlist
- **Description:** Every target path is built from a known folder (`FOLDERID_LocalAppData`, `FOLDERID_RoamingAppData`, `FOLDERID_Windows`, `FOLDERID_ProgramData`, `FOLDERID_Profile`), canonicalised, then checked against an allowlist of permitted sub-roots (e.g. `%SystemRoot%\Temp` is allowed, `%SystemRoot%` is not).
- **User story:** As a user, I want Bezem to be physically unable to delete outside the folders it lists, so a bug or a bad definition can't wipe my disk.
- **Acceptance criteria:**
  - Rejected outright: empty path, any drive root, `%SystemRoot%`, `%SystemRoot%\System32`, `%SystemRoot%\SysWOW64`, `C:\Program Files`, `C:\Program Files (x86)`, the user-profile root, `%APPDATA%` / `%LOCALAPPDATA%` roots themselves.
  - Normalisation defeats `..`, `.`, trailing dots/spaces, 8.3 short names (`PROGRA~1`), `\\?\` and `\\.\` prefixes, UNC paths, forward slashes, and env vars that expand to empty (e.g. `%UNSET%\x` → rejected, not `\x`).
  - Unit + fuzz tests (PLAN §6) cover each case above.

### F-CORE-02 Reparse-point guard
- **Description:** The walker never follows symlinks, junctions, mount points or any `FILE_ATTRIBUTE_REPARSE_POINT` entry. Reparse points themselves are reported as "skipped: link", never deleted (deleting a junction is safe, but Bezem stays conservative and leaves it).
- **User story:** As a user, I want a malicious junction in `%TEMP%` pointing at `C:\` to be ignored.
- **Acceptance criteria:** Integration test creates `%TEMP%\evil -> C:\Users\<user>\Documents` (junction) and a file symlink; after `clean`, Documents is untouched and both links are reported as skipped. OneDrive placeholder files (cloud reparse tag) are never hydrated or deleted.

### F-CORE-03 Read-only parallel scan
- **Description:** `jwalk`/`rayon` walk of all selected targets. Opens nothing for write. Applies age threshold (F-CFG-03), exclusions (F-CFG-02) and per-target globs during the scan, so the preview matches what clean would do.
- **User story:** As a user, I want to see what would be removed without any risk.
- **Acceptance criteria:** Scan never changes a file timestamp (no `LastAccessTime` writes beyond what the OS does on directory listing); a typical profile scans in < 2 s (PLAN §6 benchmark); scan works unelevated, and admin-only targets show as "needs admin: not scanned" instead of failing.

### F-CORE-04 Preview report
- **Description:** Result per cleaner: file count, total bytes, skipped count by reason, and up to 5 sample paths (all paths with `--details` / "Show all" in the GUI).
- **User story:** As a user, I want to know exactly what each cleaner will remove and how much space it frees before I confirm.
- **Acceptance criteria:** Clean is impossible without a scan in the same session (GUI button disabled; CLI `clean` always scans first and shows the table before asking). Skipped reasons are one of: `locked`, `too_new`, `excluded`, `link`, `outside_allowlist`, `needs_admin`, `app_running`, `access_denied`, `readonly`.

### F-CORE-05 Deletion engine with re-checks
- **Description:** Delete never trusts the scan. For each file: open with `FILE_FLAG_OPEN_REPARSE_POINT`, verify via the handle (`GetFinalPathNameByHandleW`) that it is still under an allowlisted root, not a reparse point, still old enough and not excluded, then delete through that same handle (`SetFileInformationByHandle` with `FileDispositionInfoEx`) to close the TOCTOU gap. Empty subfolders inside a target are removed afterwards; the target root folder is kept.
- **User story:** As a user, I want files that changed between scan and clean to be handled safely.
- **Acceptance criteria:** Test swaps a scanned file for a junction between scan and clean → nothing outside the root is touched. Read-only files are skipped and reported (`readonly`) unless the definition sets `allow_readonly = true`. Every outcome lands in the log (F-LOG-01).

### F-CORE-06 Locked / in-use files
- **Description:** On `ERROR_SHARING_VIOLATION` / `ERROR_LOCK_VIOLATION` the file is skipped and reported. No forced unlocking, no handle closing, no `MoveFileEx(..., MOVEFILE_DELAY_UNTIL_REBOOT)`.
- **Acceptance criteria:** A locked test file survives `clean` and the run still exits 0 (locks are normal, see F-CLI exit codes). The summary says "N files in use, skipped".

### F-CORE-07 Built-in named actions
- **Description:** Non-file operations are a closed set implemented in Rust and referenced by name from TOML (`action = "recycle_bin.empty"`). Definitions cannot run commands, scripts or arbitrary registry edits.
- **v1.0 action set:** `recycle_bin.empty`, `service.stop_start` (allowlist: `wuauserv`, `FontCache`), `delivery_optimization.clear`, `dns.flush`, `clipboard.clear`, `clipboard.clear_history`, `explorer.clear_mru` (fixed key list, see F-PRV-02), `chromium.clear_autofill`.
- **Acceptance criteria:** Validator rejects any unknown action name or argument; each action has a dry-run implementation that reports what it would do (and size where measurable).

### F-CORE-08 Definition loader and validator
- **Description:** Parses `cleaners/*.toml` (embedded at build time), validates schema, ids, risk/default consistency and roots.
- **Acceptance criteria:** CI fails if: a target resolves outside the allowlist; `risk = "high"` with `default_enabled = true`; an id is duplicated; `requires_admin = false` but a target is under `%SystemRoot%` or `%ProgramData%` machine-wide folders. `bezem validate <file>` exposes the same checks (F-CLI).

### F-CORE-09 Honest size accounting
- **Description:** Reports logical size and labels it "about". Files with hard-link count > 1 are counted but marked "won't free space"; Recycle Bin mode shows "space is freed when the Recycle Bin is emptied".
- **Acceptance criteria:** No inflated numbers: total equals the sum of per-file sizes actually deleted; there is no "issues found" counter anywhere (PLAN §1 Honest).

### F-CORE-10 Cancellation
- **Description:** Ctrl+C (CLI) or Cancel (GUI) stops after the current file. Completed deletions are logged; nothing is half-deleted.
- **Acceptance criteria:** Cancel during clean exits with code 4 and a partial summary.

---

## 2. Delete modes (F-DEL)

| ID | Feature | Risk | Admin | Default | MoSCoW | Milestone |
|---|---|---|---|---|---|---|
| F-DEL-01 | Permanent delete | medium | No | On for caches/temp | Must | M1 |
| F-DEL-02 | Move to Recycle Bin | low | No | On for privacy items | Must | M3 |
| F-DEL-03 | Per-cleaner default mode + global override | low | No | — | Must | M3 |

### F-DEL-01 Permanent delete
- **Description:** Delete via handle (F-CORE-05). Default for regenerable data: temp, caches, dumps, logs.
- **User story:** As a user cleaning caches, I want space freed immediately, not moved to the Recycle Bin.

### F-DEL-02 Move to Recycle Bin
- **Description:** Uses `IFileOperation` with `FOFX_RECYCLEONDELETE | FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI`. Default for privacy items (history, cookies, sessions, recent documents) so mistakes are undoable.
- **User story:** As a cautious user, I want to restore anything Bezem removed.
- **Acceptance criteria:**
  - If a file cannot be recycled (removable drive, Recycle Bin disabled, file larger than the bin quota), it is **skipped and reported** (`cannot_recycle`). Bezem never silently falls back to permanent delete.
  - The same re-checks as F-CORE-05 run immediately before handing a file to `IFileOperation`.
  - Not applicable to built-in actions (DNS flush, clipboard, Recycle Bin empty); the UI shows "n/a".

### F-DEL-03 Mode selection
- **Description:** Each definition declares `default_mode = "permanent" | "recycle"`. A global setting `delete_mode = "per_cleaner" | "recycle" | "permanent"` (default `per_cleaner`) overrides it; CLI `--recycle` / `--permanent` override the setting for one run.
- **Acceptance criteria:** Preview shows the effective mode per cleaner. `--recycle` and `--permanent` together → exit 2.

---

## 3. System cleaners (F-SYS)

All system cleaners are `category = "system"`.

| ID | Cleaner ID | Name | Risk | Admin | Default | Mode | MoSCoW | Milestone |
|---|---|---|---|---|---|---|---|---|
| F-SYS-01 | `windows.user_temp` | User temporary files | low | No | On | permanent | Must | M2 |
| F-SYS-02 | `windows.system_temp` | Windows temporary files | low | Yes | On | permanent | Must | M5 |
| F-SYS-03 | `windows.thumbnail_cache` | Thumbnail cache | low | No | On | permanent | Must | M2 |
| F-SYS-04 | `windows.wer_user` | Error reports (this user) | low | No | On | permanent | Must | M2 |
| F-SYS-05 | `windows.wer_system` | Error reports (all users) | low | Yes | On | permanent | Must | M5 |
| F-SYS-06 | `windows.crash_dumps_user` | App crash dumps | low | No | On | permanent | Must | M2 |
| F-SYS-07 | `windows.minidumps` | System minidumps | low | Yes | On | permanent | Must | M5 |
| F-SYS-08 | `windows.memory_dump` | Full memory dump (`MEMORY.DMP`) | medium | Yes | Off | permanent | Should | M5 |
| F-SYS-09 | `windows.delivery_optimization` | Delivery Optimization cache | low | Yes | On | action | Must | M5 |
| F-SYS-10 | `windows.update_download` | Windows Update download cache | medium | Yes | On | permanent | Must | M5 |
| F-SYS-11 | `windows.recycle_bin` | Empty Recycle Bin | medium | No | On | action | Must | M2 |
| F-SYS-12 | `windows.directx_shader_cache` | DirectX / GPU shader cache | medium | No | Off | permanent | Should | M2 |
| F-SYS-13 | `windows.font_cache` | Font cache | medium | Yes | Off | permanent | Could | M5 |

The first five cleaners for M2 ("first 5 system cleaners", PLAN §8) are F-SYS-01, 03, 04, 06 and 11: all unelevated.

### Paths and details

| ID | Targets (exact) | Rules / notes |
|---|---|---|
| F-SYS-01 | Contents of `%TEMP%` (normally `%LOCALAPPDATA%\Temp`) | `min_age_hours = 24`; exclude `*.lock`. If `%TEMP%` resolves to an unexpected place (e.g. a drive root), the cleaner is disabled with a warning. |
| F-SYS-02 | Contents of `%SystemRoot%\Temp` | `min_age_hours = 24`. Skips files owned by running installers (locked). |
| F-SYS-03 | `%LOCALAPPDATA%\Microsoft\Windows\Explorer\thumbcache_*.db` | Only `thumbcache_*.db`; **not** `iconcache_*.db` (icon cache rebuild causes blank icons until re-login). Explorer keeps most of these open, so many are reported `locked`; Bezem does not restart Explorer. |
| F-SYS-04 | Contents of `%LOCALAPPDATA%\Microsoft\Windows\WER\ReportArchive`, `...\WER\ReportQueue` | Reports already sent or never to be sent. |
| F-SYS-05 | Contents of `%ProgramData%\Microsoft\Windows\WER\ReportArchive`, `...\WER\ReportQueue`, `...\WER\Temp` | Admin. |
| F-SYS-06 | Contents of `%LOCALAPPDATA%\CrashDumps` | `*.dmp` only. |
| F-SYS-07 | Contents of `%SystemRoot%\Minidump` | `*.dmp` only. |
| F-SYS-08 | `%SystemRoot%\MEMORY.DMP` (single file) | Warning: "Needed if you are diagnosing a blue screen." Single-file allowlist entry, not the whole `%SystemRoot%`. |
| F-SYS-09 | Action `delivery_optimization.clear` (cache lives in `%SystemRoot%\ServiceProfiles\NetworkService\AppData\Local\Microsoft\Windows\DeliveryOptimization\Cache`) | Uses the supported Delivery Optimization interface (the one behind `Delete-DeliveryOptimizationCache`), not raw file deletion. Size reported from the cache folder (read-only). Implementation interface to confirm in M5. |
| F-SYS-10 | Contents of `%SystemRoot%\SoftwareDistribution\Download` | Wrapped in `service.stop_start("wuauserv")`: stop (30 s timeout) → delete → start **only if it was running before**. If stop fails or an update is installing (pending reboot flag `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\WindowsUpdate\Auto Update\RebootRequired` present), the cleaner is skipped with a reason. Never touches `SoftwareDistribution\DataStore`. |
| F-SYS-11 | Action `recycle_bin.empty`: `SHQueryRecycleBinW` for size, `SHEmptyRecycleBinW(NULL, NULL, SHERB_NOCONFIRMATION | SHERB_NOPROGRESSUI | SHERB_NOSOUND)` | All drives for the current user. Medium because it is irreversible; Bezem's own confirmation replaces the shell's. Disabled automatically when the run uses Recycle Bin mode (would be self-defeating); preview explains why. |
| F-SYS-12 | Contents of `%LOCALAPPDATA%\D3DSCache`, `%LOCALAPPDATA%\NVIDIA\DXCache`, `%LOCALAPPDATA%\NVIDIA\GLCache`, `%LOCALAPPDATA%\AMD\DxCache`, `%LOCALAPPDATA%\AMD\DxcCache`, `%LOCALAPPDATA%\AMD\VkCache` | Warning: "Games may stutter on first launch while shaders rebuild." Missing folders are silently skipped. |
| F-SYS-13 | Contents of `%SystemRoot%\ServiceProfiles\LocalService\AppData\Local\FontCache` (`*.dat`) | Wrapped in `service.stop_start("FontCache")`. `%SystemRoot%\System32\FNTCACHE.DAT` is **excluded on purpose** (System32 is never allowlisted). Only useful when fonts render wrongly; description says so. |

### User stories and acceptance (shared by all F-SYS)
- **User story:** As a Windows user, I want to reclaim space taken by temp files, error reports and update leftovers without breaking anything.
- **Acceptance criteria:**
  - On a test VM, each cleaner reports a non-zero size when seeded with fake data and zero after clean (except locked files).
  - Admin cleaners appear in unelevated scans as `needs_admin` with no error.
  - F-SYS-10 leaves `wuauserv` in the same state it found it (test: service stopped beforehand stays stopped).

---

## 4. Browser cleaners (F-BRW)

Category `browser`. Caches are **on by default**; everything that logs the user out or loses state is **opt-in** with a warning (PLAN §4). Every browser cleaner is blocked while that browser runs (F-RUN).

### 4.1 Supported browsers and roots

| ID | Browser | Root (profile container) | Process | MoSCoW | Milestone |
|---|---|---|---|---|---|
| F-BRW-01 | Google Chrome | `%LOCALAPPDATA%\Google\Chrome\User Data` | `chrome.exe` | Must | M4 |
| F-BRW-02 | Microsoft Edge | `%LOCALAPPDATA%\Microsoft\Edge\User Data` | `msedge.exe` | Must | M4 |
| F-BRW-03 | Brave | `%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data` | `brave.exe` | Must | M4 |
| F-BRW-04 | Vivaldi | `%LOCALAPPDATA%\Vivaldi\User Data` | `vivaldi.exe` | Must | M4 |
| F-BRW-05 | Opera / Opera GX | Profile: `%APPDATA%\Opera Software\Opera Stable` (GX: `Opera GX Stable`); cache: `%LOCALAPPDATA%\Opera Software\Opera Stable` (GX: `Opera GX Stable`) | `opera.exe` | Must | M4 |
| F-BRW-06 | Chromium pre-release channels | Chrome `Chrome Beta`, `Chrome Dev`, `Chrome SxS` (Canary); Edge `Edge Beta`, `Edge Dev`, `Edge SxS`; Brave `Brave-Browser-Beta`, `Brave-Browser-Nightly` (each `...\User Data`) | same as stable | Could | M4 |
| F-BRW-07 | Mozilla Firefox | Roaming: `%APPDATA%\Mozilla\Firefox\Profiles\<Profile>`; local: `%LOCALAPPDATA%\Mozilla\Firefox\Profiles\<Profile>` | `firefox.exe` | Must | M4 |

Not covered by browser cleaners: `msedgewebview2.exe` / WebView2 user-data folders (they belong to the host app; see F-APP).

### 4.2 Profile discovery

| ID | Feature | Risk | Admin | Default | MoSCoW | Milestone |
|---|---|---|---|---|---|---|
| F-BRW-10 | Chromium profile discovery | low | No | On | Must | M4 |
| F-BRW-11 | Firefox profile discovery | low | No | On | Must | M4 |
| F-BRW-12 | Per-profile selection | low | No | All profiles | Should | M4 |

#### F-BRW-10 Chromium profile discovery
- **Description:**
  1. Read `<Root>\Local State` (JSON). Profiles = keys of `profile.info_cache` (e.g. `Default`, `Profile 1`, `Profile 3`), with display name from `info_cache.<dir>.name`.
  2. Fallback if `Local State` is missing or unparsable: subfolders named `Default`, `Profile <n>` or `Guest Profile` that contain a `Preferences` file.
  3. Always skip `System Profile`, `Crashpad`, `ShaderCache`, etc. (non-profile folders).
  4. Profile dir names must be a single path segment (no `\`, `/`, `..`); otherwise the entry is ignored and logged.
  5. Opera: the profile root **is** the profile (no `User Data\<Profile>` layer); side profiles live under `<Root>\_side_profiles\<id>` and are treated as additional profiles. Exact Opera layout to be verified on a real install during M4.
- **User story:** As someone with a work and a personal Chrome profile, I want both cleaned and shown by their names ("Work", "Personal"), not as "Profile 1".
- **Acceptance criteria:** Fixture with `Default`, `Profile 1`, `Profile 7` (non-contiguous) and a deleted-but-listed profile → three real profiles found, the missing one reported once and skipped. A malicious `info_cache` key `..\..\Windows` is ignored.

#### F-BRW-11 Firefox profile discovery
- **Description:**
  1. Read `%APPDATA%\Mozilla\Firefox\profiles.ini`. For each `[ProfileN]` section: `Name`, `Path`, `IsRelative`.
  2. `IsRelative=1`: roaming dir = `%APPDATA%\Mozilla\Firefox\<Path>` (e.g. `Profiles\abcd1234.default-release`); local (cache) dir = `%LOCALAPPDATA%\Mozilla\Firefox\<Path>`.
  3. `IsRelative=0` (custom location): reported as "custom profile location: skipped" unless the path is under an allowlisted root. Bezem does not extend the allowlist from `profiles.ini`.
  4. `installs.ini` is read only to mark the default profile per install; it never adds paths.
  5. Fallback if `profiles.ini` is missing: no Firefox profiles (do not guess).
- **Acceptance criteria:** Fixture with two relative profiles and one absolute `D:\FF` profile → two cleaned, one reported as skipped.

#### F-BRW-12 Per-profile selection
- **Description:** GUI lists profiles under each browser (checkbox each). CLI: `--browser-profile "Chrome:Work"` (repeatable). Default: all profiles.

### 4.3 Chromium items (applies to F-BRW-01…06)

`<UD>` = the `User Data` root (for Opera, see F-BRW-10 step 5); `<P>` = `<UD>\<Profile>`. Cleaner ids are `browser.<browser>.<item>`, e.g. `browser.chrome.cache`.

| ID | Item (`<item>`) | Exact paths | Risk | Default | Mode | MoSCoW | Milestone |
|---|---|---|---|---|---|---|---|
| F-BRW-20 | Cache (`cache`) | Contents of `<P>\Cache\Cache_Data` (Chromium ≥ 93); legacy `<P>\Cache` (files `index`, `data_*`, `f_*`) | low | On | permanent | Must | M4 |
| F-BRW-21 | Code cache (`code_cache`) | Contents of `<P>\Code Cache` (`js`, `wasm`, `webui_js`) | low | On | permanent | Must | M4 |
| F-BRW-22 | GPU cache (`gpu_cache`) | Contents of `<P>\GPUCache`, `<P>\DawnCache`, `<P>\DawnGraphiteCache`, `<P>\DawnWebGPUCache` | low | On | permanent | Must | M4 |
| F-BRW-23 | Shared shader caches (`shader_cache`) | Contents of `<UD>\ShaderCache`, `<UD>\GrShaderCache`, `<UD>\GraphiteDawnCache` | low | On | permanent | Should | M4 |
| F-BRW-24 | Crash reports (`crash_reports`) | Contents of `<UD>\Crashpad\reports` | low | On | permanent | Should | M4 |
| F-BRW-25 | Service worker caches (`service_worker_cache`) | Contents of `<P>\Service Worker\CacheStorage`, `<P>\Service Worker\ScriptCache` | medium | Off | permanent | Should | M4 |
| F-BRW-26 | Cookies (`cookies`) | `<P>\Network\Cookies`, `<P>\Network\Cookies-journal`; legacy `<P>\Cookies`, `<P>\Cookies-journal` | high | Off | recycle | Must | M4 |
| F-BRW-27 | History (`history`) | `<P>\History`, `<P>\History-journal`, `<P>\Visited Links`, `<P>\Top Sites`, `<P>\Top Sites-journal`, `<P>\Shortcuts`, `<P>\Shortcuts-journal`, `<P>\Network Action Predictor`, `<P>\Network Action Predictor-journal` | high | Off | recycle | Must | M4 |
| F-BRW-28 | Sessions (`sessions`) | Contents of `<P>\Sessions` (`Session_*`, `Tabs_*`); legacy `<P>\Current Session`, `<P>\Current Tabs`, `<P>\Last Session`, `<P>\Last Tabs` | high | Off | recycle | Must | M4 |
| F-BRW-29 | Saved form data (`autofill`) | Action `chromium.clear_autofill` on `<P>\Web Data`: deletes rows of the `autofill` table only | high | Off | action | Should | M4 |
| F-BRW-30 | Site storage (`site_storage`) | Contents of `<P>\Local Storage`, `<P>\Session Storage`, `<P>\IndexedDB` | high | Off | recycle | Could | Post-1.0 |

Notes:
- **Warnings (shown before selection is accepted):** cookies → "You will be logged out of websites."; history → "Also clears the download list and address-bar suggestions."; sessions → "You won't be able to restore your last tabs."; autofill → "Saved form entries will be gone; passwords, addresses and payment cards are kept."; site storage → "Web apps may lose offline data and settings."
- `Web Data` is never deleted as a file: it also holds search engines, addresses and payment methods. F-BRW-29 opens it with SQLite (`rusqlite`, bundled) and runs one fixed `DELETE FROM autofill` statement; any schema mismatch → cleaner skipped with "unsupported browser version".
- `Login Data` (saved passwords), `Bookmarks`, `Preferences`, `Favicons`, `Extensions` and `Local Extension Settings` are never targeted (see §14).
- **User story (caches):** As a user, I want to clear tens of gigabytes of browser cache without being logged out of anything.
- **User story (privacy):** As a privacy-minded user, I want to clear history and cookies across every profile in one go, with a warning I can't miss.
- **Acceptance criteria (M4 "done when"):** With default selection, after a clean every supported browser starts with all logins and open-tab restore intact. With cookies selected, the warning is shown (GUI dialog / CLI prompt; with `--yes` the warning is printed and the run requires the cleaner id to be named explicitly, see F-CLI-04).

### 4.4 Firefox items (F-BRW-07)

`<R>` = roaming profile dir, `<L>` = local profile dir (from F-BRW-11). Cleaner ids `browser.firefox.<item>`.

| ID | Item | Exact paths | Risk | Default | Mode | MoSCoW | Milestone |
|---|---|---|---|---|---|---|---|
| F-BRW-40 | Cache (`cache`) | Contents of `<L>\cache2` (`entries`, `doomed`, `index`, `trash*`) | low | On | permanent | Must | M4 |
| F-BRW-41 | Startup cache (`startup_cache`) | Contents of `<L>\startupCache` | low | On | permanent | Should | M4 |
| F-BRW-42 | Thumbnails (`thumbnails`) | Contents of `<L>\thumbnails` | low | On | permanent | Should | M4 |
| F-BRW-43 | Crash reports (`crash_reports`) | Contents of `%APPDATA%\Mozilla\Firefox\Crash Reports\submitted` and `...\pending` | low | On | permanent | Could | M4 |
| F-BRW-44 | Cookies (`cookies`) | `<R>\cookies.sqlite`, `<R>\cookies.sqlite-wal`, `<R>\cookies.sqlite-shm` | high | Off | recycle | Must | M4 |
| F-BRW-45 | Sessions (`sessions`) | `<R>\sessionstore.jsonlz4`, contents of `<R>\sessionstore-backups` | high | Off | recycle | Must | M4 |
| F-BRW-46 | Saved form data (`form_history`) | `<R>\formhistory.sqlite` (+ `-wal`, `-shm`) | high | Off | recycle | Should | M4 |
| F-BRW-47 | History (`history`) | — (see note) | high | Off | — | Won't (1.0) | Post-1.0 |

- **F-BRW-47 note:** Firefox stores history **and bookmarks** in `<R>\places.sqlite`. Deleting the file deletes bookmarks, so Bezem won't do that. A post-1.0 named action could delete from `moz_historyvisits` and orphaned `moz_places` rows without touching `moz_bookmarks`; until then the GUI shows "Firefox history: use Firefox's own *Clear recent history* (Ctrl+Shift+Del)". Honest over complete.
- **Acceptance criteria:** After a default clean, Firefox starts with bookmarks, logins and tabs intact; `cache2` is rebuilt.

---

## 5. Privacy cleaners (F-PRV)

Category `privacy`. All **opt-in**, default mode Recycle Bin where files are involved.

| ID | Cleaner ID | Name | Targets (exact) | Risk | Admin | Default | MoSCoW | Milestone |
|---|---|---|---|---|---|---|---|---|
| F-PRV-01 | `privacy.recent_documents` | Recent documents | `%APPDATA%\Microsoft\Windows\Recent\*.lnk` (top level only) + `explorer.clear_mru(RecentDocs)` | medium | No | Off | Must | M4 |
| F-PRV-02 | `privacy.explorer_mru` | Explorer typed paths and Run history | Action `explorer.clear_mru` on values under `HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\TypedPaths`, `...\Explorer\RunMRU`, `...\Explorer\RecentDocs` | medium | No | Off | Must | M4 |
| F-PRV-03 | `privacy.jump_lists` | Jump lists | Contents of `%APPDATA%\Microsoft\Windows\Recent\AutomaticDestinations` (`*.automaticDestinations-ms`), `%APPDATA%\Microsoft\Windows\Recent\CustomDestinations` (`*.customDestinations-ms`) | high | No | Off | Should | M4 |
| F-PRV-04 | `privacy.clipboard` | Clipboard | Action `clipboard.clear` (`OpenClipboard` + `EmptyClipboard`) | low | No | Off | Must | M4 |
| F-PRV-05 | `privacy.clipboard_history` | Clipboard history (Win+V) | Action `clipboard.clear_history` (WinRT `Clipboard::ClearHistory`; pinned items are kept by Windows) | low | No | Off | Should | M4 |
| F-PRV-06 | `privacy.dns_cache` | Flush DNS cache | Action `dns.flush` (`DnsFlushResolverCache`, same as `ipconfig /flushdns`) | low | No (verify in M4) | Off | Must | M4 |

Details and acceptance:
- **F-PRV-01/02:** these are fixed, named registry values for privacy, **not** registry cleaning. `explorer.clear_mru` only deletes values (never keys) under the three keys above; the key list is compiled in and cannot be extended by definitions. Explorer may rewrite `TypedPaths` from memory when it exits; the description says "Takes full effect after you sign out." Bezem does not restart Explorer.
- **F-PRV-03:** warning "Also removes items you pinned to taskbar/Start jump lists." That's why it's high risk.
- **F-PRV-06:** not sized (preview shows "action"). If it turns out to need admin on supported Windows versions, it moves to the admin group (F-ELV).
- **User story:** As a user lending my PC to someone, I want to remove traces of what I opened recently.
- **Acceptance criteria:** Each item has a dry-run that lists what it would remove (count of `.lnk` files, count of MRU values); MRU values are logged by value name only, not data (see F-LOG-03).

---

## 6. App and developer cleaners (F-APP), post-1.0

Category `apps` (developer tools get the sub-tag `dev`). All low/medium risk caches; each blocked while its app runs (F-RUN). Targets are contents of the listed folders.

| ID | Cleaner ID | App | Targets | Risk | Default | MoSCoW |
|---|---|---|---|---|---|---|
| F-APP-01 | `apps.discord` | Discord | `%APPDATA%\discord\Cache\Cache_Data`, `%APPDATA%\discord\Code Cache`, `%APPDATA%\discord\GPUCache` | low | On | Should |
| F-APP-02 | `apps.teams` | Microsoft Teams (new) | `%LOCALAPPDATA%\Packages\MSTeams_8wekyb3d8bbwe\LocalCache\Microsoft\MSTeams\EBWebView\Default\Cache\Cache_Data`, `...\Code Cache`, `...\GPUCache` | low | On | Should |
| F-APP-03 | `apps.teams_classic` | Teams classic | `%APPDATA%\Microsoft\Teams\Cache`, `...\Code Cache`, `...\GPUCache` | low | On | Could |
| F-APP-04 | `apps.spotify` | Spotify | `%LOCALAPPDATA%\Spotify\Data`; Store build `%LOCALAPPDATA%\Packages\SpotifyAB.SpotifyMusic_zpdnekdrzrea0\LocalCache\Spotify\Data` | medium (offline songs re-download) | Off | Should |
| F-APP-05 | `apps.slack` | Slack | `%APPDATA%\Slack\Cache\Cache_Data`, `%APPDATA%\Slack\Code Cache`, `%APPDATA%\Slack\GPUCache` | low | On | Should |
| F-APP-06 | `dev.vscode` | VS Code | `%APPDATA%\Code\Cache`, `%APPDATA%\Code\CachedData`, `%APPDATA%\Code\CachedExtensionVSIXs`, `%APPDATA%\Code\Code Cache`, `%APPDATA%\Code\GPUCache` | low | Off | Should |
| F-APP-07 | `dev.npm` | npm cache | `%LOCALAPPDATA%\npm-cache\_cacache` | low | Off | Should |
| F-APP-08 | `dev.pip` | pip cache | `%LOCALAPPDATA%\pip\Cache` | low | Off | Should |
| F-APP-09 | `dev.nuget` | NuGet HTTP cache | `%LOCALAPPDATA%\NuGet\v3-cache`, `%LOCALAPPDATA%\NuGet\plugins-cache` (never `%USERPROFILE%\.nuget\packages`) | low | Off | Could |
| F-APP-10 | `windows.old` | Previous Windows installation | Via supported `cleanmgr`/DISM path only (PLAN §4) | high | Off | Could |
| F-APP-11 | — | Community definitions import (validator + diff view; opt-in) | — | high | Off | Could |

Milestone: Post-1.0 for all. Developer cleaners are off by default because a cold cache costs developers time.

---

## 7. Running-app detection (F-RUN)

| ID | Feature | Risk | Admin | Default | MoSCoW | Milestone |
|---|---|---|---|---|---|---|
| F-RUN-01 | Detect running browser / app before cleaning it | high (if wrong) | No | On (always) | Must | M4 |
| F-RUN-02 | Wait / Skip / Cancel choice | low | No | Ask | Must | M4 |

### F-RUN-01 Detection
- **Description:** Two independent signals; either one blocks the cleaner:
  1. **Process check:** `CreateToolhelp32Snapshot` for the image names in §4.1 / the definition's `process = [...]` list, in the current session only.
  2. **Lock probe:** Chromium holds `<UD>\lockfile` open while running; Firefox holds `<R>\parent.lock`. Bezem tries to open the file for exclusive access (read-only intent, never deletes it); a sharing violation = running. This catches non-standard exe names and distinguishes channels (Chrome Beta running does not block Chrome Stable).
- **Known traps:** Edge *Startup boost* and Chrome *Continue running background apps* keep processes alive with no window. The message names the setting: "Edge is running in the background (Startup boost). Close it from the tray or turn off Startup boost."
- **Acceptance criteria:** With Chrome open, `browser.chrome.*` is reported `app_running` and nothing under its root is touched; other browsers are still cleaned.

### F-RUN-02 Wait / Skip / Cancel
- **Description:** GUI dialog: **Wait** (polls every 2 s, shows a live "waiting for Chrome…"), **Skip this browser**, **Cancel**. CLI interactive: same choice as a prompt. CLI non-interactive: `--wait-for-apps <seconds>` (default 0 = skip immediately).
- **Not doing:** killing or force-closing processes, even "politely" (see §14).
- **Acceptance criteria:** Non-interactive run with a browser open finishes with exit code 6 and the rest cleaned.

---

## 8. Elevation (F-ELV)

| ID | Feature | Risk | Admin | Default | MoSCoW | Milestone |
|---|---|---|---|---|---|---|
| F-ELV-01 | Run unelevated by default (asInvoker manifest) | — | No | On | Must | M5 |
| F-ELV-02 | Elevated worker for admin-only cleaners | high (if wrong) | Yes | Ask | Must | M5 |
| F-ELV-03 | Service stop/start helper | medium | Yes | — | Must | M5 |

### Flow (F-ELV-02)

```
Unelevated bezem (scan all; admin items = "needs admin")
   │ user selects ≥1 admin cleaner and confirms
   ▼
Prompt: "3 cleaners need administrator rights: Windows temp, Windows Update cache,
         System minidumps. Continue with UAC prompt? [Yes / Skip these]"
   │ Yes
   ▼
ShellExecuteExW(verb="runas", bezem.exe --elevated-worker <request-file>)
   │ request file: %LOCALAPPDATA%\Bezem\run\<uuid>.json (ACL: current user + Administrators)
   │ contains only cleaner ids + options; no paths
   ▼
Elevated worker: re-loads embedded definitions, re-scans, re-checks, deletes only
   admin cleaners (requires_admin = true), writes result + log, exits
   ▼
Unelevated process merges results into one summary and one log
```

- **Rules:**
  - User-profile cleaners **always** run in the unelevated process. Admin cleaners only touch machine-wide paths (`%SystemRoot%\...`, `%ProgramData%\...`), so an over-the-shoulder elevation as a different admin account cannot redirect `%LOCALAPPDATA%`.
  - The worker trusts nothing from the request file except cleaner ids that exist and are `requires_admin = true`; anything else → worker exits 2.
  - No persistent elevated helper, no service, no scheduled task.
  - UAC declined → admin cleaners reported `needs_admin`, the rest still cleaned, exit code 5.
  - Already elevated (e.g. admin console): runs everything in-process, no relaunch.
  - GUI: shield icon on admin cleaners and on the Clean button when any is selected.
- **User story:** As a standard user, I want to clean my own temp files without a UAC prompt, and get exactly one prompt only when I chose system-wide items.
- **Acceptance criteria (M5 "done when"):** Selecting only user cleaners never triggers UAC; selecting admin cleaners triggers exactly one UAC prompt per run.

### F-ELV-03 Service helper
- **Description:** `service.stop_start(name)`: only for `wuauserv` and `FontCache` (compiled-in allowlist). Records prior state; stops with a 30 s timeout; runs the cleaner; restarts only if it was running; always attempts restart on error/cancel.
- **Acceptance criteria:** Killing the worker mid-clean is tested; on next start Bezem notices the service marker file and offers to restart the service.

---

## 9. Settings, exclusions, age thresholds, saved profiles (F-CFG)

| ID | Feature | Risk | Admin | Default | MoSCoW | Milestone |
|---|---|---|---|---|---|---|
| F-CFG-01 | Settings file + portable mode | low | No | — | Must | M3 |
| F-CFG-02 | User exclusions | low | No | Empty | Must | M3 |
| F-CFG-03 | Age thresholds | low | No | 24 h (temp) | Must | M1 |
| F-CFG-04 | Cleaner selection persistence | low | No | Defaults | Should | M6 |
| F-CFG-05 | Saved selection profiles | low | No | — | Should | Post-1.0 |
| F-CFG-06 | Update check (opt-in) | low | No | **Off** | Could | M7 (pending PLAN §10, decision 4) |
| F-CFG-07 | User-created scheduled run (Task Scheduler) | medium | No | Off | Could | Post-1.0 |

### F-CFG-01 Settings
- **Location:** `%APPDATA%\Bezem\config.toml`. **Portable mode:** if a file named `bezem.portable` sits next to `bezem.exe`, config and logs live next to the exe instead.
- **Keys (v1.0):**

| Key | Values | Default |
|---|---|---|
| `delete_mode` | `per_cleaner` / `recycle` / `permanent` | `per_cleaner` |
| `min_age_hours_override` | integer ≥ 0 or unset | unset (use definitions) |
| `exclusions` | list of globs | `[]` |
| `language` | `auto` / `en` / `nl` | `auto` |
| `theme` | `system` / `light` / `dark` | `system` |
| `confirm_before_clean` | bool | `true` (GUI; CLI uses `--yes`) |
| `log_retention_days` | integer, 0 = keep forever | `30` |
| `log_privacy_paths` | `summary` / `full` | `summary` |
| `selected_cleaners` | list of ids | definition defaults |
| `update_check` | bool | `false` |

- **Acceptance criteria:** Invalid config → Bezem starts with defaults, names the bad key, exit code 8 for CLI only if `--strict-config`. No settings ever stored in the registry.

### F-CFG-02 Exclusions
- **Description:** User globs (`globset` syntax, env vars expanded, case-insensitive), e.g. `%TEMP%\MyBuild\**`, `**\*.important`. Applied during scan **and** re-checked at delete (F-CORE-05). Also: exclude a whole cleaner or a browser profile.
- **Acceptance criteria:** Exclusions can only narrow what is deleted, never widen it; an exclusion that matches nothing shows a hint ("matches no scanned files"). Built-in safety rules (allowlist, reparse points) cannot be disabled by any setting.

### F-CFG-03 Age thresholds
- **Description:** Per-target `min_age_hours` in TOML (temp folders: 24). A file counts as "new" if **either** `LastWriteTime` or `CreationTime` is within the threshold (installers extracted to `%TEMP%` keep old write times but have new creation times). Global override via setting or `--min-age <hours>`.
- **Acceptance criteria:** Overriding to 0 shows a warning ("files still in use by running installers may be removed"); skipped files are reported as `too_new`.

### F-CFG-05 Saved selection profiles
- **Description:** Named sets of selected cleaners + options (delete mode, age override, browser profiles), e.g. "Quick", "Privacy sweep". Stored in `config.toml` under `[profiles.<name>]`. CLI `bezem profile list|save|delete`, `bezem clean --profile <name>`. GUI dropdown next to Scan.
- **Milestone note:** PLAN §8 lists saved profiles post-1.0; the M6 settings screen reserves the slot.
- **Acceptance criteria:** A profile referencing a cleaner id that no longer exists loads with a warning and runs the rest; high-risk items in a profile are still flagged in the preview every time.

### F-CFG-07 Scheduled run
- **Description:** `bezem schedule create --profile Quick --weekly` creates a Windows Task Scheduler task **only when the user runs that command**; `bezem schedule remove` deletes it. Scheduled runs use `--yes`, never elevate, never include high-risk cleaners. Nothing is scheduled by default or by the installer.

---

## 10. Deletion log (F-LOG)

| ID | Feature | Risk | Admin | Default | MoSCoW | Milestone |
|---|---|---|---|---|---|---|
| F-LOG-01 | Per-run JSON Lines log | low | No | On (always) | Must | M1 |
| F-LOG-02 | Log retention + `bezem log` | low | No | 30 days | Should | M3 |
| F-LOG-03 | Privacy-aware logging | low | No | `summary` | Must | M4 |

### F-LOG-01 Log format
- **Location:** `%LOCALAPPDATA%\Bezem\logs\<UTC timestamp>_<run-id>.jsonl` (e.g. `2026-10-28T14-03-22Z_7f3a.jsonl`). Local only, never uploaded (no network code exists).
- **Record per item:**
  ```json
  {"ts":"2026-10-28T14:03:22.418Z","run":"7f3a","cleaner":"windows.user_temp","path":"C:\\Users\\a\\AppData\\Local\\Temp\\x.tmp","bytes":4096,"mode":"permanent","result":"deleted"}
  ```
  `result` ∈ `deleted`, `recycled`, `action_done`, `skipped_<reason>` (reasons as in F-CORE-04), `failed` (+ `error`).
- **Header/footer records:** Bezem version, definitions hash, elevated yes/no, selection, totals, duration.
- **Acceptance criteria:** Every path deleted in a test run appears exactly once; a crash mid-run leaves a valid, truncated log (line-buffered writes).

### F-LOG-02 Retention and viewer
- Logs older than `log_retention_days` are deleted at the start of the next run (logs are the only thing Bezem cleans without a preview). `bezem log list`, `bezem log show <run>`, `bezem log path`, `bezem log purge`.

### F-LOG-03 Privacy-aware logging
- **Description:** For `category = "privacy"` and browser privacy items, the default `log_privacy_paths = "summary"` logs cleaner, count and bytes, not file names (a `Recent\*.lnk` name reveals which documents you opened). MRU values are never logged by data. `full` is opt-in.

---

## 11. Command line (F-CLI)

| ID | Feature | MoSCoW | Milestone |
|---|---|---|---|
| F-CLI-01 | `list` / `show` | Must | M3 |
| F-CLI-02 | `scan` incl. `--json` | Must | M2 (`scan`), M3 (`--json`) |
| F-CLI-03 | `clean` with confirmation | Must | M3 |
| F-CLI-04 | Non-interactive mode and high-risk gating | Must | M3 |
| F-CLI-05 | Documented exit codes | Must | M3 |
| F-CLI-06 | `validate`, `log`, `config` | Should | M3 |
| F-CLI-07 | `profile`, `schedule` | Could | Post-1.0 |

All CLI features: risk low (wrapping core), admin No (except via F-ELV), default n/a.

### Commands

| Command | Purpose |
|---|---|
| `bezem list [--category <c>] [--json]` | All cleaners: id, name, category, risk, admin, default on/off, effective mode. |
| `bezem show <cleaner-id>` | Print the cleaner's TOML definition and the resolved paths on this machine. |
| `bezem scan [selection] [--details] [--json]` | Dry run: table of cleaner, files, size, skipped. Never deletes. |
| `bezem clean [selection] [options]` | Scan → preview → confirm → delete → summary. |
| `bezem validate <file.toml>` | Run the definition validator (F-CORE-08) on a file. |
| `bezem log list\|show <run>\|path\|purge` | Inspect deletion logs. |
| `bezem config get <key>\|set <key> <value>\|path\|reset` | Settings. |
| `bezem profile list\|save <name>\|delete <name>` | Saved selections (post-1.0). |
| `bezem schedule create\|remove\|show` | User-created scheduled task (post-1.0). |
| `bezem --version`, `bezem help [command]` | |

### Selection flags (`scan`, `clean`)

| Flag | Meaning |
|---|---|
| *(none)* | Default-enabled cleaners (or the persisted GUI selection, F-CFG-04). |
| `--only <list>` | Comma-separated categories (`system`, `browser`, `privacy`, `apps`) and/or cleaner ids; `*` wildcard allowed: `--only browser.*.cache,windows.user_temp`. |
| `--exclude-cleaner <list>` | Remove ids/categories from the selection. |
| `--browser-profile <Browser:Name>` | Limit browser cleaners to a profile; repeatable. |
| `--profile <name>` | Use a saved selection (post-1.0). |
| `--all` | All non-high-risk cleaners. High-risk ones are never included by wildcards or `--all`; they must be named by full id. |

### Option flags

| Flag | Applies to | Meaning |
|---|---|---|
| `--yes`, `-y` | clean | Skip the confirmation prompt. |
| `--recycle` / `--permanent` | clean | Override delete mode for this run (mutually exclusive). |
| `--min-age <hours>` | scan, clean | Override age threshold. |
| `--no-elevate` | clean | Never prompt for UAC; admin cleaners reported `needs_admin`. |
| `--wait-for-apps <sec>` | clean | Wait up to N seconds for running browsers/apps to close (default 0). |
| `--details` | scan, clean | Show all paths instead of 5 samples. |
| `--json` | list, scan, clean, log | Machine-readable output on stdout (`"schema": 1`); human text goes to stderr. Keys never localized. |
| `--quiet`, `-q` / `--verbose`, `-v` | all | Less / more output. |
| `--no-color` | all | Also honoured via `NO_COLOR` env var. |
| `--lang <en\|nl>` | all | Override UI language. |
| `--config <path>` | all | Use another config file. |
| `--strict-config` | all | Fail (exit 8) on any config problem. |

### F-CLI-03/04 Confirmation rules
- Interactive `clean` prints the preview table and asks `Delete 1.2 GB in 3,412 files? [y/N]` (localized; default No).
- High-risk items require typing the warning acknowledgement: `This will log you out of websites. Type "yes" to continue`.
- `--yes` with a high-risk cleaner works only if that cleaner was named by full id (`--only browser.chrome.cookies`); otherwise exit 2 with an explanation. This prevents `--only browser --yes` from logging someone out.
- Non-TTY stdin without `--yes` → exit 2 (never hangs in Task Scheduler).

### F-CLI-05 Exit codes

This table is the single source of truth for exit codes; other docs link here.

| Code | Name | When |
|---|---|---|
| 0 | `OK` | Completed. Skips for normal reasons (`locked`, `too_new`, `excluded`, `link`) are still 0. |
| 1 | `ERROR` | Unexpected internal error. |
| 2 | `USAGE` | Bad arguments, conflicting flags, unknown cleaner id, high-risk without explicit id, non-TTY without `--yes`. |
| 3 | `PARTIAL` | Finished, but ≥1 file failed for an unexpected reason (`failed`, `access_denied`, `cannot_recycle`). |
| 4 | `CANCELLED` | User declined the confirmation or pressed Ctrl+C. |
| 5 | `NEEDS_ADMIN` | Admin cleaners selected but UAC declined or `--no-elevate` given; other cleaners ran. |
| 6 | `APP_RUNNING` | ≥1 cleaner skipped because its app was running (after any wait). |
| 7 | `INVALID_DEFINITION` | `validate` failed, or a built-in definition failed to load (should never ship). |
| 8 | `CONFIG_ERROR` | Config unreadable with `--strict-config`. |
| 9 | `SAFETY_VIOLATION` | The safety layer aborted a target (e.g. final path left the allowed root). Always investigate and report. |

When several apply, the highest-priority code wins in this order: 9, 1, 2, 7, 8, 4, 5, 6, 3, 0. `scan` returns 0 on success even if there is nothing to clean.

### Examples
```
bezem scan --only system --json > report.json
bezem clean --only browser.*.cache --yes --wait-for-apps 60
bezem clean --only browser.firefox.cookies,privacy.clipboard --recycle
bezem clean --all --no-elevate --yes          # Task Scheduler-friendly, never prompts UAC
```

---

## 12. Desktop UI (F-GUI)

| ID | Feature | MoSCoW | Milestone |
|---|---|---|---|
| F-GUI-01 | Grouped checklist with risk badges + "What does this remove?" panel showing resolved real paths | Must | M6 |
| F-GUI-02 | Scan → results with sizes → Clean (disabled until scanned) | Must | M6 |
| F-GUI-03 | Warning dialogs for high-risk items, running-app dialog (F-RUN-02), elevation prompt (F-ELV-02) | Must | M6 |
| F-GUI-04 | Settings screen (all F-CFG-01 keys, exclusions editor with live "matches N files") | Must | M6 |
| F-GUI-05 | Light/dark theme following Windows; full keyboard access; screen-reader labels | Must | M6 |
| F-GUI-06 | Log viewer (last runs, open log folder) | Should | M6 |
| F-GUI-07 | Profile dropdown | Should | Post-1.0 |

No splash screen, no tray icon, no auto-start, no ads (PLAN §5). Done when: feature parity with the CLI (PLAN §8 M6).

---

## 13. Localization (F-L10N)

| ID | Feature | Risk | Admin | Default | MoSCoW | Milestone |
|---|---|---|---|---|---|---|
| F-L10N-01 | String externalisation (Fluent `.ftl`, `fluent-rs`) | low | No | — | Must | M3 (strings out of code from the first CLI release) |
| F-L10N-02 | English (`en`) | low | No | Fallback | Must | M3 |
| F-L10N-03 | Dutch (`nl`) | low | No | Auto for `nl-*` | Must | M7 |
| F-L10N-04 | Localized cleaner names/descriptions/warnings | low | No | — | Must | M7 |
| F-L10N-05 | Locale-aware numbers and sizes | low | No | — | Should | M7 |

- **Detection:** `GetUserDefaultUILanguage`; `nl-NL`/`nl-BE` → `nl`, anything else → `en`. Override with setting `language` or `--lang`.
- **Definitions:** TOML gets optional `[i18n.nl]` with `name`, `description`, `warning`. Missing translation → English, and CI reports untranslated built-in cleaners as a warning before M7 and an error at M7.
- **Formatting:** sizes use 1024-based units labelled KB/MB/GB to match Explorer; decimal separator follows the language (`1.2 GB` / `1,2 GB`).
- **Not localized:** cleaner ids, JSON keys, log records, exit codes (stable for scripts).
- **Acceptance criteria:** Every UI string exists in both `en` and `nl`; a CI check fails on missing keys; screenshots in both languages fit without truncation at 100% and 150% scaling.

---

## 14. Not doing (and why)

| Not doing | Why |
|---|---|
| Registry "cleaning" (orphaned keys, "invalid entries") | Real risk, no measurable benefit (PLAN §4). The three MRU keys in F-PRV-02 are a privacy feature with a fixed key list, not a registry cleaner. |
| Prefetch / Superfetch (SysMain) clearing | Makes Windows slower to start apps. |
| "Memory optimizers", RAM boosters, "PC health scores", "issues found" counters | Dishonest; Windows manages memory better than a tool can. |
| Driver updaters, uninstaller, startup manager, duplicate finder, defragmenter | Out of scope; not "sweep temporary data". |
| Background service, tray icon, auto-start, automatic scheduled scans | Breaks zero overhead. A scheduled run exists only if the user creates it (F-CFG-07). |
| Any telemetry, crash reporting to a server, analytics | Zero telemetry. The only possible network call is the opt-in update check. |
| Killing or force-closing browsers/apps | Can lose unsaved work and corrupt profiles. We wait or skip (F-RUN-02). |
| Forcing locked files (closing handles, delete-on-reboot via `MOVEFILE_DELAY_UNTIL_REBOOT`) | A locked file is in use; forcing it risks breaking the app holding it, and reboot-time deletes can't be re-checked. |
| Restarting Explorer to release the thumbnail/icon cache | Disruptive to the user's session; reported as locked instead. |
| Icon cache (`iconcache_*.db`) deletion | Causes blank icons until re-login for little space. |
| Deleting saved passwords (`Login Data`, `logins.json`, `key4.db`) | Irreversible loss with no space benefit; use the browser's password manager. |
| Deleting Firefox `places.sqlite` | Contains bookmarks, not just history. Firefox history waits for a safe row-level action (F-BRW-47). |
| Deleting Chromium `Web Data` as a whole file | It also holds payment cards, addresses and search engines. Autofill is cleared at row level (F-BRW-29). |
| Bookmarks, extensions, preferences, browser profiles themselves | User data, not temporary data. |
| "Secure wipe" / multi-pass shredding | Ineffective on SSDs (wear levelling, TRIM) and gives a false sense of security. Honest > impressive. |
| Deleting System Restore points / Volume Shadow Copies | Removes the user's safety net; managed in Windows System Protection. |
| WinSxS / component store cleanup | Only safe through DISM; Windows does it on schedule. May be reconsidered post-1.0 the same way as `Windows.old` (supported path only). |
| Direct deletion of `Windows.old` | Only through the supported cleanmgr/DISM path (F-APP-10). |
| Clearing Windows event logs | Little space, removes troubleshooting data, and looks like anti-forensics. |
| Disabling hibernation / deleting `hiberfil.sys` / `pagefile.sys` | System configuration change, not cleanup. |
| "Debloat" / privacy tweaks (disabling Windows telemetry, removing apps) | Configuration changes with side effects; out of scope for a cleaner. |
| Store cache reset (`wsreset`), running arbitrary commands from definitions | Definitions can't run commands by design (F-CORE-07). |
| Silent fallback from Recycle Bin to permanent delete | Violates "safe by default"; such files are skipped (F-DEL-02). |
| Cloud sync, accounts, upsells, "Pro" tier | No network, no ads, no upsell (PLAN §5). |
| Windows 7/8.1 or pre-22H2 Windows 10 support | Unsupported by Microsoft; PLAN §10, decision 2 recommends Win10 22H2 and Win11. |
| Cleaners for non-Windows OSes | Bezem is a Windows tool; the core is portable but no cleaners will ship for other OSes. |

---

## 15. Milestone roll-up

| Milestone | Must-have features |
|---|---|
| M0 | (no features; repo, CI, policies) |
| M1 | F-CORE-01, 02, 03, 05, 06; F-DEL-01; F-CFG-03; F-LOG-01 |
| M2 | F-CORE-04, 07, 08, 09; F-SYS-01, 03, 04, 06, 11 (+ F-SYS-12 Should); `bezem scan` |
| M3 | F-CLI-01…06; F-DEL-02, 03; F-CORE-10; F-CFG-01, 02; F-LOG-02; F-L10N-01, 02 |
| M4 | F-BRW-01…05, 10, 11, 20–22, 26–28, 40, 44, 45; F-PRV-01, 02, 04, 06; F-RUN-01, 02; F-LOG-03 |
| M5 | F-ELV-01…03; F-SYS-02, 05, 07, 09, 10 (+ 08, 13) |
| M6 | F-GUI-01…05; F-CFG-04 |
| M7 | F-L10N-03, 04, 05; F-CFG-06 if PLAN §10, decision 4 decides "ship" |
| Post-1.0 | F-APP-*, F-CFG-05, F-CFG-07, F-BRW-30, F-BRW-47, F-GUI-07, F-CLI-07 |

# Bezem: Technical Architecture

> Status: design baseline for milestones M1 to M7. Companion to [`PLAN.md`](PLAN.md), which stays authoritative for
> principles, scope and roadmap. This document describes *how* those principles are implemented. Where this document
> refines or deviates from the plan, the change is listed in [section 15](#15-decisions-and-deviations-from-planmd).

Contents

1. [Overview](#1-overview)
2. [Crates and responsibilities](#2-crates-and-responsibilities)
3. [Public API sketch (`bezem-core`)](#3-public-api-sketch-bezem-core)
4. [Pipeline and lifecycle](#4-pipeline-and-lifecycle)
5. [Cleaner definition schema (TOML)](#5-cleaner-definition-schema-toml)
6. [Built-in special actions](#6-built-in-special-actions)
7. [Path resolution](#7-path-resolution)
8. [Safety layer](#8-safety-layer)
9. [Elevation, services and running applications](#9-elevation-services-and-running-applications)
10. [Concurrency and performance](#10-concurrency-and-performance)
11. [Logging](#11-logging)
12. [Configuration](#12-configuration)
13. [Error handling and exit codes](#13-error-handling-and-exit-codes)
14. [Dependencies](#14-dependencies)
15. [Decisions and deviations from PLAN.md](#15-decisions-and-deviations-from-planmd)

---

## 1. Overview

Bezem is a library (`bezem-core`) with two thin front-ends (`bezem-cli`, `bezem-gui`) and one data crate
(`bezem-cleaners`). Everything that touches the file system, the registry, services or processes lives in
`bezem-core`. The front-ends only select cleaners, render reports and ask for confirmation.

Design rules that follow from the plan's principles:

| Rule | Consequence in code |
|---|---|
| Scan is read-only; delete re-verifies | `DeletePlan` stores *file identities* (volume serial + 128-bit file ID), not just paths. Delete re-opens every item by handle and checks the identity again. |
| Never follow reparse points | All enumeration and deletion below a verified root is done with **handle-relative opens** using `FILE_OPEN_REPARSE_POINT`. No path string below the root is ever re-resolved by the OS. |
| Definitions are data, not code | The TOML schema is closed (`deny_unknown_fields`). Non-file operations are a closed Rust `enum` of special actions. There is no "run command" action. |
| Least privilege | The main process never elevates. Admin cleaners run in a separate elevated worker that accepts **cleaner IDs only**, never paths. |
| No network | No crate with networking capability is linked. `cargo deny` bans `reqwest`, `hyper`, `ureq`, `tokio` (net feature) and similar. |
| `unsafe` is contained | All FFI lives in `bezem-core::win32`. Every other module and crate is free of `unsafe`. |

Target platform: Windows 10 22H2 (build 19045) and Windows 11, x86-64 and ARM64. Several APIs used below
(`FILE_DISPOSITION_INFO_EX` with `FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE`, `GetTempPath2W`) require
build 17763 (1809) or later, which the minimum version covers. Pure logic (template parsing, globbing, validation,
INI/JSON profile parsing) is platform-independent and its unit tests also run on Linux in CI; everything under
`win32` is `#[cfg(windows)]`.

---

## 2. Crates and responsibilities

```
                 +-------------+        +-------------+
                 |  bezem-cli  |        |  bezem-gui  |
                 |  (clap)     |        |  (eframe)   |
                 +------+------+        +------+------+
                        |                      |
                        +----------+-----------+
                                   v
                          +-----------------+
                          | bezem-cleaners  |  embedded *.toml, catalog,
                          |                 |  community import
                          +--------+--------+
                                   v
                          +-----------------+
                          |   bezem-core    |  model, validation, resolver,
                          |                 |  safety, walker, deleter,
                          |                 |  actions, elevation, logging
                          +-----------------+
```

Dependency direction is strictly downward. `bezem-core` has no knowledge of where definitions come from and no UI
dependencies.

### 2.1 `bezem-core`

| Module | Responsibility |
|---|---|
| `model` | Definition types (`CleanerDef`, `TargetDef`, `ActionDef`), validated types (`Cleaner`, `Target`), IDs, enums. |
| `validate` | Turns `CleanerDef` into `Cleaner`. All schema rules from section 5.3. Pure, no I/O. |
| `resolve` | Token expansion (`%LOCALAPPDATA%` etc.) through Known Folder APIs, browser profile discovery, root wildcard expansion. |
| `glob` | Wrapper around `globset` with Bezem's fixed matching semantics (section 7.3). |
| `safety` | Path types (`WidePath`, `PathComponent`, section 8.3), `SafetyPolicy`, allowlisted bases, protected paths, root verification, per-item verdicts. |
| `walk` | Parallel, handle-relative directory enumeration (scan phase). |
| `delete` | Handle-relative re-verification and deletion (clean phase); Recycle Bin mode. |
| `actions` | Implementations of the closed set of special actions. |
| `apps` | Running-application detection (process snapshot) and lock attribution (Restart Manager). |
| `services` | `ServiceGuard` (stop, wait, restore). |
| `elevation` | Worker launch (UAC), named-pipe IPC, wire protocol, `worker_main`. |
| `engine` | `Engine` and `Orchestrator`: drive scan/plan/execute, merge local and elevated results. |
| `report` | `ScanReport`, `DeletePlan`, `CleanSummary`, `SkipReason`. |
| `log` | Deletion log (JSON Lines) writer and retention; `tracing` setup helpers. |
| `config` | `Config` load/save (atomic), defaults, portable mode. |
| `win32` | The **only** module with `unsafe`. Thin, safe wrappers: `OwnedHandle` RAII, `nt_open_relative`, `query_dir_batch`, `final_path`, `set_disposition_ex`, SCM, pipes, COM helpers. Each wrapper documents its invariants. |

`bezem-core/src/lib.rs` uses `#![deny(unsafe_code)]` and `mod win32` carries `#[allow(unsafe_code)]`. (`forbid`
cannot be relaxed locally, so `deny` is used in this one crate.) A CI step greps for `unsafe` outside
`crates/bezem-core/src/win32/` and fails the build if it finds any. All other crates use `#![forbid(unsafe_code)]`.

### 2.2 `bezem-cleaners`

- `build.rs` scans `../../cleaners/**/*.toml`, sorts them and generates
  `static BUILTIN: &[BuiltinFile] = &[BuiltinFile { path: "system/user_temp.toml", src: include_str!(...) }, ...];`.
  No runtime file access is needed for built-ins.
- `Catalog::builtin() -> Result<Catalog, CatalogError>`: parses and validates all embedded files. A unit test
  (`all_builtin_definitions_validate`) and the CI definition validator guarantee this never fails in a release.
- `Catalog::with_community(dir)`: loads opt-in community definitions (post-1.0) from
  `%LOCALAPPDATA%\Bezem\cleaners\community\`, each pinned by SHA-256 in the config (section 8.6).
- `import::preview(path) -> ImportPreview`: validation result plus a human-readable diff against an existing
  definition with the same ID, for the import dialog.

### 2.3 `bezem-cli`

`clap` (derive) front-end implementing the commands in PLAN section 5 plus two developer commands:

```
bezem list | scan [--json] [--elevate] | clean [--only ...] [--yes] [--recycle|--permanent]
bezem show <id> | validate <file.toml>... | log [show|path]
```

Also contains the hidden entry point `--bezem-elevated-worker` (section 9.1). Maps errors to the exit codes in
section 13.3. Output tables are plain text; `--json` emits the serde form of `ScanReport` / `CleanSummary`
(versioned with `"format": 1`).

### 2.4 `bezem-gui`

`eframe`/`egui` application (subject to open decision 1 in the plan; nothing in `bezem-core` depends on the choice).
The engine runs on a background thread; progress arrives over `std::sync::mpsc` and the closure calls
`ctx.request_repaint()`. The UI thread never performs file I/O. The GUI binary also contains the
`--bezem-elevated-worker` entry point (built with `windows_subsystem = "windows"`, so no console window appears).

---

## 3. Public API sketch (`bezem-core`)

Signatures are indicative; field visibility and naming may change during implementation, but the *shape* (sealed
plan, identity-based entries, callback progress, cooperative cancellation) is part of the design.

### 3.1 Definitions and validated cleaners

```rust
/// Raw, deserialized definition. Never handed to the engine directly.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanerDef {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: Category,
    pub group: Option<String>,
    pub risk: Risk,
    #[serde(default)] pub data_class: DataClass,
    #[serde(default)] pub requires_admin: bool,
    #[serde(default)] pub default_enabled: bool,
    pub warning: Option<String>,
    #[serde(default)] pub running_apps: Vec<String>,
    #[serde(default)] pub running_policy: RunningPolicy,
    #[serde(default)] pub preconditions: Vec<Precondition>,
    pub min_windows_build: Option<u32>,
    pub profiles: Option<ProfileSourceDef>,
    #[serde(default, rename = "target")] pub targets: Vec<TargetDef>,
    #[serde(default, rename = "action")] pub actions: Vec<ActionDef>,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetDef {
    pub path: String,
    pub include: Option<Vec<String>>,
    #[serde(default)] pub exclude: Vec<String>,
    #[serde(default)] pub recursive: bool,
    pub min_age_hours: Option<u32>,
    #[serde(default)] pub remove_empty_dirs: bool,
    #[serde(default)] pub include_system: bool,
    #[serde(default = "yes")] pub delete_readonly: bool,
    #[serde(default = "yes")] pub optional: bool,
    pub max_depth: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category { System, Browser, Privacy, Apps, Developer }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk { Low, Medium, High }

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DataClass { #[default] Disposable, Personal }

/// Validated, compiled cleaner. Only `validate::validate` can construct it.
pub struct Cleaner {
    id: CleanerId,
    def: CleanerDef,            // kept for `bezem show` and the GUI "what does this remove?" panel
    origin: Origin,             // Builtin | Community { sha256: [u8; 32] }
    scope: Scope,               // User | Machine (derived: requires_admin => Machine)
    targets: Vec<Target>,
    actions: Vec<Action>,
    running_apps: Vec<ExeName>,
}

pub struct Target {
    template: PathTemplate,     // parsed tokens + literal components + wildcard segments
    matcher: TargetMatcher,     // compiled include/exclude GlobSets
    recursive: bool,
    min_age: Duration,
    remove_empty_dirs: bool,
    include_system: bool,
    delete_readonly: bool,
    optional: bool,
    max_depth: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CleanerId(Box<str>);

pub fn validate(def: CleanerDef, origin: Origin) -> Result<Cleaner, Vec<DefinitionError>>;
```

### 3.2 Resolved roots and safety policy

```rust
/// A root path after token expansion and before verification. Untrusted.
pub struct ResolvedRoot {
    pub cleaner: CleanerId,
    pub target_index: usize,
    pub display: PathBuf,               // e.g. C:\Users\Ann\AppData\Local\Temp
    pub profile: Option<ProfileRef>,    // which browser profile produced it
}

/// Produced only by `SafetyPolicy::verify_root`. Holds an open directory handle.
pub struct VerifiedRoot {
    handle: win32::OwnedHandle,         // FILE_LIST_DIRECTORY | FILE_TRAVERSE | SYNCHRONIZE
    final_path: WidePath,               // \\?\C:\Users\Ann\AppData\Local\Temp (normalized, long names)
    identity: FileIdentity,             // volume serial + FILE_ID_128
    base: AllowedBaseId,                // which allowlisted base it lies under
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileIdentity { pub volume_serial: u64, pub file_id: [u8; 16] }

pub struct SafetyPolicy {
    allowed_bases: Vec<AllowedBase>,    // resolved and canonicalized at startup
    protected: Vec<ProtectedPath>,      // never a root, never an ancestor of a root
    privilege: Privilege,               // Unelevated | ElevatedWorker | ElevatedInteractive
    temp_min_age_floor: Duration,       // config can raise, never lower below the definition
    max_entries_per_target: u64,        // circuit breaker, default 5_000_000
}

impl SafetyPolicy {
    pub fn windows_default(known: &KnownFolders, privilege: Privilege) -> Result<Self, SafetyError>;
    pub fn verify_root(&self, root: &ResolvedRoot, scope: Scope) -> Result<VerifiedRoot, SafetyViolation>;
    /// Called by both walker and deleter for every item, using handle-derived facts only.
    pub fn verdict(&self, facts: &ItemFacts, target: &Target, now: SystemTime) -> Verdict;
}

pub enum Verdict { Eligible, Skip(SkipReason) }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    Locked,                 // sharing violation
    AccessDenied,
    ReparsePoint,
    HardLinked,             // NumberOfLinks > 1
    CloudPlaceholder,       // RECALL_ON_DATA_ACCESS / RECALL_ON_OPEN / OFFLINE
    SystemFile,             // FILE_ATTRIBUTE_SYSTEM and !include_system
    ReadOnly,               // read-only and (!delete_readonly or FS lacks IGNORE_READONLY support)
    TooNew,                 // younger than min_age
    Excluded,               // matched exclude glob (user or definition)
    ChangedSinceScan,       // identity, size or write time differs from plan
    Gone,                   // already deleted by someone else (not an error)
    OutsideRoot,            // final path check failed: target aborted, see SafetyViolation
    DeletePending,
    RecycleUnavailable,     // Recycle Bin mode, shell would delete permanently
    FsError,                // anything else; Win32/NT code kept in the log record
}
```

`SafetyPolicy` has no setters that loosen it. `Config` values (exclusions, higher age thresholds) can only
*tighten* it.

### 3.3 Scan report and delete plan

```rust
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScanReport {
    pub format: u32,                    // 1
    pub scan_id: ScanId,                // random 128-bit, ties a plan to its scan
    pub started_at: SystemTime,
    pub duration: Duration,
    pub cleaners: Vec<CleanerScan>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CleanerScan {
    pub id: CleanerId,
    pub status: CleanerStatus,
    pub totals: Totals,                 // files, dirs, bytes (logical), bytes_on_disk (allocation)
    pub targets: Vec<TargetScan>,       // per root: display path, profile, totals
    pub largest: Vec<SampleEntry>,      // up to 20 largest files, for preview
    pub skipped: BTreeMap<SkipReason, u64>,
    pub actions: Vec<ActionPreview>,    // e.g. "Empty Recycle Bin: 1.2 GB, 341 items"
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CleanerStatus {
    Ready,
    NothingToClean,
    NeedsAdmin,                         // not scanned yet; scanned by the elevated worker on request
    Blocked { reason: BlockReason },    // app running, precondition failed
    Unavailable { reason: String },     // app not installed, unsupported build
    Failed { error: String },           // root failed verification etc.
}

/// Sealed: only `Engine::plan` builds one, only from a report the same engine produced.
pub struct DeletePlan {
    scan_id: ScanId,
    mode: DeleteMode,                   // resolved per cleaner (Permanent | Recycle)
    cleaners: Vec<CleanerPlan>,
}

struct CleanerPlan {
    id: CleanerId,
    actions: Vec<Action>,               // ordered; service stops wrap the targets
    targets: Vec<TargetPlan>,
}

struct TargetPlan {
    root: RootRef,                      // re-verified at execute time, identity must match
    root_identity: FileIdentity,
    dirs: DirTable,                     // interned relative directory paths (arena)
    files: Vec<PlanEntry>,
    empty_dir_candidates: Vec<DirIndex>,// post-order, only if remove_empty_dirs
}

struct PlanEntry {
    dir: DirIndex,                      // index into DirTable
    name: Box<[u16]>,                   // long file name as enumerated (UTF-16)
    identity: FileIdentity,
    size: u64,
    last_write: FileTime,
}
```

Memory: a `PlanEntry` is about 72 bytes plus the name; directory paths are interned once. One million entries fit
in under 150 MB; a typical profile scan (50k to 200k entries) stays under 30 MB.

### 3.4 Engine, progress and cancellation

```rust
pub struct EngineConfig {
    pub privilege: Privilege,
    pub config: Config,                 // user settings (exclusions, thresholds, delete mode)
    pub log_dir: Option<PathBuf>,       // None only in tests and in the elevated worker
}

pub struct Engine { /* catalog, resolver, policy, log writer, thread pool */ }

impl Engine {
    pub fn new(catalog: Catalog, cfg: EngineConfig) -> Result<Self, EngineError>;
    pub fn catalog(&self) -> &Catalog;

    /// Expand templates and discover profiles without touching file contents. Used by `show` and the GUI preview.
    pub fn resolve(&self, ids: &[CleanerId]) -> Result<Vec<ResolvedCleaner>, EngineError>;

    pub fn scan(&self, sel: &Selection, opts: &ScanOptions,
                progress: &dyn Progress, cancel: &CancelToken) -> Result<ScanReport, EngineError>;

    pub fn plan(&self, report: &ScanReport, sel: &Selection, opts: &CleanOptions) -> Result<DeletePlan, EngineError>;

    pub fn execute(&self, plan: DeletePlan,
                   progress: &dyn Progress, cancel: &CancelToken) -> Result<CleanSummary, EngineError>;
}

/// Front-ends use this; it hides the split between in-process and elevated work.
pub struct Orchestrator { engine: Engine, elevated: Option<ElevatedSession> }

impl Orchestrator {
    pub fn scan(&mut self, sel: &Selection, opts: &ScanOptions, elevate: Elevate,
                progress: &dyn Progress, cancel: &CancelToken) -> Result<ScanReport, EngineError>;
    pub fn clean(&mut self, report: &ScanReport, sel: &Selection, opts: &CleanOptions,
                 progress: &dyn Progress, cancel: &CancelToken) -> Result<CleanSummary, EngineError>;
}

pub enum Elevate { Never, IfNeeded /* shows UAC once per Orchestrator */ }

pub struct Selection { pub cleaners: Vec<CleanerId> }

pub struct CleanOptions {
    pub delete_mode: DeleteModePref,    // Auto | Permanent | Recycle
    pub temp_min_age_hours: u32,        // >= 24 enforced for temp roots
    pub extra_excludes: Vec<String>,    // globs, absolute-path form, from config or CLI
    pub app_wait: AppWait,              // Fail | Wait { timeout: Duration }
}

pub trait Progress: Send + Sync {
    fn on_event(&self, event: &ProgressEvent);
}
impl<F: Fn(&ProgressEvent) + Send + Sync> Progress for F {
    fn on_event(&self, e: &ProgressEvent) { self(e) }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]  // also travels over IPC
pub enum ProgressEvent {
    PhaseStarted   { phase: Phase },                                   // Resolve | Scan | Clean
    CleanerStarted { id: CleanerId },
    Scanned        { id: CleanerId, files: u64, bytes: u64 },          // cumulative, throttled
    Deleted        { id: CleanerId, files: u64, bytes: u64 },          // cumulative, throttled
    Skipped        { id: CleanerId, path: String, reason: SkipReason },// first 100 per cleaner, then counts only
    WaitingForApps { id: CleanerId, apps: Vec<String> },
    ActionFinished { id: CleanerId, action: ActionKind, outcome: ActionOutcome },
    CleanerFinished{ id: CleanerId, totals: Totals, skipped: u64 },
    LogRecord      (LogRecord),                                        // worker -> parent only
}

#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);
impl CancelToken {
    pub fn cancel(&self);
    pub fn is_cancelled(&self) -> bool;
    pub fn check(&self) -> Result<(), Cancelled>;
}
```

Progress contract:

- Callbacks are invoked from worker threads; implementations must not block. Counter events are coalesced per
  cleaner and emitted at most every 100 ms (10 Hz), so the GUI stays cheap to redraw.
- Cancellation is cooperative. The walker checks the token before each directory batch; the deleter before each
  item. Target latency from `cancel()` to `execute` returning is under 200 ms. A cancelled run still returns a
  `CleanSummary` (marked `cancelled: true`), still writes its log footer and still restores stopped services.

---

## 4. Pipeline and lifecycle

```
 Catalog::builtin()          Engine::resolve           Engine::scan                     Engine::plan     Engine::execute
 parse + validate  ------>  tokens, profiles  ----->  verify roots  ----> walk  ---->  ScanReport ----> DeletePlan ----> CleanSummary
 (bezem-cleaners)           (no file content)        (handles)          (parallel,    (shown to         (sealed)         + JSONL log
                                                                         read-only)    user)
                                                            ^                                                |
                                                            |   identity re-check per item  <----------------+
```

Per cleaner during `execute`:

1. **Gate**: re-check `running_apps` and `preconditions`. If blocked and `app_wait = Wait`, poll every second until
   clear or timeout (never kill processes).
2. **Before actions**: run actions with `when = "before"`; acquire `ServiceGuard`s for `stop_service` actions.
3. **Targets**: for each `TargetPlan`, re-verify the root (identity must equal `root_identity`), then delete
   entries (section 8.4).
4. **After actions**: run actions with `when = "after"` (the default for non-service actions).
5. **Restore**: drop `ServiceGuard`s (restarts services that were running), even on error, cancel or panic.
6. **Report**: emit `CleanerFinished`; append the per-cleaner summary to the log.

Cleaners run concurrently with each other except when they share a service or a precondition, in which case they
are serialized (section 10.1).

---

## 5. Cleaner definition schema (TOML)

### 5.1 Conventions

- UTF-8 TOML 1.0, one cleaner per file, file name `<id-suffix>.toml` under `cleaners/<category>/`.
- Paths should be written as TOML **literal strings** (`'...'`) so backslashes need no escaping. Forward slashes are
  accepted and normalized to `\`.
- Unknown keys are an error (`deny_unknown_fields`). This catches typos such as `min_age_hour` that would otherwise
  silently disable a safety threshold.
- Maximum file size 64 KiB.

### 5.2 Fields

**Top level**

| Field | Type | Default | Rules |
|---|---|---|---|
| `schema_version` | integer | required | Must be `1`. |
| `id` | string | required | `^[a-z0-9]+(?:[._-][a-z0-9]+)*$`, 3 to 64 chars, unique in catalog. Built-ins: first segment equals a category-like namespace (`windows.`, `browser.`, `privacy.`, `apps.`, `dev.`). Community: must start with `community.`. |
| `name` | string | required | 1 to 60 chars, single line. |
| `description` | string | required | 1 to 500 chars. |
| `category` | enum | required | `system`, `browser`, `privacy`, `apps`, `developer`. |
| `group` | string | none | GUI grouping label, e.g. `"Google Chrome"`. 1 to 40 chars. |
| `risk` | enum | required | `low`, `medium`, `high`. |
| `data_class` | enum | `"disposable"` | `disposable` (caches, temp, dumps, logs) or `personal` (history, cookies, sessions, MRU). |
| `requires_admin` | bool | `false` | `true` means the cleaner runs only in the elevated worker. |
| `default_enabled` | bool | `false` | Opt-in is the default. |
| `warning` | string | none | 1 to 200 chars, shown before cleaning. |
| `running_apps` | array of string | `[]` | Executable file names, `^[A-Za-z0-9._ -]{1,64}\.exe$`, max 8. |
| `running_policy` | enum | `"block"` | `block` (refuse while running) or `warn` (allowed, locked files are skipped). |
| `preconditions` | array of enum | `[]` | Closed set: `no_pending_reboot`, `servicing_idle`. |
| `min_windows_build` | integer | none | E.g. `17763`. Cleaner is `Unavailable` on older builds. |
| `profiles` | table | none | Profile discovery, see below. |
| `target` | array of tables | `[]` | 0 to 32 entries. |
| `action` | array of tables | `[]` | 0 to 8 entries. |

**`[profiles]`**

| Field | Type | Default | Rules |
|---|---|---|---|
| `kind` | enum | required | `chromium` or `firefox`. |
| `user_data` | string | required for `chromium` | Path template (no `{...}` placeholders), the Chromium "User Data" directory. |
| `install` | string | `'%APPDATA%\Mozilla\Firefox'` for `firefox` | Directory containing `profiles.ini`. |
| `local` | string | `'%LOCALAPPDATA%\Mozilla\Firefox'` for `firefox` | Base for local (cache) profile directories. |

When `profiles` is set, target paths may start with a placeholder: `{user_data}` and `{profile}` (Chromium), or
`{profile}` (roaming) and `{profile_local}` (local) (Firefox). The target is expanded once per discovered profile.

**`[[target]]`**

| Field | Type | Default | Rules |
|---|---|---|---|
| `path` | string | required | Path template (section 7.1). Must begin with a token or placeholder; no drive letters, no `..`, `.`, `~N` short-name segments, `:` (streams), `\\?\`, `\\.\` or UNC prefixes. |
| `include` | array of string | `["**"]` if `recursive`, else `["*"]` | Globs relative to the root (section 7.3). Max 32. |
| `exclude` | array of string | `[]` | Globs; a match on a directory prunes its subtree. Max 32. |
| `recursive` | bool | `false` | Descend into subdirectories. |
| `min_age_hours` | integer | `24` for temp roots, else `0` | 0 to 8760. For roots under `%TEMP%`, `%TMP%` or `%WINDIR%\Temp`, values below 24 are rejected. Age is `max(LastWriteTime, ChangeTime)`. |
| `remove_empty_dirs` | bool | `false` | Remove directories left empty, strictly below the root. The root itself is never removed. |
| `include_system` | bool | `false` | Allow files with `FILE_ATTRIBUTE_SYSTEM`. |
| `delete_readonly` | bool | `true` | Delete read-only files without modifying their attributes. |
| `optional` | bool | `true` | A missing root is normal (app not installed). If `false`, a missing root marks the cleaner `Unavailable`. |
| `max_depth` | integer | `64` | 1 to 256, depth below the root. |

**`[[action]]`** (section 6 lists the kinds)

| Field | Type | Default | Rules |
|---|---|---|---|
| `kind` | enum | required | One of the closed set. |
| `when` | enum | `"after"` | `before` or `after` the targets. Ignored for `stop_service`, which always wraps the targets. |
| kind-specific parameters | | | `service`, `handler`, `lists` as documented per kind. |

### 5.3 Validation rules (beyond types)

Validation is total: it returns *all* errors, each with the TOML span (via `toml::Spanned`) so `bezem validate`
can print `file:line:col`.

1. Structural: at least one `target` or one `action`.
2. Opt-in rules: `risk = "high"` requires `default_enabled = false` and a `warning`. `data_class = "personal"`
   requires `default_enabled = false` and a `warning`. `category = "privacy"` requires `default_enabled = false`.
3. Scope: if `requires_admin = true`, every target path must start with a **machine-scope** token (`%WINDIR%`,
   `%SYSTEMROOT%`, `%PROGRAMDATA%`) and `profiles` must be absent. If `false`, machine-scope roots are rejected. Rationale in section 9.1
   (over-the-shoulder elevation).
4. Root shape: after the leading token there must be at least one literal component (a cleaner cannot target
   `%LOCALAPPDATA%` itself). Wildcard segments (`*`, `?` within one component; no `**`) are allowed in the root,
   but not as the first component after the token, and at most two per path.
5. Root allowlist: the *template* must fall under an allowlisted base (section 8.3) and must not equal or contain a
   protected path. This check is repeated at run time on the canonical path.
6. Globs compile with Bezem's options; `include` patterns must not start with `/` or `\` or contain `..`.
7. `running_apps` entries are file names only (no path separators).
8. Actions: kind-specific parameters are present and in their allowlists; `stop_service` requires
   `requires_admin = true`; at most one action of each kind, except `stop_service` (max 3).
9. Community origin (section 8.6): `requires_admin = false`, no actions, `risk >= medium`, `default_enabled = false`,
   no wildcard root segments, `id` starts with `community.`.

### 5.4 Examples

**User temp files**

```toml
schema_version  = 1
id              = "windows.user_temp"
name            = "User temporary files"
description     = "Files that applications leave behind in your temporary folder (%TEMP%)."
category        = "system"
risk            = "low"
default_enabled = true

[[target]]
path              = '%TEMP%'
recursive         = true
min_age_hours     = 24           # also the enforced minimum for temp roots
remove_empty_dirs = true
exclude           = ["*.lock", "**/*.lock"]
```

**Google Chrome cache (multi-profile)**

```toml
schema_version  = 1
id              = "browser.chrome.cache"
name            = "Google Chrome: cache"
description     = "Web cache, code cache and GPU shader caches for every Chrome profile. Does not log you out."
category        = "browser"
group           = "Google Chrome"
risk            = "low"
default_enabled = true
running_apps    = ["chrome.exe"]
running_policy  = "block"

[profiles]
kind      = "chromium"
user_data = '%LOCALAPPDATA%\Google\Chrome\User Data'

[[target]]
path      = '{profile}\Cache\Cache_Data'
recursive = true

[[target]]
path      = '{profile}\Code Cache'
recursive = true

[[target]]
path      = '{profile}\GPUCache'
recursive = true

[[target]]
path      = '{user_data}\ShaderCache'
recursive = true

[[target]]
path      = '{user_data}\GrShaderCache'
recursive = true
```

**Windows Update download cache (admin, service-wrapped)**

```toml
schema_version  = 1
id              = "windows.update_download_cache"
name            = "Windows Update download cache"
description     = "Update packages that were already downloaded. Windows downloads them again if needed."
category        = "system"
risk            = "low"
requires_admin  = true
default_enabled = true
preconditions   = ["no_pending_reboot", "servicing_idle"]

[[action]]
kind    = "stop_service"
service = "wuauserv"

[[action]]
kind    = "stop_service"
service = "bits"

[[target]]
path              = '%WINDIR%\SoftwareDistribution\Download'
recursive         = true
remove_empty_dirs = true
```

**Firefox cookies (personal data, opt-in)** (short form)

```toml
schema_version = 1
id             = "browser.firefox.cookies"
name           = "Firefox: cookies"
description    = "Removes cookies for every Firefox profile."
category       = "browser"
group          = "Mozilla Firefox"
risk           = "medium"
data_class     = "personal"
warning        = "You will be logged out of websites."
running_apps   = ["firefox.exe"]

[profiles]
kind = "firefox"

[[target]]
path    = '{profile}'
include = ["cookies.sqlite", "cookies.sqlite-wal", "cookies.sqlite-shm"]
```

---

## 6. Built-in special actions

Actions are a closed Rust enum. A definition can name an action and pass parameters from an allowlist; it cannot
supply executables, arguments, registry paths, CLSIDs or service names outside the tables below.

```rust
pub enum Action {
    EmptyRecycleBin,
    FlushDns,
    StopService { service: AllowedService },          // StartService is the implicit restore step
    ClearClipboard { history: bool },
    ClearRecentDocs,
    ClearExplorerMru { lists: Vec<MruList> },
    VolumeCache { handler: AllowedVolumeCache },
}
```

| `kind` | Parameters | Elevation | Implementation | Preview in scan |
|---|---|---|---|---|
| `empty_recycle_bin` | none | no | `SHEmptyRecycleBinW(null, null, SHERB_NOCONFIRMATION \| SHERB_NOPROGRESSUI \| SHERB_NOSOUND)`. Empties the current user's bins on all drives. | `SHQueryRecycleBinW(null)`: size and item count. |
| `flush_dns` | none | no | `DnsFlushResolverCache` from `dnsapi.dll` (loaded with `LOAD_LIBRARY_SEARCH_SYSTEM32`). Fallback: `CreateProcessW` of the absolute path `%WINDIR%\System32\ipconfig.exe` with the fixed argument `/flushdns`, no shell, no inherited handles. | Not measurable; shown as "action". |
| `stop_service` | `service`: `wuauserv`, `bits`, `DoSvc`, `FontCache` | yes | `ServiceGuard` (section 9.3). Stops before targets, restores the previous state after. There is no declarable `start_service`: starting is only ever the restore half of a stop, so a definition cannot leave the system with a service in a state it did not find it in. | Shown as "will stop and restart X". |
| `clear_clipboard` | `history` (bool, default `false`) | no | `OpenClipboard(null)` with 10 retries of 50 ms, `EmptyClipboard`, `CloseClipboard`. If `history`, WinRT `Clipboard::ClearHistory()`. | Shows whether the clipboard holds data (`CountClipboardFormats`). |
| `clear_recent_docs` | none | no | `SHAddToRecentDocs(SHARD_PIDL, null)`. Jump-list files are ordinary targets (`%APPDATA%\Microsoft\Windows\Recent\AutomaticDestinations` etc.). | Item count from enumerating `%RECENT%`. |
| `clear_explorer_mru` | `lists`: subset of `run`, `typed_paths`, `wordwheel` | no | Deletes *values* (never keys) under fixed HKCU keys: `...\Explorer\RunMRU` (all values incl. `MRUList`), `...\Explorer\TypedPaths` (`url1` to `urlN`), `...\Explorer\WordWheelQuery` (all values). Keys are hard-coded in the binary. | Value counts. |
| `volume_cache` | `handler`: `Delivery Optimization Files`, `D3D Shader Cache` (later: `Update Cleanup`, `Previous Installations`) | per handler (DO: yes) | The supported Disk Cleanup mechanism: read the handler CLSID from `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VolumeCaches\<handler>`, `CoCreateInstance(CLSCTX_INPROC_SERVER)` for `IEmptyVolumeCache2`, `InitializeEx`, `GetSpaceUsed`, `Purge`. Hardening: the CLSID must be in an embedded allowlist, its `InprocServer32` in HKLM must resolve under `%WINDIR%\System32`, and a per-user override (`HKCU\Software\Classes\CLSID\{...}`) aborts the action. | `GetSpaceUsed`. |

Not actions on purpose: killing processes, changing service start types, arbitrary registry edits, running
`cleanmgr` or PowerShell, scheduling tasks.

---

## 7. Path resolution

### 7.1 Templates and tokens

A path template is `TOKEN ( '\' segment )*` or `PLACEHOLDER ( '\' segment )*`. Tokens look like environment
variables for readability (and to match PLAN section 3.1), but **they are not read from the environment**. Each
token maps to an API:

| Token | Resolution | Scope |
|---|---|---|
| `%TEMP%`, `%TMP%` | `GetTempPath2W` (returns `C:\Windows\SystemTemp` for SYSTEM, the user temp dir otherwise) | user |
| `%LOCALAPPDATA%` | `SHGetKnownFolderPath(FOLDERID_LocalAppData, KF_FLAG_DEFAULT)` | user |
| `%LOCALAPPDATALOW%` | `FOLDERID_LocalAppDataLow` | user |
| `%APPDATA%` | `FOLDERID_RoamingAppData` | user |
| `%RECENT%` | `FOLDERID_Recent` | user |
| `%USERPROFILE%` | `FOLDERID_Profile` (only usable for sub-paths that fall under an allowlisted base) | user |
| `%PROGRAMDATA%` | `FOLDERID_ProgramData` | machine |
| `%WINDIR%`, `%SYSTEMROOT%` | `FOLDERID_Windows` | machine |

Resolution rules:

- `SHGetKnownFolderPath` is called with `KF_FLAG_DEFAULT`; the returned `PWSTR` is copied and freed with
  `CoTaskMemFree` inside the `win32` wrapper.
- A token that fails to resolve, resolves to an empty string, to a relative path, to a path shorter than 4
  characters (`C:\x`), to a drive root, or to a non-local volume makes every target using it **Unresolved**
  (reported, never guessed). This rules out the classic `"%X%\Temp"` -> `"\Temp"` expansion bug: the code never
  concatenates strings, it pushes `PathComponent`s onto an already-validated absolute `WidePath` (8.3).
  `SHGetKnownFolderPath` returns the Win32 `C:\...` form, which `WidePath::parse_extended` rightly rejects, so
  `resolve` needs a second entry point that splits that form into `PathComponent`s pushed onto `drive_root`. It must
  not reopen what `parse_extended` closes: only `X:\` with an ASCII letter, no `/` separators, no empty, `.` or `..`
  components, no prefixes, UNC or device paths.
- The matching environment variable is read only as a cross-check; a mismatch is logged at `debug` and the API value
  wins.
- `%TEMP%` is resolved once per process. In the elevated worker user-scope tokens are not available at all
  (`resolve` returns `ScopeError`).
- Placeholders (`{user_data}`, `{profile}`, `{profile_local}`) are expanded by profile discovery (7.4, 7.5) and are
  only valid when `[profiles]` is present.

After expansion, the root path is still **untrusted**. It becomes trusted only through
`SafetyPolicy::verify_root` (section 8.3).

### 7.2 Root wildcards

A root segment may contain `*` or `?` (for example `'%LOCALAPPDATA%\Packages\*\AC\INetCache'`). Expansion enumerates
the parent directory by handle, considers only directories that are not reparse points, matches names
case-insensitively, and produces one `ResolvedRoot` per match (cap: 512 per target). Each expanded root is verified
separately.

### 7.3 Glob semantics

Built on `globset` with these fixed options: `case_insensitive(true)`, `literal_separator(true)`,
`backslash_escape(false)`.

- Patterns are matched against the path **relative to the root**, components joined with `/`. A `\` in a pattern is
  rewritten to `/` before compiling, so there is no escape character; use `[*]` or `[?]` to match those characters
  literally (they are invalid in Windows file names anyway).
- `*` and `?` never cross a separator. `**` matches zero or more whole components and is only valid as a complete
  component (`**/x`, `a/**`, `**`).
- `[abc]`, `[!abc]`, `{a,b}` are supported. There is no negation; use `exclude`.
- Only **long names** are matched. The walker reads names from `FILE_ID_EXTD_DIR_INFO.FileName`, which is the long
  name; 8.3 alternate names are never seen.
- `include` applies to files only. `exclude` applies to both: a directory whose relative path matches an exclude
  pattern (or `pattern/**`) is pruned, and nothing under it is counted or deleted.
- `recursive = false` means only direct children of the root are considered, regardless of `**` in `include`.
- Case folding is Unicode simple case folding (the `regex` `(?i)` semantics). NTFS uses its upcase table; the two
  agree for all names that matter in practice, and the safety of a deletion never depends on a glob (globs only
  narrow what is eligible).
- User exclusions from the config are absolute-path globs (`'%LOCALAPPDATA%\Temp\keep-me\**'`); they are expanded
  with the same token rules and converted to root-relative patterns for every root they overlap.

### 7.4 Chromium profile discovery

Input: `user_data` (e.g. `%LOCALAPPDATA%\Microsoft\Edge\User Data`).

1. Verify `user_data` as a root (section 8.3). If missing, the cleaner is `Unavailable` ("not installed").
2. Open `Local State` relative to the verified handle (no reparse point, size <= 16 MiB), parse with `serde_json`.
   Profile directory names are the keys of `profile.info_cache`.
3. Fallback if `Local State` is missing or malformed: enumerate subdirectories named `Default`, `Profile <n>`
   (`n` decimal) or `Guest Profile`.
4. Validate every name: one component, `^[A-Za-z0-9 _.-]{1,64}$`, not `.` or `..`, no trailing dot or space.
   The directory must exist, must not be a reparse point and must contain a `Preferences` file.
5. Each accepted profile becomes a `ProfileRef { browser, dir_name, display_name }` (`display_name` from
   `info_cache.<dir>.name`, for the UI only). `{profile}` = `user_data\<dir_name>`.

Bezem never writes to `Local State` or `Preferences`.

Known `user_data` locations used by the built-ins: Chrome `%LOCALAPPDATA%\Google\Chrome\User Data`, Edge
`%LOCALAPPDATA%\Microsoft\Edge\User Data`, Brave `%LOCALAPPDATA%\BraveSoftware\Brave-Browser\User Data`, Vivaldi
`%LOCALAPPDATA%\Vivaldi\User Data`. Opera is single-profile and uses explicit paths
(`%APPDATA%\Opera Software\Opera Stable`, cache under `%LOCALAPPDATA%\Opera Software\Opera Stable\Cache`) without
`[profiles]`.

### 7.5 Firefox profile discovery

Input: `install` (`%APPDATA%\Mozilla\Firefox`) and `local` (`%LOCALAPPDATA%\Mozilla\Firefox`).

1. Read `profiles.ini` relative to the verified `install` handle (size <= 1 MiB). A small built-in INI parser
   handles sections, `key=value`, `;`/`#` comments and a UTF-8 BOM.
2. For each `[Profile<N>]` section read `Path`, `IsRelative` (default `1`) and `Name`. `[Install<hash>] Default=`
   only marks the default profile for display.
3. `Path` uses `/`; it is split into components, each validated (no `..`, `.`, `:`, empty components).
4. `IsRelative=1`: `{profile}` = `install\<Path>`, `{profile_local}` = `local\<Path>`.
   `IsRelative=0` (custom location): `{profile}` = `Path` only if its canonical form falls under an allowlisted base;
   `{profile_local}` targets are skipped with reason "profile location unsupported", because the local directory of
   a custom-location profile cannot be derived reliably.
5. Secondary running signal: if `{profile}\parent.lock` cannot be opened for delete (Firefox holds it exclusively),
   the profile is treated as in use even if no `firefox.exe` is visible (for example a portable Firefox with another
   executable name).

Firefox history lives in `places.sqlite` together with bookmarks, so it cannot be cleaned by deleting files. Bezem
1.0 does not edit SQLite databases; Firefox history cleaning is out of scope until a safe approach is designed
(Chromium's `History` file holds no bookmarks, so Chromium history is cleanable in 1.0).

---

## 8. Safety layer

### 8.1 Threat model

Assets: every file outside the intended cache/temp locations, and the integrity of the system when Bezem runs
elevated. Attackers: (a) a malicious or buggy cleaner definition, (b) a same-user process that races Bezem, and,
most importantly, (c) a **less-privileged** process that plants links in a user-writable folder that the elevated
worker cleans (`C:\Windows\Temp` is writable by standard users). Privileged arbitrary-file-deletion bugs in cleanup
tools are a known local privilege-escalation primitive, so (c) is treated as a security vulnerability class.

| Threat | Example | Mitigation |
|---|---|---|
| Junction / symlink / mount point escape | `%TEMP%\x` is a junction to `C:\Windows\System32` | Every open below a verified root uses `FILE_OPEN_REPARSE_POINT`, and any object with `FILE_ATTRIBUTE_REPARSE_POINT` (any tag) is skipped, never traversed and never deleted. Enumeration reads `ReparsePointTag` from the directory listing itself. |
| Object-manager symlink tricks | Mount point to `\RPC Control`, object symlink to a target file | Covered by the reparse-point rule; additionally the final-path check (8.4 step 8) rejects anything not on a `\\?\X:\` DOS path under the root. |
| TOCTOU: path swapped between check and use | Attacker replaces `Temp\a` with a junction after the scan, or between the check and the delete, possibly using an oplock to win the race | No path below the root is re-resolved by the OS. Each component is opened **relative to the already-open handle** of its verified parent (`NtCreateFile` with `OBJECT_ATTRIBUTES.RootDirectory`), so there is no intermediate component to swap. Directory handles are opened without `FILE_SHARE_DELETE`, so the directory cannot be renamed or replaced while held. The delete is applied **to the same handle** that was checked. |
| Stale scan | File replaced by a different file with the same name | `PlanEntry.identity` (volume serial + 128-bit file ID) must match the opened handle; otherwise `ChangedSinceScan`. |
| Hard links | Attacker hard-links `Temp\x` to a protected file; deleting would only remove the link, but attribute changes would affect the target, and final-path checks would see a foreign name | Files with `NumberOfLinks > 1` are skipped (`HardLinked`). Bezem never modifies attributes (read-only files are deleted with `FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE`). |
| 8.3 short names | `C:\PROGRA~1` evading a string denylist | No security decision uses a user- or definition-supplied string. Roots and items are compared using `GetFinalPathNameByHandleW(FILE_NAME_NORMALIZED)` output, which contains long names. The validator rejects `~<digit>` segments in templates. Globs see long names only. |
| `\\?\`, `\\.\`, `\\?\GLOBALROOT`, `\\?\Volume{...}` | Definition writes `\\?\C:\Windows` to bypass normalization | Templates cannot contain prefixes (validator). Internally all paths are extended-length `\\?\X:\...` built from API results; any resolved path that is not of that form is rejected. |
| UNC and network drives | `%LOCALAPPDATA%` redirected to `\\server\share`, mapped drive letters | Roots must be on a local fixed volume: `GetVolumeInformationByHandleW` + `FileFsDeviceInformation` must not report `FILE_REMOTE_DEVICE`, and `GetDriveTypeW` on the volume root must be `DRIVE_FIXED`. UNC final paths (`\\?\UNC\`) are rejected. |
| Empty / missing tokens | `%X%` expands to empty, giving `\Temp` (drive root of cwd) | Tokens come from APIs, empties are errors, and paths are built by component push onto an absolute validated base (7.1). Drive roots are protected paths. |
| Alternate data streams | `file.txt:stream`, `$I30` names | `:` is rejected in every name component (template, profile names, enumerated names are never re-parsed). Enumeration does not list streams. |
| Trailing dots/spaces, reserved names | `con`, `aux.txt`, `x.` | Extended-length NT paths and handle-relative opens bypass Win32 name normalization; these names are handled literally and safely. |
| Cloud placeholders | Deleting a OneDrive placeholder triggers a download or cloud delete | Files with `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`, `FILE_ATTRIBUTE_RECALL_ON_OPEN` or `FILE_ATTRIBUTE_OFFLINE` are skipped (`CloudPlaceholder`). OneDrive roots are protected paths. |
| Files in use | Deleting a temp file an app is writing | Leaf opens use share mode `FILE_SHARE_READ` only, so the open fails with a sharing violation if any other handle has write or delete access (`Locked`). Plus age thresholds and the running-app gate. |
| Malicious definitions | Root `%LOCALAPPDATA%`, `**` everywhere, `requires_admin` to get elevated deletes | Validator (5.3), run-time root allowlist (8.3), community restrictions (8.6). Elevated worker accepts only built-in IDs (9.1). |
| Runaway deletion due to a bug | A logic error selecting a huge tree | `max_entries_per_target` circuit breaker; a target whose plan exceeds it is aborted. The root itself is never deleted. |
| DLL planting next to a portable exe | `version.dll` in `Downloads` loaded by elevated `bezem.exe` | Linker flag `/DEPENDENTLOADFLAG:0x800` (System32 only for static imports, set via `cargo:rustc-link-arg` in `build.rs`); first call in `main` is `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)`; the worker sets its current directory to `%WINDIR%\System32`. |
| Elevated worker driven by a malicious parent | A same-user process connects to the worker and requests deletion of `C:\Windows` | The worker never accepts paths, only built-in cleaner IDs and tightening options (9.1). |

Out of scope: an attacker who is already administrator, or malware running as the same user that replaces
`bezem.exe` itself (the UAC prompt shows the signer; that is the protection Windows offers).

### 8.2 Invariants

The test suite asserts these directly.

1. Nothing is deleted outside a `VerifiedRoot`, and a root is never deleted itself.
2. No reparse point is ever traversed or deleted.
3. Every deletion is applied to a handle whose identity, attributes, link count, age and final path were checked on
   that same handle.
4. The elevated process never resolves a user-scope path and never receives a path over IPC.
5. A failure to verify is a skip, never a fallback to a weaker method.

### 8.3 Allowlisted bases, protected paths, root verification

**Allowlisted bases** (resolved via section 7.1 at startup, then canonicalized by opening them and calling
`GetFinalPathNameByHandleW`):

| Base | Scope |
|---|---|
| `%LOCALAPPDATA%`, `%LOCALAPPDATALOW%`, `%APPDATA%`, `%TEMP%`, `%RECENT%` (strict descendants only) | user |
| `%WINDIR%\Temp`, `%WINDIR%\SoftwareDistribution\Download`, `%WINDIR%\Minidump`, `%WINDIR%\ServiceProfiles\LocalService\AppData\Local\FontCache`, `%WINDIR%\SystemTemp` | machine |
| `%PROGRAMDATA%\Microsoft\Windows\WER` | machine |

A root must be **equal to or below** a base that lists the root's own token family (for `%WINDIR%\Temp`, equal is
allowed; for `%LOCALAPPDATA%` only strictly below).

**Protected paths** (a root may not equal one, and may not be an ancestor of one): every drive root, `%WINDIR%`,
`%WINDIR%\System32`, `%WINDIR%\SysWOW64`, `%WINDIR%\WinSxS`, `%WINDIR%\Installer`, `FOLDERID_ProgramFiles`,
`FOLDERID_ProgramFilesX86`, `FOLDERID_Profile`, `FOLDERID_Desktop`, `FOLDERID_Documents`, `FOLDERID_Downloads`,
`FOLDERID_Pictures`, `FOLDERID_Music`, `FOLDERID_Videos`, `FOLDERID_SkyDrive` (OneDrive), and the base folders
`%LOCALAPPDATA%`, `%APPDATA%`, `%PROGRAMDATA%` themselves. Known-folder redirection is honored automatically
because they are resolved through the API.

**`verify_root` algorithm**

```
verify_root(root, scope):
  1. require scope(root.token) == cleaner.scope; in ElevatedWorker require scope == Machine
  2. p = root.display as \\?\-prefixed absolute path (built from components, never parsed)
  3. h = CreateFileW(p, FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                     FILE_SHARE_READ | FILE_SHARE_WRITE,                 // no SHARE_DELETE: pins the dir
                     OPEN_EXISTING,
                     FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
       NOT_FOUND -> Missing (Unavailable if !optional)
  4. GetFileInformationByHandleEx(h, FileAttributeTagInfo): must be a directory, must not be a reparse point
     (ancestors of the root may be links, e.g. a relocated profile; the root itself and everything below may not)
  5. final = GetFinalPathNameByHandleW(h, FILE_NAME_NORMALIZED | VOLUME_NAME_DOS)
     must start with \\?\<letter>:\ ; \\?\UNC\ and anything else -> reject
  6. local fixed volume check (FILE_REMOTE_DEVICE, DRIVE_FIXED)
  7. base = the allowlisted base whose canonical final path is a prefix of `final` at a component boundary
     (CompareStringOrdinal(..., bIgnoreCase = TRUE)), same volume serial; none -> SafetyViolation
  8. for every protected path P (canonical): reject if final == P or P is below final
  9. identity = FileIdInfo(h)
 10. return VerifiedRoot { handle: h, final_path: final, identity, base }
```

**Path types and comparison** (`safety::path`, pure logic, tested on Linux too). A `WidePath` is always
`\\?\X:\` plus validated `PathComponent`s; it is built by pushing components, and the only parser accepts exactly
that form (for `GetFinalPathNameByHandleW` output). A component is never empty, `.` or `..`, has at most 255 UTF-16
units and contains no `\`, `/`, `:`, NUL, other control character (U+0001..U+001F) or Win32 wildcard
(`* ? < > " |`); trailing dots/spaces and device names are kept literally. Names are stored as UTF-16 code units, so
unpaired surrogates round-trip. Equality, hashing and containment (`starts_with`, `is_descendant_of`,
`strip_prefix`) are component-wise, ordinal and case-insensitive, emulating `CompareStringOrdinal(.., TRUE)`: each
UTF-16 unit is uppercased on its own (ASCII; otherwise its Unicode simple uppercase if that is one non-ASCII BMP
character; surrogates unchanged). So `ß` != `SS`, `İ` != `i`, and `ı`/`ſ` do not fold onto ASCII, as on Windows.
This equality is **lexical**, using the pinned toolchain's Unicode tables: it may fold pairs that an older NTFS
`$UpCase` table keeps apart (U+1C80 vs U+0412, Georgian Mtavruli U+1C90 vs U+10D0) and it ignores per-directory case
sensitivity. Over-folding loosens base containment (step 7) and the final-path tripwire (8.4 step 8) and tightens the
protected-path check (step 8). It is therefore never the sole containment guard: handle-relative opens below a
verified root and file identity (volume serial + file ID) are. A test cross-checking the folding against
`CompareStringOrdinal` on Windows is TODO for the `win32` layer, which is the only place that FFI may live.

### 8.4 Delete-time algorithm (per file)

Executed by `delete::execute_target` for each `PlanEntry`, after the root has been re-verified with
`verify_root` and its identity compared to `TargetPlan.root_identity` (mismatch aborts the target).

```
delete_entry(root: &VerifiedRoot, e: &PlanEntry, t: &Target, dircache):
  1. parent = dircache.get_or_open(e.dir):
       for each component c of the relative dir path, starting from root.handle:
         reject c unless it is a valid PathComponent (8.3): not empty, ".", "..", no '\\', '/', ':', NUL, ...
         h = NtCreateFile(RootDirectory = <previous handle>, ObjectName = c,
                          DesiredAccess = FILE_LIST_DIRECTORY | FILE_TRAVERSE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                          ShareAccess   = FILE_SHARE_READ | FILE_SHARE_WRITE,          // no SHARE_DELETE
                          Disposition   = FILE_OPEN,
                          Options       = FILE_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT
                                        | FILE_SYNCHRONOUS_IO_NONALERT
                                        | (elevated ? FILE_OPEN_FOR_BACKUP_INTENT : 0),
                          OBJ_CASE_INSENSITIVE)
         FileAttributeTagInfo(h): reparse point -> skip whole subtree (ReparsePoint)
       (handles are cached per worker thread in a small LRU, depth-first order keeps hit rate high)

  2. f = NtCreateFile(RootDirectory = parent, ObjectName = e.name,
                      DesiredAccess = DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                      ShareAccess   = FILE_SHARE_READ,                                 // fail if anyone writes/deletes
                      Disposition   = FILE_OPEN,
                      Options       = FILE_NON_DIRECTORY_FILE | FILE_OPEN_REPARSE_POINT
                                    | FILE_SYNCHRONOUS_IO_NONALERT)
       STATUS_SHARING_VIOLATION -> Locked
       STATUS_ACCESS_DENIED     -> AccessDenied
       STATUS_OBJECT_NAME_NOT_FOUND / PATH_NOT_FOUND -> Gone
       STATUS_DELETE_PENDING    -> DeletePending
       STATUS_FILE_IS_A_DIRECTORY -> ChangedSinceScan

  3. GetFileInformationByHandleEx(f, FileAttributeTagInfo)
       REPARSE_POINT -> ReparsePoint
       RECALL_ON_DATA_ACCESS | RECALL_ON_OPEN | OFFLINE -> CloudPlaceholder
       SYSTEM && !t.include_system -> SystemFile
       READONLY && !t.delete_readonly -> ReadOnly

  4. GetFileInformationByHandleEx(f, FileIdInfo)
       (VolumeSerialNumber, FileId) != e.identity -> ChangedSinceScan

  5. GetFileInformationByHandleEx(f, FileStandardInfo)
       NumberOfLinks > 1 -> HardLinked ;  DeletePending -> DeletePending
       EndOfFile != e.size -> ChangedSinceScan

  6. GetFileInformationByHandleEx(f, FileBasicInfo)
       age = now - max(LastWriteTime, ChangeTime); age < t.min_age -> TooNew
       LastWriteTime != e.last_write -> ChangedSinceScan

  7. exclusions re-checked on the plan's relative path (cheap, already long names)

  8. final = GetFinalPathNameByHandleW(f, FILE_NAME_NORMALIZED | VOLUME_NAME_DOS)
       must equal root.final_path + "\" + rel_dir + "\" + e.name (ordinal, case-insensitive)
       mismatch -> OutsideRoot: abort this TARGET, log SafetyViolation at error level, surface in the UI.
       (With handle-relative opens this cannot happen; it is a tripwire for bugs and unknown attack classes.)

  9. SetFileInformationByHandle(f, FileDispositionInfoEx,
         FILE_DISPOSITION_INFO_EX { Flags = FILE_DISPOSITION_FLAG_DELETE
                                          | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS
                                          | (t.delete_readonly ? FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE : 0) })
       ERROR_INVALID_PARAMETER / ERROR_NOT_SUPPORTED (FAT32, exFAT, some filter drivers):
         fallback SetFileInformationByHandle(f, FileDispositionInfo, { DeleteFile = TRUE })
         (read-only files then fail with ACCESS_DENIED -> ReadOnly; attributes are never cleared)

 10. CloseHandle(f). With POSIX semantics the name is gone at this point, even if another process
     still has a FILE_SHARE_DELETE handle open.
 11. emit LogRecord::Deleted { path: final (without \\?\), bytes: e.size }
     (this display path is for people only and is never parsed back into a WidePath)
```

**Empty directories** (`remove_empty_dirs = true`): after all files of a target are processed, candidate
directories are visited in post-order. The cached handle for the directory is closed, then the directory is opened
relative to its parent with `DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE`, `FILE_DIRECTORY_FILE |
FILE_OPEN_REPARSE_POINT`, its identity and non-reparse status are checked, and `FileDispositionInfoEx` is set.
`STATUS_DIRECTORY_NOT_EMPTY` simply leaves it in place. The root is never a candidate.

**Scan reuses the same primitives.** The walker opens directories exactly as in step 1 and reads entries with
`GetFileInformationByHandleEx(FileIdExtdDirectoryInfo)` (64 KiB buffers), which returns long name, attributes,
`ReparsePointTag`, sizes, times and the 128-bit `FileId` for every entry in bulk, without opening each file. On file
systems without `FileIdExtdDirectoryInfo` (FAT/exFAT) it falls back to `FileIdBothDirectoryInfo` (64-bit ID,
zero-extended) or, if that fails, `FileFullDirectoryInfo` with identity checks replaced by size + write time.

### 8.5 Recycle Bin mode

Recycle mode uses `IFileOperation` (`FOF_ALLOWUNDO | FOFX_RECYCLEONDELETE | FOF_NOCONFIRMATION | FOF_SILENT |
FOF_NOERRORUI`) on a dedicated STA thread, in batches of 500 items. Each item is first fully verified with steps
1 to 8 above; the handle is then closed and the verified final path is handed to the shell. An
`IFileOperationProgressSink::PreDeleteItem` implementation checks `TSF_DELETE_RECYCLE_IF_POSSIBLE` in `dwFlags`;
if absent (item too large for the bin, or the volume has no bin), it returns a failure HRESULT so the shell skips
the item, which is reported as `RecycleUnavailable`. Bezem never silently turns a recycle into a permanent delete.

Items with a component that ends in a dot or space, or that is a DOS device name (`CON`, `NUL`, `COM1`, ...), are
skipped in Recycle mode (`RecycleUnavailable`), because the shell may normalize such names to a different file.
The shell API is path-based, so there is a small window between verification and the move. That is acceptable only
because **Recycle mode is restricted to the unelevated process**: an attacker able to race it already has the same
rights as Bezem. The elevated worker always deletes permanently through handles.

Default mode per cleaner: `data_class = "disposable"` -> permanent; `data_class = "personal"` -> Recycle Bin
(PLAN 3.3 item 6). The user setting or `--recycle` / `--permanent` overrides.

### 8.6 Community definitions (post-1.0)

- Import is explicit (`bezem cleaners import <file>` or the GUI dialog), shows the validation result, the resolved
  paths on this machine, and a diff against any existing version.
- Accepted files are copied to `%LOCALAPPDATA%\Bezem\cleaners\community\` and their SHA-256 recorded in the config.
  A file whose hash no longer matches is ignored until re-approved.
- Restrictions (validator rule 9): user scope only, no actions, no root wildcards, `risk >= medium`, never enabled by
  default, ID namespace `community.` (cannot shadow built-ins). The UI labels them "Community" with the file path.
- They are never sent to the elevated worker.

---

## 9. Elevation, services and running applications

### 9.1 Separate elevated worker

What runs where:

| Unelevated main process | Elevated worker |
|---|---|
| UI, config, catalog loading, community definitions | Built-in cleaners with `requires_admin = true` only |
| All user-scope cleaners (`%TEMP%`, browsers, `%APPDATA%`) | Machine-scope roots only (`%WINDIR%`, `%PROGRAMDATA%`) |
| Privacy actions touching HKCU, clipboard, Recycle Bin | `stop_service`, admin `volume_cache` handlers |
| Recycle Bin delete mode | Permanent, handle-based delete only |
| Writing the deletion log (also for worker records) | No file writes outside the cleaned roots |

Why user-scope work never moves to the worker: with over-the-shoulder elevation (a standard user types an admin's
credentials), the elevated process runs as the **admin account**, so its `%LOCALAPPDATA%`, `%TEMP%` and HKCU belong
to someone else. Restricting the worker to machine scope makes it correct in both the split-token and the
over-the-shoulder case.

If Bezem is started already elevated (admin terminal), it runs everything in-process with
`Privilege::ElevatedInteractive` (handle-based deletes, no Recycle mode for machine-scope cleaners) and prints which
profile user-scope cleaners will act on. Running as `LocalSystem` is refused for user-scope cleaners.

**Launch**

```
main (medium IL)                                          worker (high IL)
----------------                                          ----------------
name = \\.\pipe\bezem-<pid>-<32 hex random (BCryptGenRandom)>
CreateNamedPipeW(name,
   PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
   PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
   1 instance, 64 KiB, 64 KiB,
   SDDL "D:P(A;;GA;;;<current user SID>)(A;;GA;;;BA)S:(ML;;NW;;;LW)")
ShellExecuteExW(lpVerb="runas", lpFile=<own exe path>,
   lpParameters="--bezem-elevated-worker --pipe <name> --parent <pid> --proto 1",
   fMask=SEE_MASK_NOCLOSEPROCESS, hwnd=<GUI window or console>, nShow=SW_HIDE)
   ERROR_CANCELLED (1223) -> ElevationError::Declined
ConnectNamedPipe (overlapped, 30 s timeout, cancellable)
                                                          SetDefaultDllDirectories(SYSTEM32); chdir System32
                                                          verify TokenElevation == elevated
                                                          CreateFileW(pipe, ..., SECURITY_SQOS_PRESENT |
                                                                      SECURITY_IDENTIFICATION)
                                                          GetNamedPipeServerProcessId == --parent, else exit
GetNamedPipeClientProcessId == GetProcessId(hProcess), else kill + fail
<-------------------------- Hello { proto: 1, version } -----------------
-- Hello ---------------------------------------------->
                                                          OpenProcess(SYNCHRONIZE, parent); exit when it exits
```

`SECURITY_IDENTIFICATION` on the client side prevents the medium-integrity pipe server from impersonating the
elevated client token. The mutual PID checks prevent another process from squatting the pipe name or connecting in
place of the worker.

**Protocol**: frames are `u32` little-endian length + JSON (`serde_json`), max 1 MiB per frame; the reader rejects
larger frames and unknown message types (protocol version mismatch -> both sides exit).

```rust
enum Request {
    Hello  { proto: u32 },
    Scan   { cleaners: Vec<CleanerId>, opts: WorkerScanOptions },
    Clean  { scan_id: ScanId, cleaners: Vec<CleanerId>, opts: WorkerCleanOptions },
    Cancel,
    Shutdown,
}
enum Response {
    Hello     { proto: u32, version: String },
    Progress  (ProgressEvent),                // includes LogRecord, written by the parent
    ScanDone  (ScanReport),
    CleanDone (CleanSummary),
    Error     (WireError),
}
struct WorkerCleanOptions { temp_min_age_hours: u32, extra_excludes: Vec<String>, app_wait: AppWait }
```

The worker treats the parent as untrusted:

- `cleaners` must be IDs of **embedded built-in** cleaners with `requires_admin = true`; anything else -> `Error`.
- It scans with its own engine and keeps its own `DeletePlan` in memory. `Clean` refers to a `scan_id` the worker
  issued; the parent cannot supply or modify plan entries.
- Options can only tighten: `temp_min_age_hours` is clamped to `>=` the definition value; `extra_excludes` only
  remove items. Delete mode is always permanent.
- One worker per session (one UAC prompt covers scan and clean in the GUI). It exits on `Shutdown`, pipe EOF,
  parent exit, or 10 minutes idle.

### 9.2 Running-application detection

- **Gate (before scan and again before clean)**: `CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS)` and
  `Process32FirstW/NextW`, matching `szExeFile` case-insensitively against `running_apps`. Only processes in the
  current session count (`ProcessIdToSessionId`). For the UI, `QueryFullProcessImageNameW` (with
  `PROCESS_QUERY_LIMITED_INFORMATION`) supplies the full path. This costs about 1 to 3 ms and needs no privileges.
- **Waiting**: with `app_wait = Wait`, the gate polls every 1 s and emits `WaitingForApps`; the user can cancel.
  Bezem never terminates or restarts other applications (no `RmShutdown`).
- **Lock attribution**: after a target finishes, up to 64 `Locked` paths are registered with the Restart Manager
  (`RmStartSession`, `RmRegisterResources`, `RmGetList`, `RmEndSession`) to tell the user *which* application holds
  them ("142 files skipped: in use by Teams"). This is informational only and skipped if it takes over 500 ms.

### 9.3 Service stop/start (`ServiceGuard`)

```
ServiceGuard::stop(name):                       // name from the AllowedService enum
  scm = OpenSCManagerW(null, null, SC_MANAGER_CONNECT)
  svc = OpenServiceW(scm, name, SERVICE_QUERY_STATUS | SERVICE_STOP | SERVICE_START | SERVICE_ENUMERATE_DEPENDENTS)
  s = QueryServiceStatusEx(svc, SC_STATUS_PROCESS_INFO)
  match s.dwCurrentState:
    STOPPED            -> return Guard { restore: false }
    STOP_PENDING       -> wait_for(STOPPED)
    START_PENDING      -> wait_for(RUNNING), then stop
    RUNNING            -> require SERVICE_ACCEPT_STOP;
                          EnumDependentServicesW(SERVICE_ACTIVE) must be empty (else fail: we do not stop chains)
                          ControlService(SERVICE_CONTROL_STOP); wait_for(STOPPED)
  wait_for: poll QueryServiceStatusEx, sleep clamp(dwWaitHint / 10, 250 ms, 2 s), total timeout 30 s;
            checkpoint not advancing for 3 polls past wait hint -> timeout
  timeout -> Err(ServiceTimeout): the cleaner's targets are NOT processed; guard still restores
  return Guard { restore: true }

Drop for Guard:
  if restore: StartServiceW(svc, 0, null); wait_for(RUNNING) up to 30 s; failure -> error-level log + report
```

- Start type is never changed. All allowed services are demand/trigger-start, so even if a restore failed, Windows
  starts them again on the next trigger.
- Guards are released in reverse acquisition order. Release builds keep `panic = "unwind"` (not `abort`) so `Drop`
  runs on a panic.
- Preconditions for the Windows Update cleaner: `no_pending_reboot` checks that neither
  `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Component Based Servicing\RebootPending` nor
  `...\WindowsUpdate\Auto Update\RebootRequired` exists; `servicing_idle` checks that `TrustedInstaller` is not
  running and the `Global\_MSIExecute` mutex is not held.

---

## 10. Concurrency and performance

### 10.1 Threading model

- One `rayon` thread pool owned by the `Engine`, `min(available_parallelism, 8)` threads (directory enumeration is
  I/O-bound; more threads add contention without gain).
- **Scan**: cleaners and their roots are `par_iter`ed. Inside a root, the walker is a work-stealing recursion
  (`rayon::scope`): each directory task enumerates one directory by handle and spawns tasks for subdirectories,
  passing down the parent handle (`Arc<OwnedHandle>`). Results are accumulated in per-task `Vec`s and merged per
  target, so there are no locks on the hot path. Counters for progress are `AtomicU64`, sampled by a 100 ms ticker.
- **Delete**: parallel across targets, capped at 4 concurrent deleters per volume, because NTFS metadata updates
  serialize on the journal and extra threads only add contention. On rotational disks (detected with
  `IOCTL_STORAGE_QUERY_PROPERTY` / `StorageDeviceSeekPenaltyProperty`) deletion is single-threaded per volume.
  Within a target, entries are processed in directory order to maximize directory-handle cache hits.
- Cleaners that share a service (`stop_service`) or a precondition are executed sequentially in one group; others
  run concurrently.
- COM work (Recycle Bin, volume cache handlers, WinRT clipboard) runs on a dedicated STA thread with its own message
  loop, never on rayon threads.
- The GUI runs the `Orchestrator` on one background thread; the UI thread only reads progress.
- No async runtime. Nothing in Bezem benefits from async I/O; threads plus channels are simpler and smaller.

### 10.2 Performance targets

Measured on the CI benchmark (synthetic tree generated on `windows-latest`) and on reference hardware (4-core,
NVMe SSD). CI fails on a regression of more than 20 % against the stored baseline.

| Metric | Target |
|---|---|
| Scan, typical profile (about 150k files, warm cache) | < 2 s (PLAN 6) |
| Scan, same, cold cache, SSD | < 6 s |
| Delete 100k small files, SSD | < 20 s |
| Cancellation latency | < 200 ms |
| Memory, typical scan | < 60 MB working set |
| Memory per planned entry | < 150 bytes average |
| `bezem list` start to exit | < 50 ms |
| GUI first frame | < 300 ms |
| Idle GUI CPU | 0 % (repaint only on input or progress) |
| Binary size (release, stripped, `opt-level = "s"`, LTO, `codegen-units = 1`) | CLI <= 3 MB, GUI <= 5 MB |

Why the custom walker instead of `jwalk` (see section 15): `FileIdExtdDirectoryInfo` returns names, attributes,
reparse tags, sizes, times *and* file IDs for ~500 entries per system call, and enumerating relative to an open
handle is required for safety anyway. Path-based walkers would need one extra open per file to obtain the ID.

---

## 11. Logging

### 11.1 Deletion log (user-facing, PLAN 3.3 item 8)

- Location: `%LOCALAPPDATA%\Bezem\logs\` (portable mode: `<exe dir>\data\logs\`).
- One file per run: `clean-2026-10-28T14-03-22Z-<8 hex run id>.jsonl`. Only `clean` creates a log; `scan` does not.
- Format: JSON Lines, UTF-8, one record per line, flushed every 256 records and at the end. Paths are written
  without the `\\?\` prefix.

```json
{"t":"2026-10-28T14:03:22.104Z","type":"run","run":"9f3c21aa","version":"1.0.0","elevated":false,"mode":"permanent","cleaners":["windows.user_temp","browser.chrome.cache"]}
{"t":"2026-10-28T14:03:22.311Z","type":"deleted","cleaner":"windows.user_temp","path":"C:\\Users\\Ann\\AppData\\Local\\Temp\\setup123.tmp","bytes":48213,"how":"permanent"}
{"t":"2026-10-28T14:03:22.312Z","type":"skipped","cleaner":"windows.user_temp","path":"C:\\Users\\Ann\\AppData\\Local\\Temp\\x.log","reason":"locked","code":"0xC0000043"}
{"t":"2026-10-28T14:03:25.020Z","type":"action","cleaner":"windows.update_download_cache","action":"stop_service","service":"wuauserv","result":"ok","source":"worker"}
{"t":"2026-10-28T14:03:31.877Z","type":"cleaner_done","cleaner":"windows.user_temp","files":1823,"dirs":41,"bytes":734003200,"skipped":{"locked":12,"too_new":96}}
{"t":"2026-10-28T14:03:31.900Z","type":"summary","files":5120,"bytes":1910034432,"skipped":131,"cancelled":false,"safety_violations":0,"duration_ms":9796}
```

- Records from the elevated worker travel over IPC (`ProgressEvent::LogRecord`) and are written by the parent into
  the same file, tagged `"source":"worker"`. The worker writes no log of its own.
- If the log cannot be created, `clean` refuses to run (transparency is a principle), unless `--no-log` is given
  explicitly.
- Retention: delete logs older than 90 days and keep total size under 100 MB (configurable), applied at the start of
  each `clean`. Bezem deletes only files matching its own `clean-*.jsonl` pattern in its own log directory.
- `bezem log show [run]` renders a log as a readable table; `bezem log path` prints the directory.

### 11.2 Diagnostic log (developer-facing)

- `tracing` + `tracing-subscriber`. Default: `warn` to stderr (CLI) / nothing (GUI).
- `--verbose` or `BEZEM_LOG=debug|trace` enables `diag.log` in the same directory, rotated at 5 MB, 3 files kept.
- Format: `2026-10-28T14:03:22.104Z DEBUG bezem_core::walk: enumerated dir="C:\..." entries=512 elapsed_us=830`.
- Every Win32/NT failure carries the API name and the code. No network sink exists.

---

## 12. Configuration

- Location: `%APPDATA%\Bezem\config.toml` (settings roam with the user profile; logs stay in `%LOCALAPPDATA%`).
- Portable mode: if a file named `bezem.portable` exists next to the executable, config, logs and community
  definitions live in `<exe dir>\data\` instead, and nothing is written to the profile.
- No registry use.
- Writes are atomic: write `config.toml.tmp` in the same directory, `FlushFileBuffers`, then
  `MoveFileExW(MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH)`.
- Loading is forgiving: unknown keys produce a warning (forward compatibility); an invalid value falls back to its
  default with a warning; a file that does not parse is renamed to `config.toml.bad-<timestamp>` and defaults are
  used. Nothing in the config can loosen the safety policy.

```toml
schema_version = 1

[general]
delete_mode         = "auto"     # auto | permanent | recycle  (auto: per data_class, section 8.5)
temp_min_age_hours  = 24         # can be raised, never below 24 for temp roots
wait_for_apps       = false      # true: wait for blocking apps instead of skipping the cleaner
language            = "auto"     # auto | en | nl
theme               = "system"   # system | light | dark

[selection]                       # differences from each cleaner's default_enabled
enabled  = ["privacy.clipboard"]
disabled = ["windows.thumbnail_cache"]

[exclusions]
paths = [
  '%LOCALAPPDATA%\Temp\my-build-cache\**',
]

[profiles.weekly]                 # saved selections (GUI "profiles", CLI --profile weekly)
cleaners = ["windows.user_temp", "browser.chrome.cache", "browser.edge.cache"]

[logs]
retention_days = 90
max_total_mb   = 100

[community]                       # post-1.0
enabled = false
approved = [
  { file = "community.foo-cache.toml", sha256 = "3b1f...e9" },
]
```

---

## 13. Error handling and exit codes

### 13.1 Error tiers

| Tier | Type | Effect | Example |
|---|---|---|---|
| Item | `SkipReason` | Counted, logged, run continues | Locked file, access denied, changed since scan |
| Target | `TargetError` | That root is abandoned, other targets continue | Root missing (non-optional), root failed verification |
| Safety violation | `SafetyViolation` | Target aborted, error-level log, red banner in UI, non-zero exit code | Final-path mismatch, root outside allowlist at run time |
| Cleaner | `CleanerError` | That cleaner is `Failed`, others continue | Service stop timeout, volume cache handler failure |
| Fatal | `EngineError` | Operation stops; services restored; log footer written | Log not writable, catalog invalid, IPC protocol error |

### 13.2 Conventions

- Libraries use `thiserror` enums; binaries use `anyhow` only at the top level for context chains.
- `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic` are denied in `bezem-core` and `bezem-cleaners`
  (allowed in tests). Panics are bugs, not error handling.
- Win32 failures are wrapped as `Win32Error { api: &'static str, code: u32 }` and NT failures as
  `NtError { api: &'static str, status: i32 }`, both displayed with the symbolic name where known.
- `DefinitionError` carries file, span and a stable code (`E0101 unknown token`), so docs can explain each code.
- User-facing messages are short and actionable ("Chrome is running. Close it or use --wait."); details go to the
  log.

### 13.3 CLI exit codes

The canonical list is [FEATURES.md, F-CLI-05](FEATURES.md#f-cli-05-exit-codes). `bezem-cli` maps errors to it: `SafetyViolation` → 9, definition errors → 7, config errors → 8, elevation declined → 5, unexpected errors → 1.

---

## 14. Dependencies

All dependencies must be MIT-compatible (MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause,
BSD-3-Clause, ISC, Zlib, Unicode-3.0; `deny.toml` is the authoritative list) and are checked by `cargo deny`
together with RustSec advisories and a ban list (any HTTP/TLS/socket crate).

| Crate | Used by | Why |
|---|---|---|
| `windows` | core | Official Microsoft bindings: Win32 (files, SCM, toolhelp, Restart Manager, pipes), WDK (`NtCreateFile`), COM (`IFileOperation`, `IEmptyVolumeCache2`) and WinRT (`Clipboard`). One crate instead of hand-written FFI. Only the needed features are enabled. |
| `serde`, `serde_json` | core, cli | Definition model, IPC frames, `--json` output, Chromium `Local State`, JSONL log. |
| `toml` | core, cleaners | Definition and config parsing, with span information for error messages. |
| `globset` | core | Fast multi-pattern globbing with case-insensitivity and `literal_separator`. |
| `rayon` | core | Work-stealing parallelism for the walker and deleter; small and well audited. |
| `thiserror` | core, cleaners | Typed library errors without boilerplate. |
| `anyhow` | cli, gui | Top-level error context only. |
| `tracing`, `tracing-subscriber` | all | Structured diagnostics with zero cost when disabled. |
| `time` | core | RFC 3339 timestamps for logs, `FILETIME` conversions. |
| `sha2` | cleaners | Pinning community definitions. |
| `clap` (derive) | cli | Argument parsing, help text, shell completions. |
| `eframe` / `egui` | gui | Pure-Rust immediate-mode GUI, small binary (open decision 1). |
| dev: `tempfile`, `proptest`, `libfuzzer-sys` (`cargo-fuzz`), `divan` | tests, fuzz, bench | Fake trees, property tests for path/glob logic, fuzzing of template and TOML parsing, CI benchmarks. |

Deliberately **not** used:

| Crate | Reason |
|---|---|
| `jwalk`, `walkdir` | Path-based; no file IDs; cannot do handle-relative opens (sections 8.4, 10.2). |
| `trash` | Path-based and recycles silently even when the shell would delete permanently; we need the `PreDeleteItem` check. |
| `sysinfo` | Large; we need only a process snapshot (Toolhelp). |
| `rusqlite` | No database editing in 1.0 (7.5). |
| `tokio`, any HTTP client | No async needed; no network by principle. |
| `rust-ini` | `profiles.ini` needs about 60 lines of parser; fuzzed in-house. |
| `dirs`, `known-folders` | We call `SHGetKnownFolderPath` directly with our own failure rules (7.1). |

---

## 15. Decisions and deviations from PLAN.md

| Topic | PLAN.md | This document | Reason |
|---|---|---|---|
| Directory walking | `rayon`, `jwalk` | `rayon` + custom handle-relative walker (`FileIdExtdDirectoryInfo`) | Needs file IDs for scan/delete identity checks and handle-relative enumeration for TOCTOU safety; also fewer syscalls. |
| Deletion log format | "plain text or JSON" | JSON Lines, with `bezem log show` for a readable view | Appendable, streamable, machine-readable, still greppable. |
| Token syntax | `%TEMP%` | Kept, but tokens are a closed set resolved through APIs, not environment lookups | Avoids empty or spoofed environment variables. |
| `StopService` / `StartService` | Named as special actions | Only `stop_service` is declarable; start is its automatic restore half | A definition cannot leave a service in a different state than it found it. |
| Firefox history | Listed under privacy | Not in 1.0 (cookies, cache, session, form history are) | `places.sqlite` also contains bookmarks; no SQLite editing in 1.0. |
| `unsafe` policy | `forbid` everywhere except `bezem-core`, which uses `deny` | Same, made concrete: `allow` on `mod win32` only, plus a CI grep | `forbid` cannot be locally overridden. |
| CLI commands | `list/scan/clean/show` | Adds `validate`, `log` and hidden `--bezem-elevated-worker` | Contributor workflow, log access, elevation. |

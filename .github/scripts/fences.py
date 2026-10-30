# SPDX-License-Identifier: MIT
"""Safety fences the Rust compiler cannot enforce (crates/bezem-core/src/win32/README.md).

Run from the repository root (or pass it as the only argument). Exit code 1 on any violation.

Matching is on words ([A-Za-z0-9_] boundaries, like `git grep -w`) over every tracked-looking
*.rs file; whole-line `//` comments (including `///` and `//!`) are ignored. Each fence switches
on by itself: while crates/bezem-core/src/win32/fs.rs is absent the promotion and primitive
fences pass with a notice. A promotion function whose definition still carries
`expect(dead_code)` has, by the compiler's guarantee, no non-test caller yet, so 0 call sites are
required; once that attribute is removed, exactly 1 call site is required in its home module.
"""

import os
import re
import sys

CORE = "crates/bezem-core/src"
FS = f"{CORE}/win32/fs.rs"
WIN32 = f"{CORE}/win32/"

# Promotion points: (function, exact definition line in win32/fs.rs, home module prefixes).
PROMOTIONS = [
    ("into_verified_dir", "    pub(crate) fn into_verified_dir(self) -> DirHandle {",
     (f"{CORE}/safety.rs", f"{CORE}/safety/")),
    ("attest", "    pub(crate) fn attest(handle: DeleteHandle) -> Self {",
     (f"{CORE}/delete.rs", f"{CORE}/delete/")),
]

PRIMITIVES = [
    "remove_file", "remove_dir", "remove_dir_all", "rename",
    "DeleteFileW", "DeleteFileA", "DeleteFile2W",
    "RemoveDirectoryW", "RemoveDirectoryA", "RemoveDirectory2W",
    "MoveFileW", "MoveFileExW", "MoveFileWithProgressW", "SHFileOperationW",
    "FILE_DELETE_ON_CLOSE", "FILE_FLAG_DELETE_ON_CLOSE",
    "FileDispositionInfo", "FileDispositionInfoEx",
    "FileDispositionInformation", "FileDispositionInformationEx",
    "FileRenameInfo", "FileRenameInfoEx", "FileRenameInformation",
]
# Narrow, exact exceptions: (file, stripped line). Test cleanup in the OwnedHandle unit test.
PRIMITIVE_EXCEPTIONS = {
    (f"{CORE}/win32/handle.rs", "std::fs::remove_file(&path).unwrap();"),
}

SKIP_DIRS = {".git", "target", "node_modules"}


def word(w):
    return re.compile(r"(?<![A-Za-z0-9_])" + re.escape(w) + r"(?![A-Za-z0-9_])")


RAW = {}


def load(root):
    """{relative path: [(line number, raw line)]} for every non-comment line of every .rs file."""
    files = {}
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames if d not in SKIP_DIRS)
        for name in sorted(filenames):
            if not name.endswith(".rs"):
                continue
            full = os.path.join(dirpath, name)
            rel = os.path.relpath(full, root).replace(os.sep, "/")
            with open(full, encoding="utf-8", errors="replace") as f:
                raw = f.read().splitlines()
            RAW[rel] = raw
            files[rel] = [(i, l) for i, l in enumerate(raw, 1) if not l.lstrip().startswith("//")]
    return files


class Report:
    def __init__(self):
        self.failed = False
        self.rows = []

    def error(self, msg, path=None, line=None):
        self.failed = True
        loc = f" file={path}" + (f",line={line}" if line else "") if path else ""
        print(f"::error{loc}::fence: {msg}")

    def row(self, check, status, detail):
        self.rows.append(f"| {check} | {status} | {detail} |")


def hits(files, pattern, only=None):
    rx = word(pattern) if isinstance(pattern, str) else pattern
    return [(p, n, l) for p, lines in files.items() if only is None or only(p)
            for n, l in lines if rx.search(l)]


def check_unsafe_code(files, rep):
    allowed_any = "#![forbid(unsafe_code)]"
    core_lib = f"{CORE}/lib.rs"
    bad, allows = [], []
    for p, n, l in hits(files, "unsafe_code"):
        s = l.strip()
        root_file = re.fullmatch(r"crates/[^/]+/src/(lib|main)\.rs", p)
        if root_file and s == allowed_any:
            continue
        if p == core_lib and s == "#![deny(unsafe_code)]":
            continue
        if p == core_lib and s == "#[allow(unsafe_code)]":
            allows.append((p, n))
            continue
        bad.append((p, n, s))
    for p, n, s in bad:
        rep.error(f"unsafe_code outside the allowed crate attributes: {s}", p, n)
    if len(allows) > 1:
        rep.error(f"#[allow(unsafe_code)] appears {len(allows)} times in {core_lib}; only once", core_lib)
    # The single allow must sit on the `mod win32;` declaration (other attributes may follow).
    for p, n in allows:
        rest = [l.strip() for i, l in files[p] if i > n]
        nxt = next((s for s in rest if not s.startswith("#[")), "")
        if nxt != "mod win32;":
            rep.error(f"#[allow(unsafe_code)] must apply to `mod win32;`, not `{nxt}`", p, n)
    ok = not bad and len(allows) <= 1
    rep.row("1. `unsafe_code` only in crate attributes", "pass" if ok else "**FAIL**",
            f"{len(allows)} `#[allow(unsafe_code)]` on `mod win32;`")


def in_home(path, home):
    return any(path == h if h.endswith(".rs") else path.startswith(h) for h in home)


def check_promotion(files, rep, idx, fn, definition, home):
    title = f"{idx}. `{fn}` only in `{home[1]}`"
    if FS not in files:
        print(f"::notice::fence: {FS} not present yet; `{fn}` fence inactive")
        rep.row(title, "inactive", f"`{FS}` not present yet")
        return
    defs = [n for n, l in files[FS] if l == definition]
    if len(defs) != 1:
        rep.error(f"expected exactly one definition line `{definition.strip()}` in {FS}, "
                  f"found {len(defs)}; if it was renamed, update this fence with the lead", FS)
        rep.row(title, "**FAIL**", "definition line missing or duplicated")
        return
    def_line = defs[0]
    # Attributes above the definition, interleaved with doc comments.
    raw = RAW[FS]
    attrs, n = [], def_line - 2
    while n >= 0 and raw[n].strip().startswith(("#[", "//")):
        attrs.append(raw[n])
        n -= 1
    pending = any("expect(dead_code" in a and not a.strip().startswith("//") for a in attrs)

    all_hits = [h for h in hits(files, fn) if not (h[0] == FS and h[1] == def_line)]
    outside = [h for h in all_hits if not in_home(h[0], home)]
    inside = [h for h in all_hits if in_home(h[0], home)]
    for p, n, l in outside:
        rep.error(f"`{fn}` used outside {home[1]}: {l.strip()}", p, n)
    want = 0 if pending else 1
    home_exists = any(in_home(p, home) for p in files)
    if len(inside) != want:
        why = ("its definition still has expect(dead_code), so no caller is expected yet"
               if pending else "expect(dead_code) is gone, so exactly one caller is required")
        for p, n, l in inside:
            print(f"  call site: {p}:{n}: {l.strip()}")
        rep.error(f"`{fn}` has {len(inside)} call site(s) in {home[1]}, expected {want} ({why})",
                  FS, def_line)
    ok = not outside and len(inside) == want
    state = "no caller yet (expect(dead_code))" if pending else "caller required"
    rep.row(title, "pass" if ok else "**FAIL**",
            f"{state}; home module {'present' if home_exists else 'absent'}; "
            f"{len(inside)} call site(s) inside, {len(outside)} outside")


def check_primitives(files, rep):
    title = "4. no deletion primitive outside `win32/fs.rs`"
    rx = re.compile(r"(?<![A-Za-z0-9_])(" + "|".join(map(re.escape, PRIMITIVES)) + r")(?![A-Za-z0-9_])")
    if FS not in files:
        print(f"::notice::fence: {FS} not present yet; checking primitives everywhere")
    bad = []
    for p, n, l in hits(files, rx):
        if p == FS or re.match(r"crates/[^/]+/tests/", p):
            continue
        if (p, l.strip()) in PRIMITIVE_EXCEPTIONS:
            continue
        bad.append((p, n, l.strip()))
    for p, n, s in bad:
        rep.error(f"deletion primitive outside {FS}: {s}", p, n)
    rep.row(title, "pass" if not bad else "**FAIL**", f"{len(bad)} violation(s)")


def check_win32_imports(files, rep):
    title = "5. `win32` never names `safety`"
    if not any(p.startswith(WIN32) for p in files):
        print(f"::notice::fence: {WIN32} not present yet; import fence inactive")
        rep.row(title, "inactive", f"`{WIN32}` not present yet")
        return
    bad = hits(files, "safety", only=lambda p: p.startswith(WIN32))
    for p, n, l in bad:
        rep.error(f"win32 must take path types from crate::path, not safety: {l.strip()}", p, n)
    rep.row(title, "pass" if not bad else "**FAIL**", f"{len(bad)} violation(s)")


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else "."
    files = load(root)
    if not files:
        print("::error::fence: no Rust files found; the fences would pass vacuously")
        return 1
    rep = Report()
    check_unsafe_code(files, rep)
    for i, (fn, definition, home) in enumerate(PROMOTIONS, 2):
        check_promotion(files, rep, i, fn, definition, home)
    check_primitives(files, rep)
    check_win32_imports(files, rep)
    table = "\n".join(["| Fence | Result | Detail |", "|---|---|---|", *rep.rows])
    print(table)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write("## Safety fences\n\n" + table + "\n")
    return 1 if rep.failed else 0


if __name__ == "__main__":
    sys.exit(main())

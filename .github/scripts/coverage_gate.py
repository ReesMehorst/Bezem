# SPDX-License-Identifier: MIT
"""Per-module line-coverage gate for CI (docs/QUALITY_AND_RELEASE.md section 1.1).

Reads the JSON written by `cargo llvm-cov --json --summary-only` (llvm-cov export format) and
checks line coverage per module. A module is a source path prefix such as
`crates/bezem-core/src/safety`; it matches `safety.rs` and everything under `safety/`. Files that
match no module are pooled and checked against --default.

A module with no files or no instrumented lines is reported as "not present yet" and passes, so
the gate activates on its own once the module lands. Writes a Markdown table to
$GITHUB_STEP_SUMMARY when that variable is set. Exit code 1 if any threshold is missed.

Usage: coverage_gate.py COV_JSON --module PREFIX=PCT [--module ...] --default PCT
"""

import argparse
import json
import os
import sys


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("cov_json")
    ap.add_argument("--module", action="append", default=[], metavar="PREFIX=PCT")
    ap.add_argument("--default", type=float, required=True, metavar="PCT")
    ap.add_argument("--root", default=os.getcwd(), help="workspace root (default: cwd)")
    args = ap.parse_args()

    modules = []
    for spec in args.module:
        prefix, _, pct = spec.rpartition("=")
        if not prefix:
            ap.error(f"--module needs PREFIX=PCT, got {spec!r}")
        modules.append((prefix.replace("\\", "/").rstrip("/"), float(pct)))

    with open(args.cov_json, encoding="utf-8") as f:
        data = json.load(f)

    # llvm-cov reports absolute paths; compare case-insensitively (Windows drive letters).
    root = args.root.replace("\\", "/").rstrip("/") + "/"
    # name -> [threshold, covered, count, files]
    buckets = {p: [t, 0, 0, 0] for p, t in modules}
    other = "(rest of workspace)"
    buckets[other] = [args.default, 0, 0, 0]

    for export in data["data"]:
        for entry in export["files"]:
            name = entry["filename"].replace("\\", "/")
            if name.lower().startswith(root.lower()):
                name = name[len(root):]
            lines = entry["summary"]["lines"]
            key = other
            for prefix, _ in modules:
                if name == prefix + ".rs" or name.startswith(prefix + "/"):
                    key = prefix
                    break
            b = buckets[key]
            b[1] += lines["covered"]
            b[2] += lines["count"]
            b[3] += 1

    failed = False
    rows = ["| Scope | Files | Lines covered | Coverage | Threshold | Result |", "|---|---|---|---|---|---|"]
    for name, (threshold, covered, count, files) in buckets.items():
        if count == 0:
            pct_s, result = "n/a", "not present yet (skipped)"
            print(f"::notice::coverage: {name}: no instrumented lines yet, gate inactive")
        else:
            pct = 100.0 * covered / count
            pct_s = f"{pct:.2f} %"
            if pct + 1e-9 >= threshold:
                result = "pass"
            else:
                result = "**FAIL**"
                failed = True
                print(f"::error::coverage: {name} at {pct:.2f} % is below {threshold:g} %")
        rows.append(f"| `{name}` | {files} | {covered}/{count} | {pct_s} | {threshold:g} % | {result} |")

    table = "\n".join(rows)
    print(table)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as f:
            f.write("## Line coverage\n\n" + table + "\n")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())

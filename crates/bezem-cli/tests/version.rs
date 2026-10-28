// SPDX-License-Identifier: MIT

use std::process::Command;

#[test]
fn version_flag_prints_package_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_bezem"))
        .arg("--version")
        .output()
        .expect("failed to run bezem");
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("stdout is UTF-8");
    assert_eq!(
        stdout.trim(),
        format!("bezem {}", env!("CARGO_PKG_VERSION"))
    );
}

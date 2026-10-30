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

#[test]
fn no_arguments_prints_usage_and_exits_2() {
    let out = Command::new(env!("CARGO_BIN_EXE_bezem"))
        .output()
        .expect("failed to run bezem");
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    let stderr = String::from_utf8(out.stderr).expect("stderr is UTF-8");
    assert!(
        stderr.contains("usage: bezem --version"),
        "stderr: {stderr}"
    );
}

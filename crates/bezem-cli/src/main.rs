// SPDX-License-Identifier: MIT

//! `bezem`: the command-line front-end. Only `--version` exists at M0.
#![forbid(unsafe_code)]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!("bezem {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!(
                "bezem {}: pre-alpha, no commands yet",
                env!("CARGO_PKG_VERSION")
            );
            eprintln!("usage: bezem --version");
            ExitCode::from(2)
        }
    }
}

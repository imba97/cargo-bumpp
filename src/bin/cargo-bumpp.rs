//! The `cargo-bumpp` binary, which is what makes `cargo bumpp` work.
//!
//! Cargo runs `cargo-bumpp bumpp <args…>`, so the leading `bumpp` is dropped
//! here. Everything else is the same tool.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("bumpp") {
        args.remove(0);
    }
    ExitCode::from(exit_code(cargo_bumpp::main(args)))
}

fn exit_code(code: i32) -> u8 {
    u8::try_from(code).unwrap_or(1)
}

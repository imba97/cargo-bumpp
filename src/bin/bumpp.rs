//! The `bumpp` binary: the tool under its short name.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    ExitCode::from(exit_code(cargo_bumpp::main(args)))
}

fn exit_code(code: i32) -> u8 {
    u8::try_from(code).unwrap_or(1)
}

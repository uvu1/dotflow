use std::process::ExitCode;

fn main() -> ExitCode {
    match dotflow::cli::run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("dotflow: {e}");
            ExitCode::FAILURE
        }
    }
}

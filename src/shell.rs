use crate::error::{Error, Result};

pub fn shell_init(shell: &str) -> Result<String> {
    let s = match shell {
        "bash" => {
            "dotflow() { if [ \"${1-}\" = cd ]; then shift; cd \"$(command dotflow root \"$@\")\"; else command dotflow \"$@\"; fi; }\n"
        }
        "zsh" => {
            "function dotflow { if [[ ${1-} == cd ]]; then shift; cd \"$(command dotflow root \"$@\")\"; else command dotflow \"$@\"; fi }\n"
        }
        "fish" => {
            "function dotflow; if test (count $argv) -gt 0; and test \"$argv[1]\" = cd; set -e argv[1]; cd (command dotflow root $argv); else; command dotflow $argv; end; end\n"
        }
        "powershell" => {
            "function dotflow { if ($args.Count -gt 0 -and $args[0] -eq 'cd') { Set-Location (& dotflow.exe root @($args | Select-Object -Skip 1)) } else { & dotflow.exe @args } }\n"
        }
        _ => return Err(Error(format!("unsupported shell: {shell}"))),
    };
    Ok(s.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_functions_handle_no_args() {
        for s in ["bash", "zsh", "fish", "powershell"] {
            assert!(shell_init(s).unwrap().contains("dotflow"));
        }
    }
    #[cfg(unix)]
    #[test]
    fn bash_wrapper_changes_parent_directory_and_handles_nounset() {
        use std::env;
        use std::fs;
        use std::process::Command;
        use tempfile::TempDir;

        if Command::new("bash").arg("--version").output().is_err() {
            return;
        }
        let t = TempDir::new().unwrap();
        let bin = t.path().join("dotflow");
        fs::write(
            &bin,
            "#!/bin/sh\nif [ \"$1\" = root ]; then printf '%s\\n' \"$DOTFLOW_TEST_ROOT\"; fi\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        let script = format!(
            "set -u; {} dotflow >/dev/null; dotflow cd; pwd",
            shell_init("bash").unwrap()
        );
        let o = Command::new("bash")
            .arg("-c")
            .arg(script)
            .env(
                "PATH",
                format!("{}:{}", t.path().display(), env::var("PATH").unwrap()),
            )
            .env("DOTFLOW_TEST_ROOT", t.path())
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(
            String::from_utf8_lossy(&o.stdout).trim(),
            t.path().to_string_lossy()
        );
    }
}

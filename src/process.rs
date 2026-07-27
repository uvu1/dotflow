use crate::error::{Error, Result};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IoMode {
    Capture,
    Inherit,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invocation {
    pub argv: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub io: IoMode,
}
pub struct Runner {
    pub dry_run: bool,
    pub intended: Vec<Invocation>,
}
impl Runner {
    pub fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            intended: vec![],
        }
    }
    fn prepare<I, S>(
        &mut self,
        program: &str,
        args: I,
        cwd: Option<&Path>,
        vars: &BTreeMap<String, String>,
        io: IoMode,
    ) -> (Vec<OsString>, Command)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let argv: Vec<_> = args
            .into_iter()
            .map(|x| x.as_ref().to_os_string())
            .collect();
        let mut shown = vec![OsString::from(program)];
        shown.extend(argv.clone());
        self.intended.push(Invocation {
            argv: shown,
            cwd: cwd.map(Path::to_path_buf),
            env: vars.clone(),
            io,
        });
        let mut cmd = Command::new(program);
        cmd.args(&argv).envs(vars);
        if let Some(c) = cwd {
            cmd.current_dir(c);
        }
        (argv, cmd)
    }
    pub fn capture<I, S>(
        &mut self,
        program: &str,
        args: I,
        cwd: Option<&Path>,
        vars: &BTreeMap<String, String>,
    ) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let (argv, mut cmd) = self.prepare(program, args, cwd, vars, IoMode::Capture);
        if self.dry_run {
            return Ok(success_output());
        }
        let out = cmd
            .output()
            .map_err(|e| Error(format!("cannot run {}: {e}", display_argv(program, &argv))))?;
        if out.status.success() {
            Ok(out)
        } else {
            Err(command_error(
                program,
                &argv,
                &out.stdout,
                &out.stderr,
                out.status,
            ))
        }
    }
    pub fn capture_status<I, S>(
        &mut self,
        program: &str,
        args: I,
        cwd: Option<&Path>,
        vars: &BTreeMap<String, String>,
    ) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let (argv, mut cmd) = self.prepare(program, args, cwd, vars, IoMode::Capture);
        if self.dry_run {
            return Ok(success_output());
        }
        cmd.output()
            .map_err(|e| Error(format!("cannot run {}: {e}", display_argv(program, &argv))))
    }
    pub fn inherit<I, S>(
        &mut self,
        program: &str,
        args: I,
        cwd: Option<&Path>,
        vars: &BTreeMap<String, String>,
    ) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let (argv, mut cmd) = self.prepare(program, args, cwd, vars, IoMode::Inherit);
        if self.dry_run {
            return Ok(());
        }
        let status = cmd
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| Error(format!("cannot run {}: {e}", display_argv(program, &argv))))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error(format!(
                "command failed with {status}: {}",
                display_argv(program, &argv)
            )))
        }
    }
}
fn command_error(
    program: &str,
    argv: &[OsString],
    stdout: &[u8],
    stderr: &[u8],
    status: ExitStatus,
) -> Error {
    let mut msg = format!(
        "command failed with {status}: {}",
        display_argv(program, argv)
    );
    if !stdout.is_empty() {
        msg.push_str("\nstdout:\n");
        msg.push_str(&String::from_utf8_lossy(stdout));
    }
    if !stderr.is_empty() {
        msg.push_str("\nstderr:\n");
        msg.push_str(&String::from_utf8_lossy(stderr));
    }
    Error(msg)
}
#[cfg(unix)]
fn success_output() -> Output {
    use std::os::unix::process::ExitStatusExt;
    Output {
        status: ExitStatus::from_raw(0),
        stdout: vec![],
        stderr: vec![],
    }
}
#[cfg(windows)]
fn success_output() -> Output {
    use std::os::windows::process::ExitStatusExt;
    Output {
        status: ExitStatus::from_raw(0),
        stdout: vec![],
        stderr: vec![],
    }
}
pub fn display_argv(p: &str, a: &[OsString]) -> String {
    std::iter::once(OsStr::new(p))
        .chain(a.iter().map(OsString::as_os_str))
        .map(|x| format!("{:?}", x.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn display_invocation(i: &Invocation) -> String {
    i.argv
        .iter()
        .map(|x| format!("{:?}", x.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_records_exact_modes_cwd_env_and_dry_run() {
        let mut r = Runner::new(true);
        let vars = BTreeMap::from([("K".into(), "V".into())]);
        r.capture("no-such", ["two words"], Some(Path::new("/tmp")), &vars)
            .unwrap();
        r.inherit("no-such", ["$(unsafe)"], None, &BTreeMap::new())
            .unwrap();
        assert_eq!(r.intended[0].io, IoMode::Capture);
        assert_eq!(r.intended[0].cwd, Some(PathBuf::from("/tmp")));
        assert_eq!(r.intended[0].env, vars);
        assert_eq!(r.intended[1].io, IoMode::Inherit);
    }
}

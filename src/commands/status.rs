use crate::config::Config;
use crate::error::{Error, Result};
use crate::git::git;
use crate::mise::{dotfiles_args, mise_status, selected_environments};
use crate::process::Runner;
use std::io::{self, Write};
use std::path::Path;

fn print_output(out: &std::process::Output) -> Result<()> {
    io::stdout()
        .write_all(&out.stdout)
        .map_err(|e| Error(e.to_string()))?;
    io::stderr()
        .write_all(&out.stderr)
        .map_err(|e| Error(e.to_string()))?;
    Ok(())
}
pub(crate) fn status(repo: &Path, cfg: &Config, profile: &str, check: bool) -> Result<u8> {
    let mut r = Runner::new(false);
    let branch = git(
        &mut r,
        repo,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )?;
    let dirty = git(
        &mut r,
        repo,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    println!(
        "repository: {}\nbranch: {}\nprofile: {}\ngit: {}",
        repo.display(),
        String::from_utf8_lossy(&branch.stdout).trim(),
        profile,
        if dirty.stdout.is_empty() {
            "clean"
        } else {
            "dirty"
        }
    );
    let mut drift = false;
    for e in selected_environments(cfg, profile)? {
        let a = dotfiles_args(&["dotfiles", "status", "--missing"], &e.targets);
        let refs: Vec<_> = a.iter().map(String::as_str).collect();
        let out = mise_status(&mut r, repo, cfg, e, &refs)?;
        print_output(&out)?;
        if !out.status.success() {
            drift = true;
        }
    }
    Ok(u8::from(check && (!dirty.stdout.is_empty() || drift)))
}

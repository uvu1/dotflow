use crate::config::Config;
use crate::error::{Error, Result};
use crate::process::Runner;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::process::Output;

pub fn normalize_remote(value: &str) -> Option<String> {
    let mut s = value.trim().trim_end_matches('/');
    if let Some(v) = s.strip_suffix(".git") {
        s = v;
    }
    let rest = s
        .strip_prefix("git@github.com:")
        .or_else(|| s.strip_prefix("ssh://git@github.com/"))
        .or_else(|| s.strip_prefix("https://github.com/"))?;
    let parts: Vec<_> = rest.split('/').collect();
    (parts.len() == 2 && parts.iter().all(|x| !x.is_empty()))
        .then(|| format!("github.com/{}/{}", parts[0], parts[1]))
}
pub fn remotes_equal(actual: &str, expected: &str) -> bool {
    match (normalize_remote(actual), normalize_remote(expected)) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(&b),
        (None, None) => canonical_remote(actual) == canonical_remote(expected),
        _ => false,
    }
}
fn canonical_remote(value: &str) -> String {
    value.trim().trim_end_matches('/').to_owned()
}

pub fn git(r: &mut Runner, repo: &Path, args: &[&str]) -> Result<Output> {
    let mut a = vec![OsStr::new("-C"), repo.as_os_str()];
    a.extend(args.iter().map(OsStr::new));
    r.capture("git", a, None, &BTreeMap::new())
}
pub fn validate_repo(r: &mut Runner, repo: &Path, cfg: &Config, require_clean: bool) -> Result<()> {
    let top = git(r, repo, &["rev-parse", "--show-toplevel"])?;
    if fs::canonicalize(String::from_utf8_lossy(&top.stdout).trim()).ok()
        != fs::canonicalize(repo).ok()
    {
        return Err(Error("repository path is not the Git worktree root".into()));
    }
    let branch = git(r, repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .map_err(|e| Error(format!("expected an attached branch: {e}")))?;
    let actual_branch = String::from_utf8_lossy(&branch.stdout);
    let actual_branch = actual_branch.trim();
    if actual_branch != cfg.repository.branch {
        return Err(Error(format!(
            "expected branch {}, found {actual_branch}",
            cfg.repository.branch
        )));
    }
    let origin = git(r, repo, &["remote", "get-url", "origin"])?;
    let actual_origin = String::from_utf8_lossy(&origin.stdout);
    if !remotes_equal(actual_origin.trim(), &cfg.repository.origin) {
        return Err(Error(format!(
            "origin does not match configured repository: expected {:?}, found {:?}",
            cfg.repository.origin,
            actual_origin.trim()
        )));
    }
    let status = git(
        r,
        repo,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if require_clean && !status.stdout.is_empty() {
        return Err(Error(format!(
            "worktree is not clean:\n{}",
            String::from_utf8_lossy(&status.stdout)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{git_cmd, repository};
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn remote_comparison_is_safe() {
        assert!(remotes_equal(
            "git@github.com:A/B.git",
            "https://github.com/a/b"
        ));
        assert!(remotes_equal("file:///one", "file:///one"));
        assert!(!remotes_equal(
            "https://one.invalid/a",
            "https://two.invalid/b"
        ));
        assert!(!remotes_equal(
            "https://github.com/a/b",
            "https://example.com/a/b"
        ));
    }
    #[test]
    fn validation_clean_dirty_untracked_branch_detached_origin() {
        let (t, c) = repository();
        validate_repo(&mut Runner::new(false), t.path(), &c, true).unwrap();
        fs::write(t.path().join("tracked"), "two").unwrap();
        assert!(validate_repo(&mut Runner::new(false), t.path(), &c, true).is_err());
        git_cmd(t.path(), &["restore", "tracked"]);
        fs::write(t.path().join("new"), "x").unwrap();
        assert!(validate_repo(&mut Runner::new(false), t.path(), &c, true).is_err());
        fs::remove_file(t.path().join("new")).unwrap();
        git_cmd(t.path(), &["switch", "-c", "other"]);
        assert!(validate_repo(&mut Runner::new(false), t.path(), &c, false).is_err());
        git_cmd(t.path(), &["switch", "master"]);
        git_cmd(t.path(), &["checkout", "--detach"]);
        assert!(validate_repo(&mut Runner::new(false), t.path(), &c, false).is_err());
    }
    #[test]
    fn real_git_ff_only_accepts_fast_forward_and_rejects_divergence() {
        let t = TempDir::new().unwrap();
        let remote = t.path().join("remote.git");
        let seed = t.path().join("seed");
        let local = t.path().join("local");
        fs::create_dir(&remote).unwrap();
        fs::create_dir(&seed).unwrap();
        git_cmd(&remote, &["init", "--bare"]);
        git_cmd(&seed, &["init", "-b", "master"]);
        for dir in [&seed, &local] {
            if dir.exists() {
                git_cmd(dir, &["config", "user.name", "Test"]);
                git_cmd(dir, &["config", "user.email", "test@example.invalid"]);
                git_cmd(dir, &["config", "commit.gpgsign", "false"]);
            }
        }
        fs::write(seed.join("file"), "one").unwrap();
        git_cmd(&seed, &["add", "file"]);
        git_cmd(&seed, &["commit", "-m", "one"]);
        git_cmd(
            &seed,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git_cmd(&seed, &["push", "-u", "origin", "master"]);
        let clone = Command::new("git")
            .args(["clone", remote.to_str().unwrap(), local.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            clone.status.success(),
            "{}",
            String::from_utf8_lossy(&clone.stderr)
        );
        git_cmd(&local, &["config", "user.name", "Test"]);
        git_cmd(&local, &["config", "user.email", "test@example.invalid"]);
        git_cmd(&local, &["config", "commit.gpgsign", "false"]);
        fs::write(seed.join("file"), "two").unwrap();
        git_cmd(&seed, &["commit", "-am", "two"]);
        git_cmd(&seed, &["push"]);
        git_cmd(&local, &["pull", "--ff-only", "origin", "master"]);
        fs::write(local.join("local"), "local").unwrap();
        git_cmd(&local, &["add", "local"]);
        git_cmd(&local, &["commit", "-m", "local"]);
        fs::write(seed.join("remote"), "remote").unwrap();
        git_cmd(&seed, &["add", "remote"]);
        git_cmd(&seed, &["commit", "-m", "remote"]);
        git_cmd(&seed, &["push"]);
        let diverged = Command::new("git")
            .arg("-C")
            .arg(&local)
            .args(["pull", "--ff-only", "origin", "master"])
            .output()
            .unwrap();
        assert!(!diverged.status.success());
    }
}

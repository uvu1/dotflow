#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

struct Fixture {
    _temp: TempDir,
    repo: PathBuf,
    bin: PathBuf,
    log: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let repo = temp.path().join("repo");
        let bin = temp.path().join("bin");
        let log = temp.path().join("log");
        fs::create_dir_all(repo.join("backend")).unwrap();
        fs::create_dir(&bin).unwrap();
        git(&repo, &["init", "-b", "master"]);
        git(&repo, &["config", "user.name", "Test"]);
        git(&repo, &["config", "user.email", "test@example.invalid"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        let config = format!(
            "schema_version=1\n[repository]\nbranch='master'\norigin={:?}\nmise_dir='backend'\n[common]\ntargets=['base']\n[profiles.host]\nplatform={:?}\nadd_environment='editing'\n[[profiles.host.environments]]\nmise_environment='one'\ntargets=['first']\n[[profiles.host.environments]]\nmise_environment='two'\ntargets=['second']\n[[profiles.host.after_sync]]\nprogram='hook'\nargs=['after','{{backend}}']\n[[profiles.host.before_install]]\nprogram='hook'\nargs=['before']\n",
            repo.to_string_lossy(),
            dotflow::detected_platform().to_string()
        );
        fs::write(repo.join(".dotflow.toml"), config).unwrap();
        fs::write(repo.join("backend/mise.toml"), "").unwrap();
        fs::write(repo.join("backend/mise.one.toml"), "").unwrap();
        fs::write(repo.join("backend/mise.two.toml"), "").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "initial"]);
        git(&repo, &["remote", "add", "origin", repo.to_str().unwrap()]);
        let script = "#!/bin/sh\nprintf '%s|%s|%s\\n' \"$(basename \"$0\")\" \"$PWD\" \"$*\" >>\"$DOTFLOW_LOG\"\nif [ \"${DOTFLOW_FAIL_MATCH-}\" != '' ] && printf '%s' \"$*\" | grep -F -- \"$DOTFLOW_FAIL_MATCH\" >/dev/null; then printf 'drift output\\n'; printf 'drift error\\n' >&2; exit 1; fi\n";
        for name in ["mise", "hook"] {
            let path = bin.join(name);
            fs::write(&path, script).unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self {
            _temp: temp,
            repo,
            bin,
            log,
        }
    }

    fn run(&self, args: &[&str], fail: Option<&str>) -> Output {
        let path = format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap());
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_dotflow"));
        cmd.arg("--repo")
            .arg(&self.repo)
            .args(args)
            .env("PATH", path)
            .env("DOTFLOW_LOG", &self.log);
        if let Some(pattern) = fail {
            cmd.env("DOTFLOW_FAIL_MATCH", pattern);
        }
        cmd.output().unwrap()
    }

    fn lines(&self) -> Vec<String> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

#[test]
fn add_edit_apply_and_status_route_exact_environments() {
    let f = Fixture::new();
    assert!(f.run(&["add", "base-target"], None).status.success());
    assert!(
        f.run(&["add", "--profile", "host", "profile-target"], None)
            .status
            .success()
    );
    assert!(f.run(&["edit", "editable"], None).status.success());
    assert!(f.run(&["apply"], None).status.success());
    let lines = f.lines();
    assert!(lines[0].ends_with("backend dotfiles add base-target"));
    assert!(lines[1].ends_with("backend -E editing dotfiles add profile-target"));
    assert!(lines[2].ends_with("backend -E editing dotfiles edit editable"));
    assert!(
        lines
            .iter()
            .any(|x| x.ends_with("backend -E one dotfiles apply --yes first"))
    );
    assert!(
        lines
            .iter()
            .any(|x| x.ends_with("backend -E two dotfiles apply --yes second"))
    );
    assert!(
        lines
            .iter()
            .any(|x| x.ends_with("backend -E one dotfiles status --missing first"))
    );
    assert!(
        lines
            .iter()
            .any(|x| x.ends_with("backend -E two dotfiles status second"))
    );
    let normal = f.run(&["status"], Some("--missing"));
    assert!(normal.status.success());
    assert!(String::from_utf8_lossy(&normal.stdout).contains("drift output"));
    let check = f.run(&["status", "--check"], Some("--missing"));
    assert_eq!(check.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&check.stderr).contains("drift error"));
    assert!(
        !f.lines()
            .iter()
            .any(|x| x.ends_with("dotfiles status --missing") || x.ends_with("dotfiles status"))
    );
}

#[test]
fn update_dry_run_executes_only_read_only_preflight_and_applies_before_install() {
    let f = Fixture::new();
    let out = f.run(&["update", "--dry-run"], None);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lines = f.lines();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].ends_with("|--version"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    for needle in [
        "\"git\" \"-C\"",
        "\"hook\" \"after\"",
        "\"mise\" \"trust\"",
        "\"hook\" \"before\"",
        "\"mise\" \"-C\"",
        "\"dotfiles\" \"status\" \"--missing\" \"first\"",
        "\"dotfiles\" \"status\" \"second\"",
    ] {
        assert!(stdout.contains(needle), "missing {needle}: {stdout}");
    }
    let at = |needle: &str| stdout.find(needle).unwrap();
    assert!(at("\"pull\"") < at("\"after\""));
    assert!(at("\"after\"") < at("\"trust\""));
    assert!(at("\"trust\"") < at("\"apply\""));
    assert!(at("\"apply\"") < at("\"before\""));
    assert!(at("\"before\"") < at("\"install\""));
}

#[test]
fn update_is_fail_fast_and_names_the_stage() {
    let f = Fixture::new();
    let out = f.run(&["update"], Some("after"));
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("stage after_sync hooks failed"));
    let lines = f.lines();
    assert!(lines.iter().any(|x| x.contains("|after ")));
    assert!(
        !lines
            .iter()
            .any(|x| x.contains("|before") || x.contains(" install") || x.contains(" dotfiles "))
    );
}

use crate::config::Config;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

pub(crate) fn config(profiles: &str) -> Config {
    toml::from_str(&format!("schema_version=1\n[repository]\nbranch='master'\norigin='git@github.com:a/b.git'\n{profiles}")).unwrap()
}
pub(crate) fn git_cmd(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
pub(crate) fn repository() -> (TempDir, Config) {
    let t = TempDir::new().unwrap();
    git_cmd(t.path(), &["init", "-b", "master"]);
    git_cmd(t.path(), &["config", "user.name", "Test"]);
    git_cmd(t.path(), &["config", "user.email", "test@example.invalid"]);
    fs::write(t.path().join("tracked"), "one").unwrap();
    git_cmd(t.path(), &["add", "tracked"]);
    git_cmd(t.path(), &["commit", "-m", "initial"]);
    git_cmd(
        t.path(),
        &["remote", "add", "origin", "https://github.com/a/b"],
    );
    (t, config("[profiles.host]\nplatform='linux'"))
}

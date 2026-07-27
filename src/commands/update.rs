use crate::commands::apply::apply;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::git::validate_repo;
use crate::hooks::run_hooks;
use crate::mise::{mise_inherit, selected_environments, trust};
use crate::platform::select_profile;
use crate::process::{Runner, display_invocation};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;

fn stage<T>(name: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    println!("==> {name}");
    f().map_err(|e| Error(format!("stage {name} failed: {e}")))
}
pub(crate) fn update(
    repo: &Path,
    cfg: &Config,
    profile: &str,
    profile_explicit: bool,
    dry: bool,
) -> Result<u8> {
    let mut preflight = Runner::new(false);
    stage("preflight", || {
        validate_repo(&mut preflight, repo, cfg, true)?;
        preflight.capture("git", [OsStr::new("--version")], None, &BTreeMap::new())?;
        preflight.capture("mise", [OsStr::new("--version")], None, &BTreeMap::new())?;
        Ok(())
    })?;
    let mut r = Runner::new(dry);
    let branch = cfg.repository.branch.clone();
    stage("pull", || {
        let mut a = vec![OsString::from("-C"), repo.as_os_str().to_os_string()];
        a.extend(
            ["pull", "--ff-only", "origin", &branch]
                .into_iter()
                .map(OsString::from),
        );
        r.inherit("git", a, None, &BTreeMap::new())
    })?;
    let refreshed = if dry {
        cfg.clone()
    } else {
        stage("reload config", || Config::load(repo))?
    };
    let selected = stage("select refreshed profile", || {
        select_profile(&refreshed, profile_explicit.then_some(profile))
    })?;
    let p = refreshed
        .profiles
        .get(&selected)
        .ok_or_else(|| Error(format!("unknown profile: {selected}")))?;
    let backend = repo.join(&refreshed.repository.mise_dir);
    stage("after_sync hooks", || {
        run_hooks(&mut r, &p.after_sync, repo, &backend)
    })?;
    stage("trust", || trust(&mut r, repo, &refreshed, &selected))?;
    stage("before_install hooks", || {
        run_hooks(&mut r, &p.before_install, repo, &backend)
    })?;
    stage("install", || {
        for e in selected_environments(&refreshed, &selected)? {
            mise_inherit(&mut r, repo, &refreshed, e, &["install"])?;
        }
        Ok(())
    })?;
    stage("apply", || apply(&mut r, repo, &refreshed, &selected, &[]))?;
    if dry {
        for i in &r.intended {
            println!("{}", display_invocation(i));
        }
    }
    Ok(0)
}

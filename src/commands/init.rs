use crate::config::Config;
use crate::error::{Error, Result};
use crate::git::{git, remotes_equal};
use crate::locator::{locator_path, resolve_repo_from};
use crate::platform::{Platform, detected_platform};
use crate::process::Runner;
use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn init(path: Option<PathBuf>) -> Result<u8> {
    let repo = path.unwrap_or(std::env::current_dir().map_err(|e| Error(e.to_string()))?);
    let repo = fs::canonicalize(&repo)
        .map_err(|e| Error(format!("cannot access {}: {e}", repo.display())))?;
    let mut r = Runner::new(false);
    let top = git(&mut r, &repo, &["rev-parse", "--show-toplevel"])?;
    if fs::canonicalize(String::from_utf8_lossy(&top.stdout).trim()).ok() != Some(repo.clone()) {
        return Err(Error(
            "init path must be the root of an existing Git clone".into(),
        ));
    }
    let branch = git(
        &mut r,
        &repo,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )?;
    let origin = git(&mut r, &repo, &["remote", "get-url", "origin"])?;
    let cfg_path = repo.join(".dotflow.toml");
    let loc = locator_path()?;
    if loc.exists() {
        let existing = resolve_repo_from(None, None, Some(loc.clone()), repo.clone())?;
        if existing != repo {
            return Err(Error(format!(
                "{} already registers {}; refusing to replace it",
                loc.display(),
                existing.display()
            )));
        }
        if !cfg_path.exists() {
            return Err(Error(format!(
                "{} already registers this repository but {} is missing; refusing a partial registration",
                loc.display(),
                cfg_path.display()
            )));
        }
    }
    if cfg_path.exists() {
        let existing = Config::load(&repo)?;
        let actual_branch = String::from_utf8_lossy(&branch.stdout);
        let actual_origin = String::from_utf8_lossy(&origin.stdout);
        if existing.repository.branch != actual_branch.trim()
            || !remotes_equal(&existing.repository.origin, actual_origin.trim())
        {
            return Err(Error(format!(
                "{} conflicts with the current branch or origin; refusing to overwrite it",
                cfg_path.display()
            )));
        }
        if !loc.exists() {
            write_locator(&loc, &repo)?;
        }
        println!("registered {}", repo.display());
        return Ok(0);
    }
    let mise_dir = if repo.join("mise/mise.toml").exists() {
        PathBuf::from("mise")
    } else if repo.join("mise.toml").exists() {
        PathBuf::from(".")
    } else {
        return Err(Error(
            "cannot initialize: no mise.toml found at repository root or mise/mise.toml".into(),
        ));
    };
    let detected = detected_platform();
    let env_names = available_environments(&repo.join(&mise_dir))?;
    let chosen = choose_init_environments(detected, &env_names);
    let profile = detected.to_string();
    let mut text = format!(
        "schema_version = 1\nminimum_dotflow_version = \"0.1.0\"\n\n[repository]\nbranch = {:?}\norigin = {:?}\nmise_dir = {:?}\n\n[profiles.{profile}]\nplatform = {:?}\n",
        String::from_utf8_lossy(&branch.stdout).trim(),
        String::from_utf8_lossy(&origin.stdout).trim(),
        mise_dir.to_string_lossy(),
        detected.to_string()
    );
    if let Some(first) = chosen.first() {
        text.push_str(&format!("add_environment = {first:?}\n"));
    }
    for name in chosen {
        text.push_str(&format!(
            "\n[[profiles.{profile}.environments]]\nmise_environment = {name:?}\n"
        ));
    }
    fs::write(&cfg_path, text)
        .map_err(|e| Error(format!("cannot write {}: {e}", cfg_path.display())))?;
    if let Err(e) = write_locator(&loc, &repo) {
        let _ = fs::remove_file(&cfg_path);
        return Err(e);
    }
    println!("registered {}", repo.display());
    Ok(0)
}
fn available_environments(dir: &Path) -> Result<Vec<String>> {
    let mut names = vec![];
    for entry in
        fs::read_dir(dir).map_err(|e| Error(format!("cannot inspect {}: {e}", dir.display())))?
    {
        let entry = entry.map_err(|e| Error(e.to_string()))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(v) = name
            .strip_prefix("mise.")
            .and_then(|v| v.strip_suffix(".toml"))
        {
            names.push(v.to_owned());
        }
    }
    names.sort();
    Ok(names)
}
fn choose_init_environments(platform: Platform, names: &[String]) -> Vec<String> {
    let preferred: &[&str] = match platform {
        Platform::Wsl => &["wsl", "linux"],
        Platform::Linux => &["linux"],
        Platform::Macos => &["macos", "darwin"],
        Platform::Windows => &["windows", "powershell", "pwsh"],
    };
    let selected: Vec<_> = preferred
        .iter()
        .filter(|p| names.iter().any(|n| n.eq_ignore_ascii_case(p)))
        .map(|p| (*p).to_owned())
        .collect();
    if selected.is_empty() && names.len() == 1 {
        names.to_vec()
    } else {
        selected
    }
}
fn write_locator(loc: &Path, repo: &Path) -> Result<()> {
    let parent = loc
        .parent()
        .ok_or_else(|| Error(format!("invalid locator path: {}", loc.display())))?;
    fs::create_dir_all(parent)
        .map_err(|e| Error(format!("cannot create {}: {e}", parent.display())))?;
    fs::write(loc, format!("repo = {:?}\n", repo.to_string_lossy()))
        .map_err(|e| Error(format!("cannot write {}: {e}", loc.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_environment_detection() {
        assert_eq!(
            choose_init_environments(
                Platform::Windows,
                &["linux".into(), "pwsh".into(), "windows".into()]
            ),
            ["windows", "pwsh"]
        );
        assert_eq!(
            choose_init_environments(Platform::Macos, &["darwin".into()]),
            ["darwin"]
        );
        assert!(choose_init_environments(Platform::Linux, &[]).is_empty());
    }
}

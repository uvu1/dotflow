use crate::error::{Error, Result};
use serde::Deserialize;
use std::env;
use std::fs;
use std::path::PathBuf;

pub fn locator_path() -> Result<PathBuf> {
    #[cfg(windows)]
    if let Some(v) = env::var_os("APPDATA") {
        return Ok(PathBuf::from(v).join("dotflow/config.toml"));
    }
    if let Some(v) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(v).join("dotflow/config.toml"));
    }
    home_dir().map(|p| p.join(".config/dotflow/config.toml"))
}
pub fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().ok_or_else(|| Error("home directory is unavailable".into()))
}
#[derive(Deserialize)]
struct Locator {
    repo: PathBuf,
}
pub fn resolve_repo_from(
    cli: Option<PathBuf>,
    env_repo: Option<PathBuf>,
    locator: Option<PathBuf>,
    fallback: PathBuf,
) -> Result<PathBuf> {
    resolve_repo_with(cli, env_repo, || Ok(locator), || Ok(fallback))
}
fn resolve_repo_with<L, F>(
    cli: Option<PathBuf>,
    env_repo: Option<PathBuf>,
    locator: L,
    fallback: F,
) -> Result<PathBuf>
where
    L: FnOnce() -> Result<Option<PathBuf>>,
    F: FnOnce() -> Result<PathBuf>,
{
    let path = if let Some(p) = cli {
        p
    } else if let Some(p) = env_repo {
        p
    } else if let Some(loc) = locator()?.filter(|p| p.exists()) {
        let s = fs::read_to_string(&loc)
            .map_err(|e| Error(format!("cannot read {}: {e}", loc.display())))?;
        toml::from_str::<Locator>(&s)
            .map_err(|e| Error(format!("invalid {}: {e}", loc.display())))?
            .repo
    } else {
        fallback()?
    };
    Ok(fs::canonicalize(&path).unwrap_or(path))
}
pub fn resolve_repo(cli: Option<PathBuf>) -> Result<PathBuf> {
    resolve_repo_with(
        cli,
        env::var_os("DOTFLOW_REPO").map(PathBuf::from),
        || locator_path().map(Some),
        || home_dir().map(|path| path.join("dotfiles")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn repository_precedence_is_explicit_and_race_free() {
        let t = TempDir::new().unwrap();
        let loc = t.path().join("locator");
        fs::write(
            &loc,
            format!("repo={:?}", t.path().join("located").to_string_lossy()),
        )
        .unwrap();
        assert_eq!(
            resolve_repo_from(
                Some(t.path().join("cli")),
                Some(t.path().join("env")),
                Some(loc.clone()),
                t.path().join("fallback")
            )
            .unwrap(),
            t.path().join("cli")
        );
        assert_eq!(
            resolve_repo_from(
                None,
                Some(t.path().join("env")),
                Some(loc.clone()),
                t.path().join("fallback")
            )
            .unwrap(),
            t.path().join("env")
        );
        assert_eq!(
            resolve_repo_from(None, None, Some(loc), t.path().join("fallback")).unwrap(),
            t.path().join("located")
        );
    }
    #[test]
    fn explicit_repository_does_not_discover_lower_precedence_paths() {
        let path = PathBuf::from("explicit");
        assert_eq!(
            resolve_repo_with(
                Some(path.clone()),
                None,
                || panic!("locator must not be evaluated"),
                || panic!("home fallback must not be evaluated"),
            )
            .unwrap(),
            path
        );
    }
    #[test]
    fn environment_repository_does_not_discover_lower_precedence_paths() {
        let path = PathBuf::from("environment");
        assert_eq!(
            resolve_repo_with(
                None,
                Some(path.clone()),
                || panic!("locator must not be evaluated"),
                || panic!("home fallback must not be evaluated"),
            )
            .unwrap(),
            path
        );
    }
    #[test]
    fn locator_repository_does_not_require_home() {
        let t = TempDir::new().unwrap();
        let located = t.path().join("located");
        let locator = t.path().join("config.toml");
        fs::write(&locator, format!("repo={:?}", located.to_string_lossy())).unwrap();
        assert_eq!(
            resolve_repo_with(
                None,
                None,
                || Ok(Some(locator)),
                || Err(Error("home directory is unavailable".into())),
            )
            .unwrap(),
            located
        );
    }
    #[test]
    fn home_is_required_only_for_the_final_fallback() {
        let error = resolve_repo_with(
            None,
            None,
            || Ok(None),
            || Err(Error("home directory is unavailable".into())),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "home directory is unavailable");
    }
}

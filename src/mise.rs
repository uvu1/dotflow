use crate::config::{Config, Environment, Profile};
use crate::error::{Error, Result};
use crate::process::Runner;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::process::Output;

pub fn selected_environments<'a>(cfg: &'a Config, profile: &str) -> Result<Vec<&'a Environment>> {
    let p = cfg
        .profiles
        .get(profile)
        .ok_or_else(|| Error(format!("unknown profile: {profile}")))?;
    if p.environments.is_empty() {
        Ok(vec![&cfg.common])
    } else {
        Ok(p.environments.iter().collect())
    }
}
pub fn add_environment(profile: &Profile) -> Environment {
    Environment {
        mise_environment: profile.add_environment.clone(),
        targets: vec![],
    }
}
fn mise_args(repo: &Path, cfg: &Config, env: &Environment, args: &[&str]) -> Vec<OsString> {
    let mut a = vec![
        OsString::from("-C"),
        repo.join(&cfg.repository.mise_dir).into_os_string(),
    ];
    if let Some(e) = &env.mise_environment {
        a.push("-E".into());
        a.push(e.into());
    }
    a.extend(args.iter().map(OsString::from));
    a
}
pub fn mise_inherit(
    r: &mut Runner,
    repo: &Path,
    cfg: &Config,
    env: &Environment,
    args: &[&str],
) -> Result<()> {
    r.inherit(
        "mise",
        mise_args(repo, cfg, env, args),
        None,
        &BTreeMap::new(),
    )
}
pub fn mise_status(
    r: &mut Runner,
    repo: &Path,
    cfg: &Config,
    env: &Environment,
    args: &[&str],
) -> Result<Output> {
    r.capture_status(
        "mise",
        mise_args(repo, cfg, env, args),
        None,
        &BTreeMap::new(),
    )
}
pub fn trust(r: &mut Runner, repo: &Path, cfg: &Config, profile: &str) -> Result<()> {
    let dir = repo.join(&cfg.repository.mise_dir);
    let mut paths = vec![dir.join("mise.toml")];
    for e in selected_environments(cfg, profile)? {
        if let Some(n) = &e.mise_environment {
            paths.push(dir.join(format!("mise.{n}.toml")));
        }
    }
    let mut seen = BTreeSet::new();
    for path in paths {
        if seen.insert(path.clone()) {
            r.inherit(
                "mise",
                [OsStr::new("trust"), path.as_os_str()],
                None,
                &BTreeMap::new(),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::config;

    #[test]
    fn environment_fallback_and_multiple() {
        let c = config(
            "[common]\ntargets=['base']\n[profiles.a]\nplatform='windows'\n[[profiles.a.environments]]\nmise_environment='windows'\n[[profiles.a.environments]]\nmise_environment='pwsh'",
        );
        assert_eq!(selected_environments(&c, "a").unwrap().len(), 2);
        let c = config("[common]\ntargets=['base']\n[profiles.a]\nplatform='linux'");
        assert_eq!(selected_environments(&c, "a").unwrap().len(), 1);
        assert_eq!(selected_environments(&c, "a").unwrap()[0].targets, ["base"]);
    }
    #[test]
    fn trust_deduplicates_environment_files() {
        let c = config(
            "[profiles.a]\nplatform='linux'\n[[profiles.a.environments]]\nmise_environment='linux'\n[[profiles.a.environments]]\nmise_environment='linux'",
        );
        let mut r = Runner::new(true);
        trust(&mut r, Path::new("/repo"), &c, "a").unwrap();
        assert_eq!(r.intended.len(), 2);
    }
}

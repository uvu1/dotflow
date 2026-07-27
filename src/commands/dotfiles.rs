use crate::config::Config;
use crate::error::{Error, Result};
use crate::mise::{add_environment, mise_inherit};
use crate::process::Runner;
use std::path::Path;

pub(crate) fn add(
    repo: &Path,
    cfg: &Config,
    profile: &str,
    profile_explicit: bool,
    target: &[String],
) -> Result<u8> {
    let env = if profile_explicit {
        add_environment(
            cfg.profiles
                .get(profile)
                .ok_or_else(|| Error(format!("unknown profile: {profile}")))?,
        )
    } else {
        cfg.common.clone()
    };
    let mut a = vec!["dotfiles".to_owned(), "add".into()];
    a.extend(target.to_owned());
    let refs: Vec<_> = a.iter().map(String::as_str).collect();
    mise_inherit(&mut Runner::new(false), repo, cfg, &env, &refs)?;
    Ok(0)
}
pub(crate) fn edit(repo: &Path, cfg: &Config, profile: &str, target: &str) -> Result<u8> {
    let env = add_environment(
        cfg.profiles
            .get(profile)
            .ok_or_else(|| Error(format!("unknown profile: {profile}")))?,
    );
    mise_inherit(
        &mut Runner::new(false),
        repo,
        cfg,
        &env,
        &["dotfiles", "edit", target],
    )?;
    Ok(0)
}

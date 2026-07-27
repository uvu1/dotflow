use crate::config::Config;
use crate::error::Result;
use crate::mise::{mise_inherit, selected_environments};
use crate::process::Runner;
use std::path::Path;

pub(crate) fn apply(
    r: &mut Runner,
    repo: &Path,
    cfg: &Config,
    profile: &str,
    targets: &[String],
) -> Result<()> {
    for e in selected_environments(cfg, profile)? {
        let selected = if targets.is_empty() {
            &e.targets
        } else {
            targets
        };
        let mut a = vec!["dotfiles".to_owned(), "apply".into(), "--yes".into()];
        a.extend(selected.iter().cloned());
        let refs: Vec<_> = a.iter().map(String::as_str).collect();
        mise_inherit(r, repo, cfg, e, &refs)?;
    }
    for e in selected_environments(cfg, profile)? {
        mise_inherit(r, repo, cfg, e, &["dotfiles", "status", "--missing"])?;
        mise_inherit(r, repo, cfg, e, &["dotfiles", "status"])?;
    }
    Ok(())
}

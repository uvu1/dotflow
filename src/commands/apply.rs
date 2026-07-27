use crate::config::{Config, Environment};
use crate::error::Result;
use crate::mise::{dotfiles_args, mise_inherit, selected_environments};
use crate::process::Runner;
use std::path::Path;

fn selected_targets<'a>(cli: &'a [String], env: &'a Environment) -> &'a [String] {
    if cli.is_empty() { &env.targets } else { cli }
}
fn run(
    r: &mut Runner,
    repo: &Path,
    cfg: &Config,
    env: &Environment,
    action: &[&str],
    targets: &[String],
) -> Result<()> {
    let a = dotfiles_args(action, targets);
    let refs: Vec<_> = a.iter().map(String::as_str).collect();
    mise_inherit(r, repo, cfg, env, &refs)
}
pub(crate) fn apply(
    r: &mut Runner,
    repo: &Path,
    cfg: &Config,
    profile: &str,
    targets: &[String],
) -> Result<()> {
    for e in selected_environments(cfg, profile)? {
        let t = selected_targets(targets, e);
        run(r, repo, cfg, e, &["dotfiles", "apply", "--yes"], t)?;
    }
    for e in selected_environments(cfg, profile)? {
        let t = selected_targets(targets, e);
        run(r, repo, cfg, e, &["dotfiles", "status", "--missing"], t)?;
        run(r, repo, cfg, e, &["dotfiles", "status"], t)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::config;

    fn calls(r: &Runner) -> Vec<String> {
        r.intended
            .iter()
            .map(|i| {
                i.argv
                    .iter()
                    .skip(3)
                    .map(|x| x.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }
    #[test]
    fn environment_targets_scope_apply_and_both_status_checks() {
        let c = config(
            "[profiles.a]\nplatform='windows'\n[[profiles.a.environments]]\nmise_environment='windows'\ntargets=['~/.config/nvim']\n[[profiles.a.environments]]\nmise_environment='pwsh'\ntargets=['~/.config/starship.toml']",
        );
        let mut r = Runner::new(true);
        apply(&mut r, Path::new("/repo"), &c, "a", &[]).unwrap();
        assert_eq!(
            calls(&r),
            [
                "-E windows dotfiles apply --yes ~/.config/nvim",
                "-E pwsh dotfiles apply --yes ~/.config/starship.toml",
                "-E windows dotfiles status --missing ~/.config/nvim",
                "-E windows dotfiles status ~/.config/nvim",
                "-E pwsh dotfiles status --missing ~/.config/starship.toml",
                "-E pwsh dotfiles status ~/.config/starship.toml",
            ]
        );
    }
    #[test]
    fn cli_targets_override_environment_targets_everywhere() {
        let c = config(
            "[profiles.a]\nplatform='linux'\n[[profiles.a.environments]]\nmise_environment='one'\ntargets=['first']",
        );
        let mut r = Runner::new(true);
        apply(&mut r, Path::new("/repo"), &c, "a", &["cli".to_owned()]).unwrap();
        assert_eq!(
            calls(&r),
            [
                "-E one dotfiles apply --yes cli",
                "-E one dotfiles status --missing cli",
                "-E one dotfiles status cli",
            ]
        );
    }
    #[test]
    fn empty_targets_append_no_arguments() {
        let c = config("[profiles.a]\nplatform='linux'");
        let mut r = Runner::new(true);
        apply(&mut r, Path::new("/repo"), &c, "a", &[]).unwrap();
        assert_eq!(
            calls(&r),
            [
                "dotfiles apply --yes",
                "dotfiles status --missing",
                "dotfiles status",
            ]
        );
    }
    #[test]
    fn common_targets_scope_the_fallback_environment() {
        let c = config("[common]\ntargets=['base']\n[profiles.a]\nplatform='linux'");
        let mut r = Runner::new(true);
        apply(&mut r, Path::new("/repo"), &c, "a", &[]).unwrap();
        assert_eq!(
            calls(&r),
            [
                "dotfiles apply --yes base",
                "dotfiles status --missing base",
                "dotfiles status base",
            ]
        );
    }
}

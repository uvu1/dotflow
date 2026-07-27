use crate::commands::{apply, doctor, dotfiles, init, status, update};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::locator::resolve_repo;
use crate::mise::trust;
use crate::platform::select_profile;
use crate::process::Runner;
use crate::shell::shell_init;
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(version, about)]
pub(crate) struct Cli {
    #[arg(long, global = true)]
    repo: Option<PathBuf>,
    #[arg(long, global = true)]
    profile: Option<String>,
    #[command(subcommand)]
    command: Cmd,
}
#[derive(Subcommand)]
pub(crate) enum Cmd {
    Init {
        path: Option<PathBuf>,
    },
    Root,
    Cd,
    Add {
        #[arg(required = true)]
        target: Vec<String>,
    },
    Edit {
        target: String,
    },
    Status {
        #[arg(long)]
        check: bool,
    },
    Apply {
        target: Vec<String>,
    },
    Update {
        #[arg(long)]
        dry_run: bool,
    },
    Doctor,
    Completion {
        shell: CompletionShell,
    },
    ShellInit {
        shell: String,
    },
}
#[derive(Clone, ValueEnum)]
pub(crate) enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

fn context(cli: &Cli) -> Result<(PathBuf, Config, String)> {
    let repo = resolve_repo(cli.repo.clone())?;
    let cfg = Config::load(&repo)?;
    let profile = select_profile(&cfg, cli.profile.as_deref())?;
    Ok((repo, cfg, profile))
}
pub fn run() -> Result<u8> {
    let matches = Cli::command().get_matches();
    let profile_explicit = matches.value_source("profile").is_some();
    let cli = Cli::from_arg_matches(&matches).map_err(|e| Error(e.to_string()))?;
    match &cli.command {
        Cmd::Init { path } => init::init(path.clone().or(cli.repo.clone())),
        Cmd::Completion { shell } => {
            let mut c = Cli::command();
            generate(
                match shell {
                    CompletionShell::Bash => Shell::Bash,
                    CompletionShell::Zsh => Shell::Zsh,
                    CompletionShell::Fish => Shell::Fish,
                    CompletionShell::Powershell => Shell::PowerShell,
                },
                &mut c,
                "dotflow",
                &mut io::stdout(),
            );
            Ok(0)
        }
        Cmd::ShellInit { shell } => {
            print!("{}", shell_init(shell)?);
            Ok(0)
        }
        _ => {
            let (repo, cfg, profile) = context(&cli)?;
            dispatch(&cli.command, &repo, &cfg, &profile, profile_explicit)
        }
    }
}
fn dispatch(
    cmd: &Cmd,
    repo: &Path,
    cfg: &Config,
    profile: &str,
    profile_explicit: bool,
) -> Result<u8> {
    match cmd {
        Cmd::Root | Cmd::Cd => {
            println!("{}", repo.display());
            Ok(0)
        }
        Cmd::Add { target } => dotfiles::add(repo, cfg, profile, profile_explicit, target),
        Cmd::Edit { target } => dotfiles::edit(repo, cfg, profile, target),
        Cmd::Apply { target } => {
            let mut r = Runner::new(false);
            trust(&mut r, repo, cfg, profile)?;
            apply::apply(&mut r, repo, cfg, profile, target)?;
            Ok(0)
        }
        Cmd::Status { check } => status::status(repo, cfg, profile, *check),
        Cmd::Doctor => doctor::doctor(repo, cfg, profile),
        Cmd::Update { dry_run } => update::update(repo, cfg, profile, profile_explicit, *dry_run),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clap_accepts_global_profile_on_both_sides_and_only_once() {
        for args in [
            ["dotflow", "--profile", "x", "add", "a"],
            ["dotflow", "add", "--profile", "x", "a"],
        ] {
            let m = Cli::command().try_get_matches_from(args).unwrap();
            assert!(m.value_source("profile").is_some());
        }
        let help = Cli::command().render_long_help().to_string();
        assert_eq!(help.matches("--profile").count(), 1);
    }
    #[test]
    fn completions_generate() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
            let mut out = vec![];
            generate(shell, &mut Cli::command(), "dotflow", &mut out);
            assert!(!out.is_empty());
        }
    }
}

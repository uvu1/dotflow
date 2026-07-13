use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use clap_complete::{Shell, generate};
use dotflow::*;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[arg(long, global = true)]
    repo: Option<PathBuf>,
    #[arg(long, global = true)]
    profile: Option<String>,
    #[command(subcommand)]
    command: Cmd,
}
#[derive(Subcommand)]
enum Cmd {
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
enum CompletionShell {
    Bash,
    Zsh,
    Fish,
    Powershell,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("dotflow: {e}");
            ExitCode::FAILURE
        }
    }
}
fn context(cli: &Cli) -> Result<(PathBuf, Config, String)> {
    let repo = resolve_repo(cli.repo.clone())?;
    let cfg = Config::load(&repo)?;
    let profile = select_profile(&cfg, cli.profile.as_deref())?;
    Ok((repo, cfg, profile))
}
fn run() -> Result<u8> {
    let matches = Cli::command().get_matches();
    let profile_explicit = matches.value_source("profile").is_some();
    let cli = Cli::from_arg_matches(&matches).map_err(|e| Error(e.to_string()))?;
    match &cli.command {
        Cmd::Init { path } => init(path.clone().or(cli.repo.clone())),
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

fn init(path: Option<PathBuf>) -> Result<u8> {
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
        Cmd::Add { target } => {
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
            a.extend(target.clone());
            let refs: Vec<_> = a.iter().map(String::as_str).collect();
            mise_inherit(&mut Runner::new(false), repo, cfg, &env, &refs)?;
            Ok(0)
        }
        Cmd::Edit { target } => {
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
        Cmd::Apply { target } => {
            let mut r = Runner::new(false);
            trust(&mut r, repo, cfg, profile)?;
            apply(&mut r, repo, cfg, profile, target)?;
            Ok(0)
        }
        Cmd::Status { check } => status(repo, cfg, profile, *check),
        Cmd::Doctor => doctor(repo, cfg, profile),
        Cmd::Update { dry_run } => update(repo, cfg, profile, profile_explicit, *dry_run),
        _ => unreachable!(),
    }
}
fn apply(
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
fn print_output(out: &std::process::Output) -> Result<()> {
    io::stdout()
        .write_all(&out.stdout)
        .map_err(|e| Error(e.to_string()))?;
    io::stderr()
        .write_all(&out.stderr)
        .map_err(|e| Error(e.to_string()))?;
    Ok(())
}
fn status(repo: &Path, cfg: &Config, profile: &str, check: bool) -> Result<u8> {
    let mut r = Runner::new(false);
    let branch = git(
        &mut r,
        repo,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )?;
    let dirty = git(
        &mut r,
        repo,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    println!(
        "repository: {}\nbranch: {}\nprofile: {}\ngit: {}",
        repo.display(),
        String::from_utf8_lossy(&branch.stdout).trim(),
        profile,
        if dirty.stdout.is_empty() {
            "clean"
        } else {
            "dirty"
        }
    );
    let mut drift = false;
    for e in selected_environments(cfg, profile)? {
        let out = mise_status(&mut r, repo, cfg, e, &["dotfiles", "status", "--missing"])?;
        print_output(&out)?;
        if !out.status.success() {
            drift = true;
        }
    }
    Ok(u8::from(check && (!dirty.stdout.is_empty() || drift)))
}
fn doctor(repo: &Path, cfg: &Config, profile: &str) -> Result<u8> {
    println!(
        "dotflow {}\nrepo: {}\nprofile: {}\nconfig: valid",
        VERSION,
        repo.display(),
        profile
    );
    let mut r = Runner::new(false);
    validate_repo(&mut r, repo, cfg, false)?;
    for tool in ["git", "mise"] {
        r.capture(tool, [OsStr::new("--version")], None, &BTreeMap::new())
            .map_err(|_| Error(format!("{tool} is required and must be on PATH")))?;
        println!("{tool}: available");
    }
    for e in selected_environments(cfg, profile)? {
        println!(
            "mise environment: {}",
            e.mise_environment.as_deref().unwrap_or("base")
        );
    }
    Ok(0)
}
fn stage<T>(name: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    println!("==> {name}");
    f().map_err(|e| Error(format!("stage {name} failed: {e}")))
}
fn update(
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

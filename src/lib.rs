use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output, Stdio};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    #[serde(default)]
    pub minimum_dotflow_version: Option<String>,
    pub repository: Repository,
    #[serde(default)]
    pub common: Environment,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    pub branch: String,
    pub origin: String,
    #[serde(default = "default_mise_dir")]
    pub mise_dir: PathBuf,
}
fn default_mise_dir() -> PathBuf {
    PathBuf::from("mise")
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    #[serde(default)]
    pub mise_environment: Option<String>,
    #[serde(default)]
    pub targets: Vec<String>,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Wsl,
    Linux,
    Macos,
    Windows,
}
impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Wsl => "wsl",
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
        })
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub platform: Platform,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub add_environment: Option<String>,
    #[serde(default)]
    pub environments: Vec<Environment>,
    #[serde(default)]
    pub after_sync: Vec<Hook>,
    #[serde(default)]
    pub before_install: Vec<Hook>,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

impl Config {
    pub fn load(repo: &Path) -> Result<Self> {
        let path = repo.join(".dotflow.toml");
        let text = fs::read_to_string(&path)
            .map_err(|e| Error(format!("cannot read {}: {e}", path.display())))?;
        let config: Self =
            toml::from_str(&text).map_err(|e| Error(format!("invalid {}: {e}", path.display())))?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(Error(format!(
                "unsupported schema version {}; this dotflow supports {SCHEMA_VERSION}",
                self.schema_version
            )));
        }
        if let Some(req) = &self.minimum_dotflow_version {
            let requirement = VersionReq::parse(req)
                .or_else(|_| VersionReq::parse(&format!(">={req}")))
                .map_err(|e| Error(format!("invalid minimum_dotflow_version: {e}")))?;
            let current = Version::parse(VERSION).map_err(|e| Error(e.to_string()))?;
            if !requirement.matches(&current) {
                return Err(Error(format!(
                    "dotflow {VERSION} does not satisfy minimum version {req}"
                )));
            }
        }
        if self.repository.branch.trim().is_empty() || self.repository.origin.trim().is_empty() {
            return Err(Error("repository branch and origin are required".into()));
        }
        for (name, profile) in &self.profiles {
            if profile.arch.as_ref().is_some_and(|a| a.trim().is_empty()) {
                return Err(Error(format!("profile {name} has an empty arch")));
            }
            if profile
                .add_environment
                .as_ref()
                .is_some_and(|a| a.trim().is_empty())
            {
                return Err(Error(format!(
                    "profile {name} has an empty add_environment"
                )));
            }
        }
        Ok(())
    }
}

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

pub fn detected_platform() -> Platform {
    if cfg!(windows) {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::Macos
    } else if env::var_os("WSL_DISTRO_NAME").is_some()
        || fs::read_to_string("/proc/version")
            .map(|s| s.to_ascii_lowercase().contains("microsoft"))
            .unwrap_or(false)
    {
        Platform::Wsl
    } else {
        Platform::Linux
    }
}
pub fn select_profile_for(
    config: &Config,
    requested: Option<&str>,
    platform: Platform,
    arch: &str,
) -> Result<String> {
    if let Some(name) = requested {
        return config
            .profiles
            .contains_key(name)
            .then(|| name.to_owned())
            .ok_or_else(|| Error(format!("unknown profile: {name}")));
    }
    let candidates: Vec<_> = config
        .profiles
        .iter()
        .filter(|(_, p)| {
            p.platform == platform
                && p.arch
                    .as_deref()
                    .is_none_or(|a| a.eq_ignore_ascii_case(arch))
        })
        .map(|(n, _)| n.clone())
        .collect();
    match candidates.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(Error(format!(
            "no profile matches platform {platform} and arch {arch}; use --profile"
        ))),
        _ => Err(Error(format!(
            "multiple profiles match platform {platform} and arch {arch}: {}; use --profile",
            candidates.join(", ")
        ))),
    }
}
pub fn select_profile(config: &Config, requested: Option<&str>) -> Result<String> {
    select_profile_for(config, requested, detected_platform(), env::consts::ARCH)
}

pub fn normalize_remote(value: &str) -> Option<String> {
    let mut s = value.trim().trim_end_matches('/');
    if let Some(v) = s.strip_suffix(".git") {
        s = v;
    }
    let rest = s
        .strip_prefix("git@github.com:")
        .or_else(|| s.strip_prefix("ssh://git@github.com/"))
        .or_else(|| s.strip_prefix("https://github.com/"))?;
    let parts: Vec<_> = rest.split('/').collect();
    (parts.len() == 2 && parts.iter().all(|x| !x.is_empty()))
        .then(|| format!("github.com/{}/{}", parts[0], parts[1]))
}
pub fn remotes_equal(actual: &str, expected: &str) -> bool {
    match (normalize_remote(actual), normalize_remote(expected)) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(&b),
        (None, None) => canonical_remote(actual) == canonical_remote(expected),
        _ => false,
    }
}
fn canonical_remote(value: &str) -> String {
    value.trim().trim_end_matches('/').to_owned()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IoMode {
    Capture,
    Inherit,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Invocation {
    pub argv: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub io: IoMode,
}
pub struct Runner {
    pub dry_run: bool,
    pub intended: Vec<Invocation>,
}
impl Runner {
    pub fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            intended: vec![],
        }
    }
    fn prepare<I, S>(
        &mut self,
        program: &str,
        args: I,
        cwd: Option<&Path>,
        vars: &BTreeMap<String, String>,
        io: IoMode,
    ) -> (Vec<OsString>, Command)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let argv: Vec<_> = args
            .into_iter()
            .map(|x| x.as_ref().to_os_string())
            .collect();
        let mut shown = vec![OsString::from(program)];
        shown.extend(argv.clone());
        self.intended.push(Invocation {
            argv: shown,
            cwd: cwd.map(Path::to_path_buf),
            env: vars.clone(),
            io,
        });
        let mut cmd = Command::new(program);
        cmd.args(&argv).envs(vars);
        if let Some(c) = cwd {
            cmd.current_dir(c);
        }
        (argv, cmd)
    }
    pub fn capture<I, S>(
        &mut self,
        program: &str,
        args: I,
        cwd: Option<&Path>,
        vars: &BTreeMap<String, String>,
    ) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let (argv, mut cmd) = self.prepare(program, args, cwd, vars, IoMode::Capture);
        if self.dry_run {
            return Ok(success_output());
        }
        let out = cmd
            .output()
            .map_err(|e| Error(format!("cannot run {}: {e}", display_argv(program, &argv))))?;
        if out.status.success() {
            Ok(out)
        } else {
            Err(command_error(
                program,
                &argv,
                &out.stdout,
                &out.stderr,
                out.status,
            ))
        }
    }
    pub fn capture_status<I, S>(
        &mut self,
        program: &str,
        args: I,
        cwd: Option<&Path>,
        vars: &BTreeMap<String, String>,
    ) -> Result<Output>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let (argv, mut cmd) = self.prepare(program, args, cwd, vars, IoMode::Capture);
        if self.dry_run {
            return Ok(success_output());
        }
        cmd.output()
            .map_err(|e| Error(format!("cannot run {}: {e}", display_argv(program, &argv))))
    }
    pub fn inherit<I, S>(
        &mut self,
        program: &str,
        args: I,
        cwd: Option<&Path>,
        vars: &BTreeMap<String, String>,
    ) -> Result<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let (argv, mut cmd) = self.prepare(program, args, cwd, vars, IoMode::Inherit);
        if self.dry_run {
            return Ok(());
        }
        let status = cmd
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| Error(format!("cannot run {}: {e}", display_argv(program, &argv))))?;
        if status.success() {
            Ok(())
        } else {
            Err(Error(format!(
                "command failed with {status}: {}",
                display_argv(program, &argv)
            )))
        }
    }
}
fn command_error(
    program: &str,
    argv: &[OsString],
    stdout: &[u8],
    stderr: &[u8],
    status: ExitStatus,
) -> Error {
    let mut msg = format!(
        "command failed with {status}: {}",
        display_argv(program, argv)
    );
    if !stdout.is_empty() {
        msg.push_str("\nstdout:\n");
        msg.push_str(&String::from_utf8_lossy(stdout));
    }
    if !stderr.is_empty() {
        msg.push_str("\nstderr:\n");
        msg.push_str(&String::from_utf8_lossy(stderr));
    }
    Error(msg)
}
#[cfg(unix)]
fn success_output() -> Output {
    use std::os::unix::process::ExitStatusExt;
    Output {
        status: ExitStatus::from_raw(0),
        stdout: vec![],
        stderr: vec![],
    }
}
#[cfg(windows)]
fn success_output() -> Output {
    use std::os::windows::process::ExitStatusExt;
    Output {
        status: ExitStatus::from_raw(0),
        stdout: vec![],
        stderr: vec![],
    }
}
pub fn display_argv(p: &str, a: &[OsString]) -> String {
    std::iter::once(OsStr::new(p))
        .chain(a.iter().map(OsString::as_os_str))
        .map(|x| format!("{:?}", x.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn display_invocation(i: &Invocation) -> String {
    i.argv
        .iter()
        .map(|x| format!("{:?}", x.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ")
}
pub fn expand(value: &str, repo: &Path, home: &Path, backend: &Path) -> String {
    value
        .replace("{repo}", &repo.to_string_lossy())
        .replace("{home}", &home.to_string_lossy())
        .replace("{backend}", &backend.to_string_lossy())
}

pub fn git(r: &mut Runner, repo: &Path, args: &[&str]) -> Result<Output> {
    let mut a = vec![OsStr::new("-C"), repo.as_os_str()];
    a.extend(args.iter().map(OsStr::new));
    r.capture("git", a, None, &BTreeMap::new())
}
pub fn validate_repo(r: &mut Runner, repo: &Path, cfg: &Config, require_clean: bool) -> Result<()> {
    let top = git(r, repo, &["rev-parse", "--show-toplevel"])?;
    if fs::canonicalize(String::from_utf8_lossy(&top.stdout).trim()).ok()
        != fs::canonicalize(repo).ok()
    {
        return Err(Error("repository path is not the Git worktree root".into()));
    }
    let branch = git(r, repo, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .map_err(|e| Error(format!("expected an attached branch: {e}")))?;
    let actual_branch = String::from_utf8_lossy(&branch.stdout);
    let actual_branch = actual_branch.trim();
    if actual_branch != cfg.repository.branch {
        return Err(Error(format!(
            "expected branch {}, found {actual_branch}",
            cfg.repository.branch
        )));
    }
    let origin = git(r, repo, &["remote", "get-url", "origin"])?;
    let actual_origin = String::from_utf8_lossy(&origin.stdout);
    if !remotes_equal(actual_origin.trim(), &cfg.repository.origin) {
        return Err(Error(format!(
            "origin does not match configured repository: expected {:?}, found {:?}",
            cfg.repository.origin,
            actual_origin.trim()
        )));
    }
    let status = git(
        r,
        repo,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if require_clean && !status.stdout.is_empty() {
        return Err(Error(format!(
            "worktree is not clean:\n{}",
            String::from_utf8_lossy(&status.stdout)
        )));
    }
    Ok(())
}
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
pub fn run_hooks(r: &mut Runner, hooks: &[Hook], repo: &Path, backend: &Path) -> Result<()> {
    let home = home_dir()?;
    for h in hooks {
        let args: Vec<_> = h
            .args
            .iter()
            .map(|v| expand(v, repo, &home, backend))
            .collect();
        let cwd = h
            .cwd
            .as_ref()
            .map(|v| PathBuf::from(expand(v, repo, &home, backend)));
        let vars = h
            .env
            .iter()
            .map(|(k, v)| (k.clone(), expand(v, repo, &home, backend)))
            .collect();
        r.inherit(
            &expand(&h.program, repo, &home, backend),
            &args,
            cwd.as_deref(),
            &vars,
        )?;
    }
    Ok(())
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

pub fn shell_init(shell: &str) -> Result<String> {
    let s = match shell {
        "bash" => {
            "dotflow() { if [ \"${1-}\" = cd ]; then shift; cd \"$(command dotflow root \"$@\")\"; else command dotflow \"$@\"; fi; }\n"
        }
        "zsh" => {
            "function dotflow { if [[ ${1-} == cd ]]; then shift; cd \"$(command dotflow root \"$@\")\"; else command dotflow \"$@\"; fi }\n"
        }
        "fish" => {
            "function dotflow; if test (count $argv) -gt 0; and test \"$argv[1]\" = cd; set -e argv[1]; cd (command dotflow root $argv); else; command dotflow $argv; end; end\n"
        }
        "powershell" => {
            "function dotflow { if ($args.Count -gt 0 -and $args[0] -eq 'cd') { Set-Location (& dotflow.exe root @($args | Select-Object -Skip 1)) } else { & dotflow.exe @args } }\n"
        }
        _ => return Err(Error(format!("unsupported shell: {shell}"))),
    };
    Ok(s.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn config(profiles: &str) -> Config {
        toml::from_str(&format!("schema_version=1\n[repository]\nbranch='master'\norigin='git@github.com:a/b.git'\n{profiles}")).unwrap()
    }
    fn git_cmd(dir: &Path, args: &[&str]) {
        let o = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
    fn repository() -> (TempDir, Config) {
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
    #[test]
    fn selection_platform_arch_override() {
        let c = config(
            "[profiles.w]\nplatform='wsl'\n[profiles.l]\nplatform='linux'\narch='x86_64'\n[profiles.m]\nplatform='macos'\n[profiles.win]\nplatform='windows'",
        );
        assert_eq!(
            select_profile_for(&c, None, Platform::Wsl, "x86_64").unwrap(),
            "w"
        );
        assert_eq!(
            select_profile_for(&c, None, Platform::Linux, "x86_64").unwrap(),
            "l"
        );
        assert_eq!(
            select_profile_for(&c, None, Platform::Macos, "aarch64").unwrap(),
            "m"
        );
        assert_eq!(
            select_profile_for(&c, None, Platform::Windows, "x86_64").unwrap(),
            "win"
        );
        assert_eq!(
            select_profile_for(&c, Some("m"), Platform::Linux, "bad").unwrap(),
            "m"
        );
        assert!(select_profile_for(&c, None, Platform::Linux, "aarch64").is_err());
    }
    #[test]
    fn selection_rejects_multiple_and_none() {
        let c = config("[profiles.a]\nplatform='linux'\n[profiles.b]\nplatform='linux'");
        assert!(
            select_profile_for(&c, None, Platform::Linux, "x86_64")
                .unwrap_err()
                .0
                .contains("multiple")
        );
        assert!(
            select_profile_for(&c, None, Platform::Wsl, "x86_64")
                .unwrap_err()
                .0
                .contains("no profile")
        );
    }
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
    fn remote_comparison_is_safe() {
        assert!(remotes_equal(
            "git@github.com:A/B.git",
            "https://github.com/a/b"
        ));
        assert!(remotes_equal("file:///one", "file:///one"));
        assert!(!remotes_equal(
            "https://one.invalid/a",
            "https://two.invalid/b"
        ));
        assert!(!remotes_equal(
            "https://github.com/a/b",
            "https://example.com/a/b"
        ));
    }
    #[test]
    fn expansion_uses_absolute_backend_without_shell_evaluation() {
        assert_eq!(
            expand(
                "{repo}:{home}:{backend}:$HOME:$(x)",
                Path::new("/r"),
                Path::new("/h"),
                Path::new("/r/mise")
            ),
            "/r:/h:/r/mise:$HOME:$(x)"
        );
    }
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
    #[test]
    fn validation_clean_dirty_untracked_branch_detached_origin() {
        let (t, c) = repository();
        validate_repo(&mut Runner::new(false), t.path(), &c, true).unwrap();
        fs::write(t.path().join("tracked"), "two").unwrap();
        assert!(validate_repo(&mut Runner::new(false), t.path(), &c, true).is_err());
        git_cmd(t.path(), &["restore", "tracked"]);
        fs::write(t.path().join("new"), "x").unwrap();
        assert!(validate_repo(&mut Runner::new(false), t.path(), &c, true).is_err());
        fs::remove_file(t.path().join("new")).unwrap();
        git_cmd(t.path(), &["switch", "-c", "other"]);
        assert!(validate_repo(&mut Runner::new(false), t.path(), &c, false).is_err());
        git_cmd(t.path(), &["switch", "master"]);
        git_cmd(t.path(), &["checkout", "--detach"]);
        assert!(validate_repo(&mut Runner::new(false), t.path(), &c, false).is_err());
    }
    #[test]
    fn real_git_ff_only_accepts_fast_forward_and_rejects_divergence() {
        let t = TempDir::new().unwrap();
        let remote = t.path().join("remote.git");
        let seed = t.path().join("seed");
        let local = t.path().join("local");
        fs::create_dir(&remote).unwrap();
        fs::create_dir(&seed).unwrap();
        git_cmd(&remote, &["init", "--bare"]);
        git_cmd(&seed, &["init", "-b", "master"]);
        for dir in [&seed, &local] {
            if dir.exists() {
                git_cmd(dir, &["config", "user.name", "Test"]);
                git_cmd(dir, &["config", "user.email", "test@example.invalid"]);
                git_cmd(dir, &["config", "commit.gpgsign", "false"]);
            }
        }
        fs::write(seed.join("file"), "one").unwrap();
        git_cmd(&seed, &["add", "file"]);
        git_cmd(&seed, &["commit", "-m", "one"]);
        git_cmd(
            &seed,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git_cmd(&seed, &["push", "-u", "origin", "master"]);
        let clone = Command::new("git")
            .args(["clone", remote.to_str().unwrap(), local.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(
            clone.status.success(),
            "{}",
            String::from_utf8_lossy(&clone.stderr)
        );
        git_cmd(&local, &["config", "user.name", "Test"]);
        git_cmd(&local, &["config", "user.email", "test@example.invalid"]);
        git_cmd(&local, &["config", "commit.gpgsign", "false"]);
        fs::write(seed.join("file"), "two").unwrap();
        git_cmd(&seed, &["commit", "-am", "two"]);
        git_cmd(&seed, &["push"]);
        git_cmd(&local, &["pull", "--ff-only", "origin", "master"]);
        fs::write(local.join("local"), "local").unwrap();
        git_cmd(&local, &["add", "local"]);
        git_cmd(&local, &["commit", "-m", "local"]);
        fs::write(seed.join("remote"), "remote").unwrap();
        git_cmd(&seed, &["add", "remote"]);
        git_cmd(&seed, &["commit", "-m", "remote"]);
        git_cmd(&seed, &["push"]);
        let diverged = Command::new("git")
            .arg("-C")
            .arg(&local)
            .args(["pull", "--ff-only", "origin", "master"])
            .output()
            .unwrap();
        assert!(!diverged.status.success());
    }
    #[test]
    fn runner_records_exact_modes_cwd_env_and_dry_run() {
        let mut r = Runner::new(true);
        let vars = BTreeMap::from([("K".into(), "V".into())]);
        r.capture("no-such", ["two words"], Some(Path::new("/tmp")), &vars)
            .unwrap();
        r.inherit("no-such", ["$(unsafe)"], None, &BTreeMap::new())
            .unwrap();
        assert_eq!(r.intended[0].io, IoMode::Capture);
        assert_eq!(r.intended[0].cwd, Some(PathBuf::from("/tmp")));
        assert_eq!(r.intended[0].env, vars);
        assert_eq!(r.intended[1].io, IoMode::Inherit);
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
    #[test]
    fn shell_functions_handle_no_args() {
        for s in ["bash", "zsh", "fish", "powershell"] {
            assert!(shell_init(s).unwrap().contains("dotflow"));
        }
    }
    #[cfg(unix)]
    #[test]
    fn bash_wrapper_changes_parent_directory_and_handles_nounset() {
        if Command::new("bash").arg("--version").output().is_err() {
            return;
        }
        let t = TempDir::new().unwrap();
        let bin = t.path().join("dotflow");
        fs::write(
            &bin,
            "#!/bin/sh\nif [ \"$1\" = root ]; then printf '%s\\n' \"$DOTFLOW_TEST_ROOT\"; fi\n",
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        let script = format!(
            "set -u; {} dotflow >/dev/null; dotflow cd; pwd",
            shell_init("bash").unwrap()
        );
        let o = Command::new("bash")
            .arg("-c")
            .arg(script)
            .env(
                "PATH",
                format!("{}:{}", t.path().display(), env::var("PATH").unwrap()),
            )
            .env("DOTFLOW_TEST_ROOT", t.path())
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(
            String::from_utf8_lossy(&o.stdout).trim(),
            t.path().to_string_lossy()
        );
    }
}

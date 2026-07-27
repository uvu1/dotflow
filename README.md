# dotflow

`dotflow` is a small, public MIT-licensed Rust CLI for safely operating a cloned,
Git-backed [mise](https://mise.jdx.dev/) dotfiles repository. It coordinates Git,
mise environments, target sets, and explicit argv hooks without evaluating a shell.

## Install

Rust 1.85 or newer is required when building from source:

```sh
cargo install --locked --path .
# After a release exists:
cargo install --locked dotflow
```

Release archives are produced for x86-64 Linux, Apple Silicon macOS, and x86-64
Windows. Crates.io publication is deliberately manual.

## Register a repository

Clone your dotfiles yourself, then run:

```sh
dotflow init ~/dotfiles
```

`init` only registers an existing Git worktree. It never clones, initializes Git,
scaffolds the repository, or overwrites existing configuration. Repository lookup
uses `--repo`, then `DOTFLOW_REPO`, then the user locator, then `~/dotfiles`. The
locator is `$XDG_CONFIG_HOME/dotflow/config.toml` (normally
`~/.config/dotflow/config.toml`) or `%APPDATA%\dotflow\config.toml` on Windows.

## Configuration

The repository contains `.dotflow.toml`:

```toml
schema_version = 1
minimum_dotflow_version = "0.1.0"

[repository]
branch = "master"
origin = "git@github.com:owner/dotfiles.git"
mise_dir = "mise"

[common]
targets = ["~/.gitconfig"]

[profiles.work-wsl]
platform = "wsl"
arch = "x86_64" # optional
add_environment = "linux"

[[profiles.work-wsl.environments]]
mise_environment = "linux"
targets = ["~/.zshrc"]

[[profiles.work-wsl.after_sync]]
program = "home-manager"
args = ["switch", "--flake", "{repo}#me@wsl"]
cwd = "{repo}"

[[profiles.work-wsl.before_install]]
program = "tool"
args = ["--backend", "{backend}"]
[profiles.work-wsl.before_install.env]
DOTFILES_HOME = "{home}"

[profiles.windows-desktop]
platform = "windows"
add_environment = "windows"

[[profiles.windows-desktop.environments]]
mise_environment = "windows"
targets = ["~/AppData/Roaming/tool/config.toml"]

[[profiles.windows-desktop.environments]]
mise_environment = "pwsh"
targets = ["~/Documents/PowerShell/Microsoft.PowerShell_profile.ps1"]
```

Profile names are arbitrary. Auto-selection compares the required `platform` and
optional `arch` fields and requires exactly one match; use `--profile` to override.
A profile may contain any number of environment tables. With none, update/apply
use `[common]` once. Only an environment's `targets` are applied and checked;
dotfiles that the merged mise config manages but no environment lists are left
alone. Use `mise -C <mise_dir> -E <env> dotfiles status` for the full merged view.
`add` without an explicitly supplied `--profile` always uses
the base mise config; explicit profile add and `edit` use `add_environment`.
Only the selected profile's hooks run. Only `{repo}`, `{home}`, and `{backend}` are expanded in hook
arguments, cwd, environment values, and program; strings are passed directly as
argv and are never shell-evaluated. `{backend}` is the absolute repository
`mise_dir`. Schema and minimum-version checks fail early.

## Commands

```text
dotflow [--repo PATH] [--profile PROFILE] init [PATH]
dotflow root                         print the repository root
dotflow cd                           print the root (use shell integration to cd)
dotflow [--profile PROFILE] add TARGET...
dotflow edit TARGET
dotflow status [--check]
dotflow apply [TARGET...]
dotflow update [--dry-run]
dotflow doctor
dotflow completion <bash|zsh|fish|powershell>
dotflow shell-init <bash|zsh|fish|powershell>
```

`add` streams mise output and never stages Git. `edit`, apply/install operations,
and hooks inherit the terminal, including stdin. `apply` trusts the base config and
each selected environment config exactly once, applies the selected profile's
environments, then shows the status of the same targets it applied. It never pulls
or installs. `status` displays mise status output for each selected environment's
targets; `status --check` returns nonzero for a dirty repository or mise drift in
those targets (`mise dotfiles status --missing`).

For parent-shell `cd`, source the generated wrapper, for example:

```sh
eval "$(dotflow shell-init zsh)"
# bash: eval "$(dotflow shell-init bash)"
# fish: dotflow shell-init fish | source
```

PowerShell:

```powershell
Invoke-Expression (& dotflow shell-init powershell | Out-String)
```

Generate static completion with `dotflow completion zsh` (or another shell).

## Safe updates

`dotflow update` validates the exact repository root, configured branch, normalized
GitHub origin, clean tracked and untracked state, and dependencies before pulling.
It then performs an exact `git pull --ff-only origin <branch>`, reloads and validates
configuration, revalidates the selected profile, runs that profile's `after_sync`
hooks, trusts configs, applies targets and checks missing then normal status, runs
that profile's `before_install` hooks, and installs each selected mise environment.
Applying before installing lets a dotfile-distributed mise config define the toolset
that `install` then resolves, which is what copy distribution otherwise breaks.
Failures stop immediately. It never commits, pushes, stashes, resets,
rebases, or deletes files.

`dotflow update --dry-run` performs local validation, makes no network or filesystem
changes, and prints the ordered argv that would run. GitHub SSH, `ssh://`, and HTTPS
origins are compared after normalization, with an optional `.git` suffix accepted.

Typical workflow:

```sh
dotflow doctor
dotflow status --check
dotflow update --dry-run
dotflow update
dotflow add ~/.config/tool/config.toml
git -C "$(dotflow root)" diff
```

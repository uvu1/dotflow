use crate::config::Hook;
use crate::error::Result;
use crate::locator::home_dir;
use crate::process::Runner;
use std::path::{Path, PathBuf};

pub fn expand(value: &str, repo: &Path, home: &Path, backend: &Path) -> String {
    value
        .replace("{repo}", &repo.to_string_lossy())
        .replace("{home}", &home.to_string_lossy())
        .replace("{backend}", &backend.to_string_lossy())
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

#[cfg(test)]
mod tests {
    use super::*;

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
}

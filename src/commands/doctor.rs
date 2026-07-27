use crate::VERSION;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::git::validate_repo;
use crate::mise::selected_environments;
use crate::process::Runner;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::Path;

pub(crate) fn doctor(repo: &Path, cfg: &Config, profile: &str) -> Result<u8> {
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

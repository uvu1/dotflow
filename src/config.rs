use crate::error::{Error, Result};
use crate::platform::Platform;
use crate::{SCHEMA_VERSION, VERSION};
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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

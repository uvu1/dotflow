use crate::config::Config;
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fmt;
use std::fs;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::config;

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
}

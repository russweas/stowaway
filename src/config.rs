use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineConfig {
    pub version: u32,
    pub ssh: SshConfig,
    #[serde(default)]
    pub trees: Vec<TreeConfig>,
    #[serde(default)]
    pub metadata: Vec<FileMetadata>,
    #[serde(default)]
    pub scripts: Vec<ScriptConfig>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SshConfig {
    pub destination: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeConfig {
    pub source: PathBuf,
    pub target: String,
    #[serde(default)]
    pub privileged: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileMetadata {
    pub source: PathBuf,
    pub mode: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptConfig {
    pub path: PathBuf,
    #[serde(default)]
    pub privileged: bool,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    1_800
}

impl MachineConfig {
    pub fn parse(path: &Path) -> Result<Self> {
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("could not read {}", path.display()))?;
        let config: Self = toml::from_str(&source)
            .with_context(|| format!("invalid configuration in {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1,
            "unsupported machine version {}",
            self.version
        );
        ensure!(
            !self.ssh.destination.trim().is_empty(),
            "SSH destination cannot be empty"
        );
        ensure!(
            !self.trees.is_empty(),
            "at least one [[trees]] entry is required"
        );

        let mut sources = BTreeSet::new();
        for tree in &self.trees {
            validate_relative(&tree.source, "tree source")?;
            ensure!(
                sources.insert(tree.source.clone()),
                "duplicate tree source {}",
                tree.source.display()
            );
            if tree.privileged {
                ensure!(
                    tree.target.starts_with('/'),
                    "privileged target must be absolute: {}",
                    tree.target
                );
            } else {
                ensure!(
                    tree.target == "~" || tree.target.starts_with("~/"),
                    "unprivileged target must be within '~': {}",
                    tree.target
                );
            }
            validate_remote_target(&tree.target)?;
        }

        let mut metadata = BTreeSet::new();
        for entry in &self.metadata {
            validate_relative(&entry.source, "metadata source")?;
            ensure!(
                metadata.insert(entry.source.clone()),
                "duplicate metadata for {}",
                entry.source.display()
            );
            parse_mode(&entry.mode)?;
        }

        let mut scripts = BTreeSet::new();
        for script in &self.scripts {
            validate_relative(&script.path, "script path")?;
            ensure!(
                scripts.insert(script.path.clone()),
                "duplicate script {}",
                script.path.display()
            );
            ensure!(
                script.timeout_seconds > 0,
                "script timeout must be greater than zero"
            );
        }
        Ok(())
    }
}

fn validate_remote_target(target: &str) -> Result<()> {
    ensure!(
        !target.contains(['\n', '\r', '\t', '\0']),
        "remote target contains an invalid character"
    );
    let path = if target == "~" {
        return Ok(());
    } else if let Some(relative) = target.strip_prefix("~/") {
        Path::new(relative)
    } else {
        Path::new(target)
    };

    for component in path.components() {
        match component {
            Component::RootDir | Component::Normal(_) => {}
            _ => bail!("remote target contains an unsafe component: {target}"),
        }
    }
    ensure!(
        !target.ends_with('/') || target == "/",
        "remote target must not end with '/': {target}"
    );
    Ok(())
}

pub fn validate_relative(path: &Path, label: &str) -> Result<()> {
    ensure!(!path.as_os_str().is_empty(), "{label} cannot be empty");
    ensure!(
        !path.is_absolute(),
        "{label} must be relative: {}",
        path.display()
    );
    for component in path.components() {
        match component {
            Component::Normal(_) => {}
            _ => bail!("{label} contains an unsafe component: {}", path.display()),
        }
    }
    Ok(())
}

pub fn parse_mode(mode: &str) -> Result<u32> {
    ensure!(
        mode.len() == 4 && mode.starts_with('0'),
        "mode must contain four octal digits, such as 0644: {mode}"
    );
    let parsed = u32::from_str_radix(mode, 8).with_context(|| format!("invalid mode {mode}"))?;
    ensure!(parsed <= 0o7777, "invalid mode {mode}");
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_parent_components() {
        assert!(validate_relative(Path::new("../etc"), "path").is_err());
    }

    #[test]
    fn parses_modes() {
        assert_eq!(parse_mode("0755").unwrap(), 0o755);
        assert!(parse_mode("755").is_err());
        assert!(parse_mode("0999").is_err());
    }

    #[test]
    fn rejects_escaping_remote_targets() {
        assert!(validate_remote_target("~/../etc").is_err());
        assert!(validate_remote_target("/etc/../root").is_err());
        assert!(validate_remote_target("~/.config").is_ok());
        assert!(validate_remote_target("/etc/network").is_ok());
        assert!(validate_remote_target("~/bad\tname").is_err());
    }
}

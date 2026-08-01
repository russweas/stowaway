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
    #[serde(default)]
    pub udev_rules: Vec<UdevRuleConfig>,
    #[serde(default)]
    pub apt_packages: Vec<AptPackageConfig>,
    #[serde(default)]
    pub containers: Vec<ContainerConfig>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ContainerConfig {
    pub name: String,
    pub image: String,
    #[serde(default = "default_restart_policy")]
    pub restart: String,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub environment: Vec<String>,
}

fn default_restart_policy() -> String {
    "unless-stopped".into()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AptPackageConfig {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AptConstraint {
    pub operator: String,
    pub version: String,
}

impl AptPackageConfig {
    pub fn constraints(&self) -> Result<Vec<AptConstraint>> {
        parse_version_range(&self.version)
    }
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UdevRuleConfig {
    pub source: PathBuf,
    pub name: String,
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

        let mut udev_rules = BTreeSet::new();
        for rule in &self.udev_rules {
            validate_relative(&rule.source, "udev rule source")?;
            ensure!(
                udev_rules.insert(rule.source.clone()),
                "duplicate udev rule source {}",
                rule.source.display()
            );
            ensure!(
                rule.name.ends_with(".rules")
                    && !rule.name.is_empty()
                    && rule
                        .name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric()
                            || matches!(byte, b'-' | b'_' | b'.')),
                "udev rule name must be a simple .rules filename: {}",
                rule.name
            );
        }
        let mut packages = BTreeSet::new();
        for package in &self.apt_packages {
            ensure!(
                !package.name.is_empty()
                    && package.name.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'.' | b'-')
                    }),
                "invalid APT package name: {}",
                package.name
            );
            ensure!(
                packages.insert(&package.name),
                "duplicate APT package: {}",
                package.name
            );
            package.constraints()?;
        }
        let mut containers = BTreeSet::new();
        for container in &self.containers {
            ensure!(
                !container.name.is_empty()
                    && container.name.len() <= 128
                    && container.name.as_bytes()[0].is_ascii_alphanumeric()
                    && container.name.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
                    }),
                "invalid Docker container name: {}",
                container.name
            );
            ensure!(
                containers.insert(&container.name),
                "duplicate Docker container: {}",
                container.name
            );
            ensure!(
                !container.image.is_empty()
                    && !container.image.chars().any(char::is_whitespace)
                    && !container.image.contains(['\n', '\r', '\0']),
                "invalid Docker image: {}",
                container.image
            );
            ensure!(
                matches!(
                    container.restart.as_str(),
                    "no" | "always" | "on-failure" | "unless-stopped"
                ),
                "invalid Docker restart policy: {}",
                container.restart
            );
            for port in &container.ports {
                validate_port_mapping(port)?;
            }
            for environment in &container.environment {
                let (key, _) = environment.split_once('=').with_context(|| {
                    format!("Docker environment entry must be KEY=VALUE: {environment}")
                })?;
                ensure!(
                    !key.is_empty()
                        && key.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
                        }),
                    "invalid Docker environment key: {key}"
                );
            }
        }
        Ok(())
    }
}

fn validate_port_mapping(mapping: &str) -> Result<()> {
    let (host, container) = mapping
        .split_once(':')
        .context("Docker port must use HOST:CONTAINER format")?;
    ensure!(
        !host.is_empty() && !container.is_empty() && !mapping.contains(['\n', '\r', '\t', '\0']),
        "invalid Docker port mapping: {mapping}"
    );
    for port in [host, container] {
        let value: u16 = port
            .parse()
            .with_context(|| format!("invalid Docker port in mapping: {mapping}"))?;
        ensure!(
            value > 0,
            "Docker ports must be greater than zero: {mapping}"
        );
    }
    Ok(())
}

pub fn parse_version_range(range: &str) -> Result<Vec<AptConstraint>> {
    let mut constraints = Vec::new();
    for part in range.split(',') {
        let part = part.trim();
        ensure!(
            !part.is_empty(),
            "APT version range contains an empty constraint"
        );
        let (operator, version) = [">=", "<=", ">", "<", "="]
            .iter()
            .find_map(|operator| {
                part.strip_prefix(operator)
                    .map(|version| (*operator, version.trim()))
            })
            .unwrap_or(("=", part));
        ensure!(!version.is_empty(), "APT version constraint has no version");
        ensure!(
            !version.chars().any(char::is_whitespace)
                && version.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b':' | b'+' | b'~' | b'-')
                }),
            "invalid APT version: {version}"
        );
        constraints.push(AptConstraint {
            operator: operator.to_owned(),
            version: version.to_owned(),
        });
    }
    Ok(constraints)
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

    #[test]
    fn validates_udev_rule_names() {
        let mut config = MachineConfig {
            version: 1,
            ssh: SshConfig {
                destination: "host".into(),
            },
            trees: vec![TreeConfig {
                source: "home".into(),
                target: "~".into(),
                privileged: false,
            }],
            metadata: Vec::new(),
            scripts: Vec::new(),
            udev_rules: vec![UdevRuleConfig {
                source: "rules/10-disk.rules".into(),
                name: "10-disk.rules".into(),
            }],
            apt_packages: Vec::new(),
            containers: Vec::new(),
        };
        assert!(config.validate().is_ok());
        config.udev_rules[0].name = "../unsafe.rules".into();
        assert!(config.validate().is_err());
    }

    #[test]
    fn parses_apt_version_ranges() {
        assert_eq!(
            parse_version_range(">= 1.2, < 2:0").unwrap(),
            vec![
                AptConstraint {
                    operator: ">=".into(),
                    version: "1.2".into()
                },
                AptConstraint {
                    operator: "<".into(),
                    version: "2:0".into()
                },
            ]
        );
        assert!(parse_version_range(">= 1.0,, < 2.0").is_err());
        assert!(parse_version_range(">= 1.0 bad").is_err());
    }

    #[test]
    fn validates_containers() {
        let mut config = MachineConfig {
            version: 1,
            ssh: SshConfig {
                destination: "host".into(),
            },
            trees: vec![TreeConfig {
                source: "home".into(),
                target: "~".into(),
                privileged: false,
            }],
            metadata: Vec::new(),
            scripts: Vec::new(),
            udev_rules: Vec::new(),
            apt_packages: Vec::new(),
            containers: vec![ContainerConfig {
                name: "web".into(),
                image: "nginx:1.27".into(),
                restart: "unless-stopped".into(),
                ports: vec!["8080:80".into()],
                environment: vec!["APP_ENV=production".into()],
            }],
        };
        assert!(config.validate().is_ok());
        config.containers[0].ports[0] = "8080:bad".into();
        assert!(config.validate().is_err());
        config.containers[0].ports[0] = "8080:80".into();
        config.containers[0].restart = "always-on".into();
        assert!(config.validate().is_err());
    }
}

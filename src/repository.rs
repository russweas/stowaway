use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};

use crate::config::{MachineConfig, parse_mode};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct Repository {
    root: PathBuf,
}

#[derive(Debug)]
pub struct Deployment {
    pub name: String,
    pub config: MachineConfig,
    pub files: Vec<DeploymentFile>,
    pub scripts: Vec<DeploymentScript>,
    pub digest: String,
    pub directory: PathBuf,
    pub apt_packages: Vec<LockedAptPackage>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedAptPackage {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AptLock {
    version: u32,
    #[serde(default)]
    packages: Vec<LockedAptPackage>,
}

#[derive(Debug)]
pub struct DeploymentFile {
    pub source: PathBuf,
    pub target: String,
    pub privileged: bool,
    pub mode: u32,
    pub contents: Vec<u8>,
}

#[derive(Debug)]
pub struct DeploymentScript {
    pub source: PathBuf,
    pub privileged: bool,
    pub timeout_seconds: u64,
    pub contents: Vec<u8>,
}

impl Repository {
    pub fn open(root: PathBuf) -> Result<Self> {
        let root = root
            .canonicalize()
            .with_context(|| format!("repository does not exist: {}", root.display()))?;
        ensure!(
            root.is_dir(),
            "repository is not a directory: {}",
            root.display()
        );
        Ok(Self { root })
    }

    pub fn machine_names(&self) -> Result<Vec<String>> {
        let machines = self.root.join("machines");
        let entries = fs::read_dir(&machines)
            .with_context(|| format!("could not read {}", machines.display()))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.file_type()?.is_dir() && entry.path().join("machine.toml").is_file() {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    pub fn require_clean_worktree(&self) -> Result<()> {
        let output = Command::new("git")
            .args(["status", "--porcelain=v1", "--untracked-files=normal"])
            .current_dir(&self.root)
            .output()
            .context("could not start git to inspect the repository")?;
        ensure!(
            output.status.success(),
            "repository is not a Git worktree: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        ensure!(
            output.stdout.is_empty(),
            "Git worktree has local changes; commit or stash them before continuing"
        );
        Ok(())
    }

    pub fn load_machine(&self, name: &str) -> Result<Deployment> {
        validate_machine_name(name)?;
        let directory = self.root.join("machines").join(name);
        let canonical_directory = directory
            .canonicalize()
            .with_context(|| format!("machine does not exist: {name}"))?;
        let machines_root = self.root.join("machines").canonicalize()?;
        ensure!(
            canonical_directory.starts_with(&machines_root),
            "machine directory escapes the repository: {name}"
        );
        let config = MachineConfig::parse(&directory.join("machine.toml"))?;
        let apt_packages = load_apt_lock(&directory, &config)?;
        let metadata: BTreeMap<_, _> = config
            .metadata
            .iter()
            .map(|item| Ok((item.source.clone(), parse_mode(&item.mode)?)))
            .collect::<Result<_>>()?;
        let mut files = Vec::new();
        let mut targets = BTreeSet::new();

        for tree in &config.trees {
            let tree_root = directory.join(&tree.source);
            let tree_info = fs::symlink_metadata(&tree_root)
                .with_context(|| format!("tree source does not exist: {}", tree_root.display()))?;
            ensure!(
                tree_info.is_dir() && !tree_info.file_type().is_symlink(),
                "tree source is not a directory: {}",
                tree_root.display()
            );
            let canonical_tree = tree_root.canonicalize()?;
            ensure!(
                canonical_tree.starts_with(&canonical_directory),
                "tree source escapes the machine directory: {}",
                tree.source.display()
            );
            collect_files(&tree_root, &tree_root, &mut |relative, path, fs_mode| {
                let source = tree.source.join(relative);
                let target = join_remote(&tree.target, relative);
                ensure!(
                    targets.insert(target.clone()),
                    "duplicate remote target {target}"
                );
                let mode = metadata
                    .get(&source)
                    .copied()
                    .unwrap_or(if fs_mode & 0o111 != 0 { 0o755 } else { 0o644 });
                files.push(DeploymentFile {
                    source,
                    target,
                    privileged: tree.privileged,
                    mode,
                    contents: fs::read(path)
                        .with_context(|| format!("could not read {}", path.display()))?,
                });
                Ok(())
            })?;
        }

        for source in metadata.keys() {
            ensure!(
                files.iter().any(|file| &file.source == source),
                "metadata references an unmanaged file: {}",
                source.display()
            );
        }
        let mut scripts = Vec::new();
        for script in &config.scripts {
            let path = directory.join(&script.path);
            let canonical_script = path
                .canonicalize()
                .with_context(|| format!("script does not exist: {}", path.display()))?;
            ensure!(
                canonical_script.starts_with(&canonical_directory),
                "script escapes the machine directory: {}",
                script.path.display()
            );
            let info = fs::symlink_metadata(&path)
                .with_context(|| format!("script does not exist: {}", path.display()))?;
            ensure!(
                info.file_type().is_file(),
                "script must be a regular file: {}",
                path.display()
            );
            scripts.push(DeploymentScript {
                source: script.path.clone(),
                privileged: script.privileged,
                timeout_seconds: script.timeout_seconds,
                contents: fs::read(&path)
                    .with_context(|| format!("could not read {}", path.display()))?,
            });
        }

        for rule in &config.udev_rules {
            let path = directory.join(&rule.source);
            let canonical_rule = path
                .canonicalize()
                .with_context(|| format!("udev rule does not exist: {}", path.display()))?;
            ensure!(
                canonical_rule.starts_with(&canonical_directory),
                "udev rule escapes the machine directory: {}",
                rule.source.display()
            );
            let info = fs::symlink_metadata(&path)
                .with_context(|| format!("udev rule does not exist: {}", path.display()))?;
            ensure!(
                info.file_type().is_file(),
                "udev rule must be a regular file: {}",
                path.display()
            );
            let target = format!("/etc/udev/rules.d/{}", rule.name);
            ensure!(
                targets.insert(target.clone()),
                "duplicate remote target {target}"
            );
            files.push(DeploymentFile {
                source: rule.source.clone(),
                target,
                privileged: true,
                mode: 0o644,
                contents: fs::read(&path)
                    .with_context(|| format!("could not read {}", path.display()))?,
            });
        }

        files.sort_by(|a, b| a.target.cmp(&b.target));
        let digest = deployment_digest(&config, &files, &directory)?;
        Ok(Deployment {
            name: name.to_owned(),
            config,
            files,
            scripts,
            digest,
            directory,
            apt_packages,
        })
    }

    pub fn machine_config(&self, name: &str) -> Result<(MachineConfig, PathBuf)> {
        validate_machine_name(name)?;
        let directory = self.root.join("machines").join(name);
        ensure!(directory.is_dir(), "machine does not exist: {name}");
        Ok((
            MachineConfig::parse(&directory.join("machine.toml"))?,
            directory,
        ))
    }

    pub fn git_commit(&self) -> Result<String> {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&self.root)
            .output()
            .context("could not start git to identify the current commit")?;
        ensure!(
            output.status.success(),
            "could not identify current Git commit: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }
}

fn load_apt_lock(directory: &Path, config: &MachineConfig) -> Result<Vec<LockedAptPackage>> {
    let path = directory.join("machine.lock");
    if config.apt_packages.is_empty() {
        ensure!(
            !path.exists(),
            "machine.lock contains no declared APT packages"
        );
        return Ok(Vec::new());
    }
    ensure!(
        path.is_file(),
        "missing {}; run `stowaway lock` to create it",
        path.display()
    );
    let source =
        fs::read_to_string(&path).with_context(|| format!("could not read {}", path.display()))?;
    let lock: AptLock =
        toml::from_str(&source).with_context(|| format!("invalid lock file {}", path.display()))?;
    ensure!(
        lock.version == 1,
        "unsupported APT lock version {}",
        lock.version
    );
    ensure!(
        lock.packages.len() == config.apt_packages.len(),
        "APT lock does not match machine.toml; run `stowaway lock`"
    );
    for (declared, locked) in config.apt_packages.iter().zip(&lock.packages) {
        ensure!(
            declared.name == locked.name,
            "APT lock package order differs; run `stowaway lock`"
        );
        ensure!(
            !locked.version.is_empty(),
            "APT lock contains an empty version"
        );
        for constraint in declared.constraints()? {
            let status = Command::new("dpkg")
                .args([
                    "--compare-versions",
                    &locked.version,
                    &constraint.operator,
                    &constraint.version,
                ])
                .status()
                .context("could not run dpkg to validate the APT lock")?;
            ensure!(
                status.success(),
                "locked APT version {} does not satisfy {} {} for {}",
                locked.version,
                constraint.operator,
                constraint.version,
                declared.name
            );
        }
    }
    Ok(lock.packages)
}

fn collect_files(
    root: &Path,
    directory: &Path,
    callback: &mut impl FnMut(&Path, &Path, u32) -> Result<()>,
) -> Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let info = fs::symlink_metadata(&path)?;
        if info.file_type().is_symlink() {
            bail!("source symlinks are not supported: {}", path.display());
        } else if info.is_dir() {
            collect_files(root, &path, callback)?;
        } else if info.is_file() {
            callback(path.strip_prefix(root)?, &path, info.permissions().mode())?;
        } else {
            bail!("special files are not supported: {}", path.display());
        }
    }
    Ok(())
}

fn deployment_digest(
    config: &MachineConfig,
    files: &[DeploymentFile],
    directory: &Path,
) -> Result<String> {
    let mut hash = Sha256::new();
    hash.update(b"stowaway-deployment-v1\0");
    hash_field(&mut hash, config.ssh.destination.as_bytes());
    for package in &config.apt_packages {
        hash_field(&mut hash, package.name.as_bytes());
        hash_field(&mut hash, package.version.as_bytes());
    }
    for file in files {
        hash_field(&mut hash, file.source.as_os_str().as_encoded_bytes());
        hash_field(&mut hash, file.target.as_bytes());
        hash.update([u8::from(file.privileged)]);
        hash.update(file.mode.to_be_bytes());
        hash_field(&mut hash, &file.contents);
    }
    for script in &config.scripts {
        hash_field(&mut hash, script.path.as_os_str().as_encoded_bytes());
        hash.update([u8::from(script.privileged)]);
        hash.update(script.timeout_seconds.to_be_bytes());
        hash_field(&mut hash, &fs::read(directory.join(&script.path))?);
    }
    Ok(hex::encode(hash.finalize()))
}

fn hash_field(hash: &mut Sha256, value: &[u8]) {
    hash.update((value.len() as u64).to_be_bytes());
    hash.update(value);
}

fn join_remote(root: &str, relative: &Path) -> String {
    let relative = relative.to_string_lossy();
    if root == "/" {
        format!("/{relative}")
    } else {
        format!("{}/{relative}", root.trim_end_matches('/'))
    }
}

fn validate_machine_name(name: &str) -> Result<()> {
    ensure!(!name.is_empty(), "machine name cannot be empty");
    ensure!(
        name.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "invalid machine name: {name}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        let machine = temp.path().join("machines/test");
        fs::create_dir_all(machine.join("home/.config/app")).unwrap();
        fs::create_dir_all(machine.join("root/etc/network/interfaces.d")).unwrap();
        fs::create_dir_all(machine.join("udev")).unwrap();
        fs::write(machine.join("home/.config/app/config"), "value=true\n").unwrap();
        fs::write(
            machine.join("root/etc/network/interfaces.d/eth0"),
            "auto eth0\n",
        )
        .unwrap();
        fs::write(
            machine.join("udev/80-example.rules"),
            "SUBSYSTEM==\"block\", ENV{ID_FS_TYPE}==\"ext4\", TAG+=\"stowaway\"\n",
        )
        .unwrap();
        let mut script = fs::File::create(machine.join("setup.sh")).unwrap();
        writeln!(script, "#!/bin/bash").unwrap();
        fs::write(
            machine.join("machine.toml"),
            r#"
version = 1
[ssh]
destination = "test-host"
[[trees]]
source = "home"
target = "~"
[[trees]]
source = "root"
target = "/"
privileged = true
[[metadata]]
source = "root/etc/network/interfaces.d/eth0"
mode = "0600"
[[scripts]]
path = "setup.sh"
[[udev_rules]]
source = "udev/80-example.rules"
name = "80-example.rules"
"#,
        )
        .unwrap();
        temp
    }

    fn git(temp: &tempfile::TempDir, arguments: &[&str]) {
        let status = Command::new("git")
            .args(arguments)
            .current_dir(temp.path())
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn committed_fixture() -> tempfile::TempDir {
        let temp = fixture();
        git(&temp, &["init", "--quiet"]);
        git(&temp, &["add", "."]);
        git(
            &temp,
            &[
                "-c",
                "user.name=Stowaway Tests",
                "-c",
                "user.email=stowaway@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
        );
        temp
    }

    #[test]
    fn loads_and_hashes_machine() {
        let temp = fixture();
        let repo = Repository::open(temp.path().to_owned()).unwrap();
        let first = repo.load_machine("test").unwrap();
        let second = repo.load_machine("test").unwrap();
        assert_eq!(first.files.len(), 3);
        assert_eq!(first.digest, second.digest);
        assert_eq!(
            first
                .files
                .iter()
                .find(|file| file.target == "/etc/network/interfaces.d/eth0")
                .unwrap()
                .mode,
            0o600
        );
        assert_eq!(
            first
                .files
                .iter()
                .find(|file| file.target == "/etc/udev/rules.d/80-example.rules")
                .unwrap()
                .mode,
            0o644
        );
    }

    #[test]
    fn content_changes_digest() {
        let temp = fixture();
        let repo = Repository::open(temp.path().to_owned()).unwrap();
        let before = repo.load_machine("test").unwrap().digest;
        fs::write(
            temp.path().join("machines/test/home/.config/app/config"),
            "value=false\n",
        )
        .unwrap();
        let after = repo.load_machine("test").unwrap().digest;
        assert_ne!(before, after);
    }

    #[test]
    fn accepts_clean_git_worktree() {
        let temp = committed_fixture();
        let repo = Repository::open(temp.path().to_owned()).unwrap();
        repo.require_clean_worktree().unwrap();
    }

    #[test]
    fn rejects_modified_and_untracked_files() {
        let temp = committed_fixture();
        let repo = Repository::open(temp.path().to_owned()).unwrap();

        fs::write(temp.path().join("untracked"), "new").unwrap();
        assert!(repo.require_clean_worktree().is_err());
        fs::remove_file(temp.path().join("untracked")).unwrap();

        fs::write(temp.path().join("machines/test/setup.sh"), "changed").unwrap();
        assert!(repo.require_clean_worktree().is_err());
    }

    #[test]
    fn rejects_non_git_directory() {
        let temp = fixture();
        let repo = Repository::open(temp.path().to_owned()).unwrap();
        assert!(repo.require_clean_worktree().is_err());
    }
}

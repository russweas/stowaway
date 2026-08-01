use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;

use anyhow::{Context, Result, bail};
use similar::{ChangeTag, TextDiff};

use crate::remote::{Remote, Snapshot};
use crate::repository::{Deployment, DeploymentFile};

pub fn diff(deployment: &Deployment) -> Result<bool> {
    Ok(preview(deployment)?.0)
}

fn preview(deployment: &Deployment) -> Result<(bool, Vec<bool>)> {
    let remote = Remote::new(&deployment.config.ssh.destination)?;
    let mut changed = false;
    let mut script_apply = Vec::with_capacity(deployment.scripts.len());

    for file in &deployment.files {
        let snapshot = remote.inspect(&file.target, file.privileged)?;
        if print_file_diff(file, snapshot) {
            changed = true;
        }
    }

    for script in &deployment.scripts {
        let check =
            remote.check_script(&script.contents, script.privileged, script.timeout_seconds)?;
        if check.needs_apply || !check.stdout.is_empty() || !check.stderr.is_empty() {
            println!(
                "script {}: {}",
                script.source.display(),
                if check.needs_apply {
                    "apply needed"
                } else {
                    "ok"
                }
            );
            print_script_output("stdout", &check.stdout);
            print_script_output("stderr", &check.stderr);
        }
        changed |= check.needs_apply;
        script_apply.push(check.needs_apply);
    }

    Ok((changed, script_apply))
}

fn print_script_output(stream: &str, output: &[u8]) {
    if output.is_empty() {
        return;
    }
    let output = String::from_utf8_lossy(output);
    for line in output.lines() {
        println!("  {stream}: {line}");
    }
}

pub fn status(deployment: &Deployment) -> Result<()> {
    let remote = Remote::new(&deployment.config.ssh.destination)?;
    let Some(state) = remote.state()? else {
        println!("{}: never deployed", deployment.name);
        return Ok(());
    };

    println!("machine:        {}", state.machine);
    println!("deployed at:    {}", state.deployed_at);
    println!("git commit:     {}", state.git_commit);
    println!("content digest: {}", state.content_digest);
    if state.machine != deployment.name {
        println!("warning: remote state belongs to a different machine configuration");
    } else if state.content_digest == deployment.digest {
        println!("local content:  matches deployed content");
    } else {
        println!("local content:  differs from deployed content");
    }
    Ok(())
}

pub fn apply(deployment: &Deployment, git_commit: &str, yes: bool, adopt: bool) -> Result<()> {
    let (changed, script_apply) = preview(deployment)?;
    let remote = Remote::new(&deployment.config.ssh.destination)?;
    let state = remote.state()?;
    if !changed
        && state.as_ref().is_some_and(|state| {
            state.machine == deployment.name && state.content_digest == deployment.digest
        })
    {
        println!("no changes");
        return Ok(());
    }
    confirm("Apply these changes?", yes)?;
    for (index, file) in deployment.files.iter().enumerate() {
        remote.stage_file(
            &deployment.digest,
            index,
            &file.contents,
            file.mode,
            file.privileged,
        )?;
    }
    let previous = state
        .as_ref()
        .filter(|state| state.machine == deployment.name)
        .map_or(&[][..], |state| state.managed_paths.as_slice());
    remote.apply_transaction(crate::remote::ApplyRequest {
        machine: &deployment.name,
        digest: &deployment.digest,
        git_commit,
        files: &deployment.files,
        scripts: &deployment.scripts,
        script_apply: &script_apply,
        previous,
        adopt,
    })?;
    println!("applied {} ({})", deployment.name, deployment.digest);
    Ok(())
}

pub fn pull(deployment: &Deployment, yes: bool) -> Result<()> {
    let remote = Remote::new(&deployment.config.ssh.destination)?;
    let mut snapshots = Vec::new();
    let mut changed = false;
    for file in &deployment.files {
        let snapshot = remote.inspect(&file.target, file.privileged)?;
        changed |= print_pull_diff(file, &snapshot);
        snapshots.push(snapshot);
    }
    if !changed {
        println!("no changes");
        return Ok(());
    }
    confirm("Import these changes into the local worktree?", yes)?;
    for (file, snapshot) in deployment.files.iter().zip(snapshots) {
        let local = deployment.directory.join(&file.source);
        match snapshot {
            Snapshot::Missing | Snapshot::DanglingLink => {
                fs::remove_file(&local)
                    .with_context(|| format!("could not remove {}", local.display()))?;
            }
            Snapshot::File { mode, contents } => {
                fs::write(&local, contents)
                    .with_context(|| format!("could not write {}", local.display()))?;
                let mut permissions = fs::metadata(&local)?.permissions();
                let current = permissions.mode();
                permissions.set_mode((current & !0o111) | (mode & 0o111));
                fs::set_permissions(&local, permissions)?;
            }
            other => bail!(
                "cannot import {} from remote {}",
                snapshot_name(&other),
                file.target
            ),
        }
    }
    println!(
        "pulled changes for {}; review and commit them when ready",
        deployment.name
    );
    Ok(())
}

fn print_pull_diff(local: &DeploymentFile, remote: &Snapshot) -> bool {
    match remote {
        Snapshot::File { mode, contents } if *mode == local.mode && *contents == local.contents => {
            false
        }
        Snapshot::File { mode, contents } => {
            println!("--- local:{}", local.source.display());
            println!("+++ remote:{}", local.target);
            if *mode != local.mode {
                println!("mode {:04o} -> {:04o}", local.mode, mode);
            }
            match (
                std::str::from_utf8(&local.contents),
                std::str::from_utf8(contents),
            ) {
                (Ok(old), Ok(new)) => print_text_diff(old, new),
                _ if *contents != local.contents => println!(
                    "binary content differs ({} local bytes, {} remote bytes)",
                    local.contents.len(),
                    contents.len()
                ),
                _ => {}
            }
            true
        }
        other => {
            println!(
                "{}: local file -> {}",
                local.source.display(),
                snapshot_name(other)
            );
            true
        }
    }
}

fn confirm(prompt: &str, yes: bool) -> Result<()> {
    if yes {
        return Ok(());
    }
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim(), "y" | "Y" | "yes" | "YES") {
        bail!("cancelled")
    }
    Ok(())
}

fn print_file_diff(expected: &DeploymentFile, actual: Snapshot) -> bool {
    match actual {
        Snapshot::File { mode, contents }
            if mode == expected.mode && contents == expected.contents =>
        {
            false
        }
        Snapshot::File { mode, contents } => {
            println!("--- remote:{}", expected.target);
            println!("+++ local:{}", expected.source.display());
            if mode != expected.mode {
                println!("mode {:04o} -> {:04o}", mode, expected.mode);
            }
            match (
                std::str::from_utf8(&contents),
                std::str::from_utf8(&expected.contents),
            ) {
                (Ok(old), Ok(new)) => print_text_diff(old, new),
                _ if contents != expected.contents => println!(
                    "binary content differs ({} remote bytes, {} local bytes)",
                    contents.len(),
                    expected.contents.len()
                ),
                _ => {}
            }
            true
        }
        other => {
            println!(
                "{}: {} -> file ({:04o}, {} bytes)",
                expected.target,
                snapshot_name(&other),
                expected.mode,
                expected.contents.len()
            );
            true
        }
    }
}

fn print_text_diff(old: &str, new: &str) {
    for change in TextDiff::from_lines(old, new).iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => '-',
            ChangeTag::Insert => '+',
            ChangeTag::Equal => ' ',
        };
        print!("{sign}{change}");
        if change.missing_newline() {
            println!();
        }
    }
}

fn snapshot_name(snapshot: &Snapshot) -> &'static str {
    match snapshot {
        Snapshot::Missing => "missing",
        Snapshot::DanglingLink => "dangling link",
        Snapshot::File { .. } => "file",
        Snapshot::Directory => "directory",
        Snapshot::Symlink => "symlink",
        Snapshot::Other => "special file",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn file() -> DeploymentFile {
        DeploymentFile {
            source: PathBuf::from("root/etc/example"),
            target: "/etc/example".into(),
            privileged: true,
            mode: 0o644,
            contents: b"new\n".to_vec(),
        }
    }

    #[test]
    fn identical_file_is_unchanged() {
        assert!(!print_file_diff(
            &file(),
            Snapshot::File {
                mode: 0o644,
                contents: b"new\n".to_vec()
            }
        ));
    }

    #[test]
    fn missing_file_is_changed() {
        assert!(print_file_diff(&file(), Snapshot::Missing));
    }

    #[test]
    fn pull_detects_content_and_mode_changes() {
        assert!(print_pull_diff(
            &file(),
            &Snapshot::File {
                mode: 0o755,
                contents: b"remote\n".to_vec(),
            }
        ));
        assert!(!print_pull_diff(
            &file(),
            &Snapshot::File {
                mode: 0o644,
                contents: b"new\n".to_vec(),
            }
        ));
    }
}

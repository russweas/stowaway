use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

const INSPECT_SCRIPT: &str = r#"
set -eu
path=$1
case "$path" in
  '~') path=$HOME ;;
  '~/'*) path=$HOME/${path#\~/} ;;
esac
run() {
  if [ "$2" = 1 ]; then
    shift 2
    sudo -n -- "$@"
  else
    shift 2
    "$@"
  fi
}
privileged=$2
if run x "$privileged" test -L "$path" && ! run x "$privileged" test -e "$path"; then
  printf 'STOWAWAY1\tDANGLING\t-\n'
elif ! run x "$privileged" test -e "$path"; then
  printf 'STOWAWAY1\tMISSING\t-\n'
elif run x "$privileged" test -f "$path"; then
  mode=$(run x "$privileged" stat -Lc '%a' -- "$path")
  printf 'STOWAWAY1\tFILE\t%s\n' "$mode"
  run x "$privileged" cat -- "$path"
elif run x "$privileged" test -d "$path"; then
  printf 'STOWAWAY1\tDIRECTORY\t-\n'
elif run x "$privileged" test -L "$path"; then
  printf 'STOWAWAY1\tSYMLINK\t-\n'
else
  printf 'STOWAWAY1\tOTHER\t-\n'
fi
"#;

const STATE_SCRIPT: &str = r#"
set -eu
state=/var/lib/stowaway/state.toml
if ! sudo -n -- test -f "$state"; then
  printf 'STOWAWAY1\tMISSING\t-\n'
else
  printf 'STOWAWAY1\tSTATE\t-\n'
  sudo -n -- cat -- "$state"
fi
"#;

#[derive(Debug, PartialEq, Eq)]
pub enum Snapshot {
    Missing,
    DanglingLink,
    File { mode: u32, contents: Vec<u8> },
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeploymentState {
    pub version: u32,
    pub machine: String,
    pub git_commit: String,
    pub content_digest: String,
    pub deployed_at: String,
    #[serde(default)]
    pub managed_paths: Vec<ManagedPath>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ManagedPath {
    pub path: String,
    pub privileged: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ScriptCheck {
    pub needs_apply: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub struct Remote<'a> {
    destination: &'a str,
    ssh_program: PathBuf,
}

pub struct ApplyRequest<'a> {
    pub machine: &'a str,
    pub digest: &'a str,
    pub git_commit: &'a str,
    pub files: &'a [crate::repository::DeploymentFile],
    pub scripts: &'a [crate::repository::DeploymentScript],
    pub script_apply: &'a [bool],
    pub previous: &'a [ManagedPath],
    pub adopt: bool,
}

impl<'a> Remote<'a> {
    pub fn new(destination: &'a str) -> Result<Self> {
        ensure!(
            !destination.starts_with('-'),
            "SSH destination cannot begin with '-'"
        );
        ensure!(
            !destination.contains(['\n', '\r', '\0']),
            "SSH destination contains an invalid character"
        );
        Ok(Self {
            destination,
            ssh_program: PathBuf::from("ssh"),
        })
    }

    #[cfg(test)]
    fn with_ssh_program(destination: &'a str, ssh_program: &std::path::Path) -> Result<Self> {
        let mut remote = Self::new(destination)?;
        remote.ssh_program = ssh_program.to_owned();
        Ok(remote)
    }

    pub fn inspect(&self, path: &str, privileged: bool) -> Result<Snapshot> {
        let command = format!(
            "bash -c {} -- {} {}",
            shell_quote(INSPECT_SCRIPT),
            shell_quote(path),
            if privileged { "1" } else { "0" }
        );
        let output = Command::new(&self.ssh_program)
            .arg("--")
            .arg(self.destination)
            .arg(command)
            .output()
            .with_context(|| format!("could not start ssh for {}", self.destination))?;
        ensure_success(&output, self.destination)?;
        parse_snapshot(&output.stdout)
            .with_context(|| format!("invalid response while inspecting {path}"))
    }

    pub fn state(&self) -> Result<Option<DeploymentState>> {
        let command = format!("bash -c {}", shell_quote(STATE_SCRIPT));
        let output = self.execute(command)?;
        let newline = output
            .iter()
            .position(|byte| *byte == b'\n')
            .context("missing state protocol header")?;
        let header = &output[..newline];
        match header {
            b"STOWAWAY1\tMISSING\t-" => Ok(None),
            b"STOWAWAY1\tSTATE\t-" => {
                let body = std::str::from_utf8(&output[newline + 1..])?;
                let state: DeploymentState =
                    toml::from_str(body).context("invalid remote deployment state")?;
                ensure!(
                    state.version == 1,
                    "unsupported remote state version {}",
                    state.version
                );
                Ok(Some(state))
            }
            _ => bail!("unsupported state protocol response"),
        }
    }

    pub fn check_script(
        &self,
        script: &[u8],
        privileged: bool,
        timeout_seconds: u64,
    ) -> Result<ScriptCheck> {
        let timeout = format!("{timeout_seconds}s");
        let command = if privileged {
            format!(
                "sudo -n -- timeout --signal=TERM --kill-after=5s {} bash -s -- check",
                shell_quote(&timeout)
            )
        } else {
            format!(
                "timeout --signal=TERM --kill-after=5s {} bash -s -- check",
                shell_quote(&timeout)
            )
        };
        let mut child = Command::new(&self.ssh_program)
            .arg("--")
            .arg(self.destination)
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("could not start ssh for {}", self.destination))?;
        child
            .stdin
            .take()
            .context("could not open ssh standard input")?
            .write_all(script)
            .context("could not send script over ssh")?;
        let output = child
            .wait_with_output()
            .context("could not wait for script check over ssh")?;
        parse_script_check(output, self.destination)
    }

    pub fn stage_file(
        &self,
        digest: &str,
        index: usize,
        contents: &[u8],
        mode: u32,
        privileged: bool,
    ) -> Result<()> {
        let root = if privileged {
            "/var/lib/stowaway/store"
        } else {
            "~/.local/share/stowaway/store"
        };
        let path = format!("{root}/{digest}/{index}");
        let expanded = shell_path(&path);
        let parent = path
            .rsplit_once('/')
            .map_or_else(|| shell_quote("."), |(parent, _)| shell_path(parent));
        let prefix = if privileged { "sudo -n -- " } else { "" };
        let command = format!(
            "set -eu; {prefix}mkdir -p -- {}; {prefix}tee -- {} >/dev/null; {prefix}chmod {:04o} -- {}",
            parent, expanded, mode, expanded
        );
        self.execute_with_input(command, contents).map(|_| ())
    }

    pub fn apply_transaction(&self, request: ApplyRequest<'_>) -> Result<()> {
        let ApplyRequest {
            machine,
            digest,
            git_commit,
            files,
            scripts,
            script_apply,
            previous,
            adopt,
        } = request;
        let mut script = String::from(
            "set -eu\nrollback_dir=$(mktemp -d)\ncommitted=0\nrollback() { code=$?; if [ \"$committed\" = 0 ]; then while IFS='\t' read -r flag priv path backup; do run=; [ \"$priv\" = 1 ] && run='sudo -n --'; if [ \"$flag\" = present ]; then $run rm -rf -- \"$path\"; $run cp -a -- \"$backup\" \"$path\"; else $run rm -rf -- \"$path\"; fi; done < \"$rollback_dir/journal\"; fi; rm -rf -- \"$rollback_dir\"; exit $code; }\ntrap rollback EXIT HUP INT TERM\n: > \"$rollback_dir/journal\"\n",
        );
        let old: std::collections::BTreeSet<_> = previous.iter().map(|p| p.path.as_str()).collect();
        for (index, file) in files.iter().enumerate() {
            let target = shell_path(&file.target);
            let priv_flag = u8::from(file.privileged);
            let managed = old.contains(file.target.as_str());
            script.push_str(&format!(
                "target={target}; run=; [ {priv_flag} = 1 ] && run='sudo -n --'\n"
            ));
            if managed {
                script.push_str("if $run test -e \"$target\" || $run test -L \"$target\"; then valid=0; if $run test -L \"$target\"; then link=$($run readlink -- \"$target\"); case \"$link\" in /var/lib/stowaway/store/*|\"$HOME\"/.local/share/stowaway/store/*) valid=1 ;; esac; fi; if [ \"$valid\" = 0 ]; then echo \"managed target was replaced: $target (use --adopt to take it over)\" >&2; [ " );
                script.push_str(if adopt { "1" } else { "0" });
                script.push_str(" = 1 ] || exit 1; stamp=$(date -u +%Y%m%dT%H%M%SZ); $run cp -a -- \"$target\" \"$target.stowaway-backup-$stamp\"; fi; fi\n");
            }
            if !managed && !adopt {
                script.push_str("if $run test -e \"$target\" || $run test -L \"$target\"; then echo \"unmanaged collision: $target\" >&2; exit 1; fi\n");
            }
            if !managed && adopt {
                script.push_str("if $run test -e \"$target\" || $run test -L \"$target\"; then stamp=$(date -u +%Y%m%dT%H%M%SZ); $run cp -a -- \"$target\" \"$target.stowaway-backup-$stamp\"; fi\n");
            }
            script.push_str(&format!("$run mkdir -p -- $(dirname -- \"$target\")\nif $run test -e \"$target\" || $run test -L \"$target\"; then backup=\"$rollback_dir/{index}\"; $run cp -a -- \"$target\" \"$backup\"; printf 'present\\t{priv_flag}\\t%s\\t%s\\n' \"$target\" \"$backup\" >> \"$rollback_dir/journal\"; else printf 'missing\\t{priv_flag}\\t%s\\t-\\n' \"$target\" >> \"$rollback_dir/journal\"; fi\n$run rm -rf -- \"$target\"\n"));
            let store = if file.privileged {
                shell_quote(&format!("/var/lib/stowaway/store/{digest}/{index}"))
            } else {
                format!("\"$HOME/.local/share/stowaway/store/{digest}/{index}\"")
            };
            script.push_str(&format!("$run ln -s -- {store} \"$target\"\n"));
        }
        let new_paths: std::collections::BTreeSet<_> =
            files.iter().map(|f| f.target.as_str()).collect();
        for (index, path) in previous
            .iter()
            .filter(|p| !new_paths.contains(p.path.as_str()))
            .enumerate()
        {
            let target = shell_path(&path.path);
            let run = if path.privileged { "sudo -n -- " } else { "" };
            script.push_str(&format!("target={target}; if {run} test -L \"$target\"; then link=$({run}readlink -- \"$target\"); case \"$link\" in /var/lib/stowaway/store/*|\"$HOME\"/.local/share/stowaway/store/*) backup=\"$rollback_dir/old-{index}\"; {run}cp -a -- \"$target\" \"$backup\"; printf 'present\\t{}\\t%s\\t%s\\n' \"$target\" \"$backup\" >> \"$rollback_dir/journal\"; {run}rm -- \"$target\" ;; esac; fi\n", u8::from(path.privileged)));
        }
        ensure!(
            scripts.len() == script_apply.len(),
            "internal script plan length mismatch"
        );
        for script_item in scripts
            .iter()
            .zip(script_apply)
            .filter_map(|(script, apply)| apply.then_some(script))
        {
            let timeout = format!("{}s", script_item.timeout_seconds);
            let run = if script_item.privileged {
                "sudo -n -- "
            } else {
                ""
            };
            script.push_str(&format!("{run}timeout --signal=TERM --kill-after=5s {} bash -s -- apply <<'STOWAWAY_SCRIPT'\n{}\nSTOWAWAY_SCRIPT\n", shell_quote(&timeout), String::from_utf8_lossy(&script_item.contents)));
        }
        let managed_paths = files
            .iter()
            .map(|file| ManagedPath {
                path: file.target.clone(),
                privileged: file.privileged,
            })
            .collect::<Vec<_>>();
        let state = toml::to_string(&StateWrite {
            version: 1,
            machine,
            git_commit,
            content_digest: digest,
        })?;
        let paths = toml::to_string(&StatePaths {
            managed_paths: &managed_paths,
        })?;
        script.push_str(&format!("state_tmp=$(mktemp); cat > \"$state_tmp\" <<'STOWAWAY_STATE'\n{state}STOWAWAY_STATE\nprintf 'deployed_at = \"%s\"\\n' \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\" >> \"$state_tmp\"\ncat >> \"$state_tmp\" <<'STOWAWAY_PATHS'\n{paths}STOWAWAY_PATHS\nsudo -n -- mkdir -p /var/lib/stowaway; sudo -n -- install -m 0644 \"$state_tmp\" /var/lib/stowaway/state.toml; committed=1; rm -f \"$state_tmp\"\n"));
        self.execute(format!("bash -c {}", shell_quote(&script)))
            .map(|_| ())
    }

    fn execute(&self, command: String) -> Result<Vec<u8>> {
        let output = Command::new(&self.ssh_program)
            .arg("--")
            .arg(self.destination)
            .arg(command)
            .output()
            .with_context(|| format!("could not start ssh for {}", self.destination))?;
        ensure_success(&output, self.destination)?;
        Ok(output.stdout)
    }

    fn execute_with_input(&self, command: String, input: &[u8]) -> Result<Vec<u8>> {
        let mut child = Command::new(&self.ssh_program)
            .arg("--")
            .arg(self.destination)
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("could not start ssh for {}", self.destination))?;
        child
            .stdin
            .take()
            .context("could not open ssh standard input")?
            .write_all(input)?;
        let output = child.wait_with_output()?;
        ensure_success(&output, self.destination)?;
        Ok(output.stdout)
    }
}

#[derive(Serialize)]
struct StateWrite<'a> {
    version: u32,
    machine: &'a str,
    git_commit: &'a str,
    content_digest: &'a str,
}

#[derive(Serialize)]
struct StatePaths<'a> {
    managed_paths: &'a [ManagedPath],
}

fn shell_path(path: &str) -> String {
    if path == "~" {
        "\"$HOME\"".into()
    } else if let Some(rest) = path.strip_prefix("~/") {
        format!("\"$HOME\"/{}", shell_quote(rest))
    } else {
        shell_quote(path)
    }
}

fn parse_script_check(output: Output, destination: &str) -> Result<ScriptCheck> {
    let code = output.status.code();
    if matches!(code, Some(0 | 10)) {
        return Ok(ScriptCheck {
            needs_apply: code == Some(10),
            stdout: output.stdout,
            stderr: output.stderr,
        });
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if code == Some(124) {
        bail!("script check on {destination} timed out: {}", stderr.trim());
    }
    bail!(
        "script check on {destination} failed with {}: {}",
        output.status,
        stderr.trim()
    )
}

fn ensure_success(output: &Output, destination: &str) -> Result<()> {
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "ssh to {destination} failed with {}: {}",
        output.status,
        stderr.trim()
    )
}

fn parse_snapshot(bytes: &[u8]) -> Result<Snapshot> {
    let newline = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing protocol header"))?;
    let header = std::str::from_utf8(&bytes[..newline])?;
    let mut fields = header.split('\t');
    ensure!(
        fields.next() == Some("STOWAWAY1"),
        "unsupported protocol response"
    );
    let kind = fields.next().context("missing response type")?;
    let mode = fields.next().context("missing response mode")?;
    ensure!(fields.next().is_none(), "unexpected protocol field");
    let contents = &bytes[newline + 1..];

    Ok(match kind {
        "MISSING" => Snapshot::Missing,
        "DANGLING" => Snapshot::DanglingLink,
        "FILE" => Snapshot::File {
            mode: u32::from_str_radix(mode, 8).context("invalid remote file mode")?,
            contents: contents.to_vec(),
        },
        "DIRECTORY" => Snapshot::Directory,
        "SYMLINK" => Snapshot::Symlink,
        "OTHER" => Snapshot::Other,
        _ => bail!("unknown response type {kind}"),
    })
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;

    fn output(code: i32, stdout: &[u8], stderr: &[u8]) -> Output {
        Output {
            status: std::process::ExitStatus::from_raw(code << 8),
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
        }
    }

    fn fake_ssh(body: &str) -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("ssh");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
        (directory, path)
    }

    #[test]
    fn parses_file_with_binary_body() {
        assert_eq!(
            parse_snapshot(b"STOWAWAY1\tFILE\t600\nhello\0world").unwrap(),
            Snapshot::File {
                mode: 0o600,
                contents: b"hello\0world".to_vec()
            }
        );
    }

    #[test]
    fn parses_missing_path() {
        assert_eq!(
            parse_snapshot(b"STOWAWAY1\tMISSING\t-\n").unwrap(),
            Snapshot::Missing
        );
    }

    #[test]
    fn quotes_shell_values() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn parses_deployment_state() {
        let state: DeploymentState = toml::from_str(
            r#"
version = 1
machine = "sim-01"
git_commit = "abc123"
content_digest = "def456"
deployed_at = "2026-07-31T12:00:00Z"
[[managed_paths]]
path = "/etc/example"
privileged = true
"#,
        )
        .unwrap();
        assert_eq!(state.machine, "sim-01");
        assert_eq!(state.content_digest, "def456");
        assert_eq!(state.managed_paths[0].path, "/etc/example");
    }

    #[test]
    fn parses_script_check_contract() {
        assert_eq!(
            parse_script_check(output(0, b"ready\n", b""), "host").unwrap(),
            ScriptCheck {
                needs_apply: false,
                stdout: b"ready\n".to_vec(),
                stderr: Vec::new(),
            }
        );
        assert!(
            parse_script_check(output(10, b"", b"needed\n"), "host")
                .unwrap()
                .needs_apply
        );
        assert!(parse_script_check(output(1, b"", b"broken\n"), "host").is_err());
        assert!(parse_script_check(output(124, b"", b""), "host").is_err());
    }

    #[test]
    fn builds_safe_home_path_expressions() {
        assert_eq!(shell_path("~/.config/app"), "\"$HOME\"/'.config/app'");
        assert_eq!(shell_path("/etc/app"), "'/etc/app'");
    }

    #[test]
    fn fake_ssh_rejects_protocol_errors() {
        let (_directory, ssh) = fake_ssh("printf 'NOT-STOWAWAY\\n'");
        let remote = Remote::with_ssh_program("fake-host", &ssh).unwrap();

        let error = remote.inspect("~/.config/app", false).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("invalid response while inspecting")
        );
        assert!(format!("{error:#}").contains("unsupported protocol response"));
    }

    #[test]
    fn fake_ssh_reports_script_timeouts() {
        let (_directory, ssh) = fake_ssh("echo 'deadline exceeded' >&2; exit 124");
        let remote = Remote::with_ssh_program("fake-host", &ssh).unwrap();

        let error = remote.check_script(b"exit 0\n", false, 1).unwrap_err();

        assert!(error.to_string().contains("timed out"));
        assert!(error.to_string().contains("deadline exceeded"));
    }

    #[test]
    fn fake_ssh_reports_connection_failures() {
        let (_directory, ssh) = fake_ssh("echo 'connection refused' >&2; exit 255");
        let remote = Remote::with_ssh_program("fake-host", &ssh).unwrap();

        let error = remote.inspect("/etc/app", true).unwrap_err();

        assert!(error.to_string().contains("ssh to fake-host failed"));
        assert!(error.to_string().contains("connection refused"));
    }
}

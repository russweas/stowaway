mod cli;
mod commands;
mod config;
mod remote;
mod repository;

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Command};
use crate::repository::Repository;

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let repository = Repository::open(cli.repo)?;
    match cli.command {
        Command::Validate { machine } => {
            if let Some(machine) = machine {
                let deployment = repository.load_machine(&machine)?;
                println!(
                    "{}: valid for {} ({} files, {})",
                    deployment.name,
                    deployment.config.ssh.destination,
                    deployment.files.len(),
                    deployment.digest
                );
            } else {
                let machines = repository.machine_names()?;
                for machine in &machines {
                    let deployment = repository.load_machine(machine)?;
                    println!(
                        "{}: valid for {} ({} files, {})",
                        deployment.name,
                        deployment.config.ssh.destination,
                        deployment.files.len(),
                        deployment.digest
                    );
                }
                println!("validated {} machine(s)", machines.len());
            }
        }
        Command::Diff { machine } => {
            let deployment = repository.load_machine(&machine)?;
            let changed = commands::diff(&deployment)?;
            if !changed {
                println!("no changes");
            }
        }
        Command::Status { machine } => {
            let deployment = repository.load_machine(&machine)?;
            commands::status(&deployment)?;
        }
        Command::Apply {
            machine,
            yes,
            adopt,
        } => {
            repository.require_clean_worktree()?;
            let deployment = repository.load_machine(&machine)?;
            let commit = repository.git_commit()?;
            commands::apply(&deployment, &commit, yes, adopt)?;
        }
        Command::Pull { machine, yes } => {
            repository.require_clean_worktree()?;
            let deployment = repository.load_machine(&machine)?;
            commands::pull(&deployment, yes)?;
        }
        Command::Devices { machine, subsystem } => {
            let deployment = repository.load_machine(&machine)?;
            commands::devices(&deployment, subsystem.as_deref())?;
        }
    }

    Ok(())
}

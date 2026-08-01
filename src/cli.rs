use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(version, about)]
pub struct Cli {
    /// Root of the Stowaway repository.
    #[arg(long, global = true, default_value = ".")]
    pub repo: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Validate one machine, or every machine when omitted.
    Validate { machine: Option<String> },
    /// Preview differences between local configuration and a host.
    Diff { machine: String },
    /// Preview and deploy a machine configuration.
    Apply {
        machine: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
        /// Back up and take ownership of unmanaged target files.
        #[arg(long)]
        adopt: bool,
    },
    /// Preview and import changes from a managed host.
    Pull {
        machine: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Show the last deployment recorded on a host.
    Status { machine: String },
}

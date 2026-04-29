use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "TechToolKit")]
#[command(
    version,
    about = "Touch-friendly maintenance toolkit with optional automation mode"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Run automation commands without starting the GUI.
    Automation {
        #[command(subcommand)]
        command: Option<AutomationCommand>,
    },
}

#[derive(Debug, Subcommand)]
pub enum AutomationCommand {
    /// Start a headless automation session.
    Run,
}

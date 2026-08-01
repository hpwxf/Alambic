//! Entry point of the Ornament & Crime simulator.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use oc_sim::tui::{Tui, replay_headless};

#[derive(Debug, Parser)]
#[command(
    name = "oc-sim",
    about = "Run the Ornament & Crime firmware without hardware",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Open the interactive terminal interface (the default).
    Run {
        /// Record every input and write the scenario here on exit.
        #[arg(long, value_name = "FILE")]
        record: Option<PathBuf>,
    },
    /// Replay a scenario without a terminal and print the final state.
    Replay {
        /// Scenario file to replay.
        scenario: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Run { record: None }) {
        Command::Run { record } => {
            let mut tui = Tui::new();
            if let Some(path) = record {
                tui.record_to(path);
            }
            tui.run()
        }
        Command::Replay { scenario } => {
            print!("{}", replay_headless(&scenario)?);
            Ok(())
        }
    }
}

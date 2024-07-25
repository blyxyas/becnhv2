#![feature(let_chains)]

use std::fs;

use anyhow::Result;
use clap::{Parser, Subcommand};
use git2::{ErrorCode, Repository};
use log::{debug, error, info, warn};

mod ops;

const SETUP_COMPLETED_LOCK: &str = ".setup-completed__";
const CLIPPY_PATH: &str = ".clippy__";
const RUST_TREE_PATH: &str = ".rust-upstream__";

#[derive(Parser, Debug)]
struct Cli {
    #[arg(short, long, default_value = "true")]
    master: bool,
    #[arg(long, default_value = "false")]
    no_pr: bool,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    PR {
        number: usize,
    },
    Setup {
        /// Skip asking if proceeding, assume Y is always the answer
        #[arg(short)]
        yes: bool,
    },
}

fn main() -> Result<()> {
    env_logger::init();
    info!("Parsing arguments");
    let args = Cli::parse();

    debug!("Arguments passed: {:#?}", &args);

    // Let's check our setup
    // Checking if user has run `becnhv2 setup`

    if let Commands::Setup { yes } = args.command {
        warn!("Command is setup, creating and pulling rust (upstream) and rust-clippy");

        match ops::setup(yes) {
            Ok(_) => {
                debug!("Setup succesful");
                println!("Setup done, now you can benchmark any Clippy PR!");
                return Ok(());
            }
            Err(e) => {
                error!("Error encountered");
                anyhow::bail!(e)
            }
        };
    }

    info!("Checking if user has run `becnhv2 setup`");
    if fs::metadata(SETUP_COMPLETED_LOCK).is_err() {
        error!("File `.setup-completed__` not found, and thus, setup has not been completed");
        anyhow::bail!("Setup lock not found, you should run `becnhv2 setup`");
    };

    Ok(())
}

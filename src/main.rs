#![feature(let_chains)]
use std::{fs, path::Path};

use anyhow::Result;
use clap::{Parser, Subcommand};
use git2::{build::CheckoutBuilder, Object, Repository};
use human_panic::setup_panic;
use log::{debug, error, info, warn};

mod get_prs;
mod setup;

const SETUP_COMPLETED_LOCK: &str = ".setup-completed__";
const CLIPPY_PATH: &str = ".rust-clippy__";
const RUST_TREE_PATH: &str = ".rust-upstream__";
const RUSTC_PERF_PATH: &str = ".rustc-perf__";

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
        #[arg(long, short, default_value = "false")]
        master: bool,
    },
    Setup {
        /// Skip asking if proceeding, assume Y is always the answer
        #[arg(short)]
        yes: bool,
    },
}

fn main() -> Result<()> {
    env_logger::init();
    setup_panic!();
    info!("Parsing arguments");
    let args = Cli::parse();

    debug!("Arguments passed: {:#?}", &args);

    // Let's check our setup
    // Checking if user has run `becnhv2 setup`

    if let Commands::Setup { yes } = args.command {
        warn!("Command is setup, creating and pulling rust (upstream) and rust-clippy");

        match setup::setup(yes) {
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

    let rust_repo = Repository::open(RUST_TREE_PATH)?;
    let clippy_repo = Repository::open(CLIPPY_PATH)?;

    if let Commands::PR { number, master } = args.command {
        dbg!(get_prs::get_pr(number, &rust_repo, &clippy_repo, master))?;
    }

    Ok(())
}

pub(crate) fn checkout_to_ref<'a>(repo: &'a Repository, ref_name: &'a str) -> Result<Object<'a>> {
    let (object, reference) = repo.revparse_ext(ref_name)?;
    // As we are sure that reference points to a branch, we'll unwrap it
    let reference = reference.unwrap();

    repo.checkout_tree(&object, Some(CheckoutBuilder::default().force()))?;
    repo.set_head(reference.name().unwrap())?;

    Ok(object)
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}

pub(crate) fn pause() {
    use std::io::{stdin, stdout, Read, Write};

    let mut stdout = stdout();
    stdout.write_all(b"Press Enter to continue...").unwrap();
    stdout.flush().unwrap();
    stdin().read_exact(&mut [0]).unwrap();
}

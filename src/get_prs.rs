use std::{
    fs::{self, canonicalize, File},
    io::{self, copy, Read, Write},
    path::Path,
    process::{Child, Command, ExitStatus},
};

use anyhow::Result;
use curl::easy::Easy;
use datetime::{convenience::Today, ISO};
use git2::{self, build::CheckoutBuilder, Repository, StatusOptions};
use nightly2version::{RustVersion, ToVersion};
use regex::Regex;
use semver::Version;
use tar::Archive;

use crate::{checkout_to_ref, copy_dir_all, pause, CLIPPY_PATH, RUSTC_PERF_PATH, RUST_TREE_PATH};

use log::{debug, trace, warn};

pub(crate) fn get_pr(number: usize, clippy_repo: &Repository, master: bool) -> Result<()> {
    // Checkout PR
    debug!("Checking out PR");
    trace!("Cloning PR");
    clippy_repo.find_remote("origin")?.fetch(
        &[&format!("pull/{number}/head:current_pr")],
        None,
        None,
    )?;

    let branch_name = &format!("pull/{number}/headrefs/heads/current_pr");

    debug!("Remote PR found, changing to that branch");
    checkout_to_ref(clippy_repo, branch_name)?;

    debug!("Migrating that PR to tree");
    let mut easy = Easy::new();

    let rust_toolchain =
        &fs::read_to_string(Path::new(CLIPPY_PATH).join("rust-toolchain"))?[31..41];

    debug!("{}", &format!("{rust_toolchain}"));

    let mut dst = Vec::new();
    let download_path = format!("{RUST_TREE_PATH}.tar.xz");

    if !Path::new(&format!("{RUST_TREE_PATH}.tar.xz")).exists() {
        easy.url(&format!(
            "https://static.rust-lang.org/dist/{rust_toolchain}/rustc-nightly-src.tar.xz"
        ))?;

        {
            let mut transfer = easy.transfer();
            transfer
                .write_function(|data| {
                    dst.extend_from_slice(data);
                    Ok(data.len())
                })
                .unwrap();
            transfer.perform().unwrap();
        }
        let mut file = File::create(download_path.clone())?;
        dbg!(dst.len());
        file.write_all(dst.as_slice())?;
    }

    pause();

    let mut buf: Vec<u8> = Vec::new();

    debug!("Decompressing archive");
    // Command::new("tar").arg("-xf").arg(download_path).status()?;

    pause();

    // Build Rustc and cargo
    debug!("Building rustc");
    Command::new("./x.py")
        .args([
            "build",
            "rustc",
            "--set",
            "llvm.download-ci-llvm=true",
            "--stage",
            "2",
        ])
        .current_dir(RUST_TREE_PATH)
        .status()?;

    // Replace Clippy with PR Clippy
    let clippy_upstream_path = Path::new(RUST_TREE_PATH)
        .join("src")
        .join("tools")
        .join("clippy");

    if clippy_upstream_path.exists() {
        fs::remove_dir_all(&clippy_upstream_path)?;
    }

    copy_dir_all(CLIPPY_PATH, clippy_upstream_path)?;

    pause();

    debug!("Building Rust on sync-from-clippy");

    let mut rust_child = build_rust().unwrap();
    dbg!("1");

    debug!("Benchmarking artifact as PR-{number}");
dbg!("1");

    bench_artifact(&format!("PR-{number}"))?;

    // if master {
    //     debug!("");
    //     checkout_to_ref(rust_repo, "master")?;
    //     let mut rust_child = build_rust()?;
    //     let today = datetime::LocalDate::today();
    //     warn!("Benching master as `master-{}`", today.iso());
    //     bench_artifact(&mut rust_child, &format!("master-{}", today.iso()))?;
    // }

    // Cleanup
    // cleanup(rust_repo, clippy_repo, number)?;
    Ok(()) // APPLY A PATCH< BENCHMARK< RESTORE FILES IN PR< BENCHMARK AGAIN
}

fn migrate_pr_to_tree(// branch_name: &str,
    // rust_repo: &Repository,
    // clippy_repo: &Repository,
) -> Result<()> {
    // Now, let's get the according version of Rust's
    debug!("Getting necessary version, so that everything's correct");
    // We'll do it in a bit of brute force, honestly.

    debug!("Installing toolchain via rustup to get version");

    let mut version = read_toolchain_version()?;
    version.minor -= 1;
    let version_str = version.to_string();

    debug!("Synching Clippy with Rust@{version}");

    // if version.exists_in_stable() {
    // let pr_reference =
    //     checkout_to_ref(rust_repo, &version_str).expect("checkouts should always be to commits");
    // dbg!(&pr_reference);
    pause();
    // } else {
    // panic!();
    // }

    debug!("Creating branch");
    // let sync_branch = rust_repo.branch(
    //     "sync-from-clippy",
    //     &pr_reference
    //         .as_tag()
    //         .unwrap()
    //         .target()
    //         .unwrap()
    //         .as_commit()
    //         .unwrap(),
    //     true,
    // )?;

    trace!("Checkout into sync branch");

    // checkout_to_ref(rust_repo, "sync-from-clippy");
    // rust_repo.checkout_tree(head_commit.as_object(), None)?;
    // trace!("Set head to sync branch");
    // rust_repo.set_head(sync_branch.get().name().unwrap())?;

    debug!("Copying directory .clippy__ -> src/tools/clippy");

    let tools_clippy_path = Path::new(RUST_TREE_PATH)
        .join("src")
        .join("tools")
        .join("clippy");

    fs::remove_dir_all(&tools_clippy_path)?;
    copy_dir_all(CLIPPY_PATH, tools_clippy_path)?;

    // let clippy_subtree = Repository::open(
    //     canonicalize(Path::new(RUST_TREE_PATH))?
    //         .join("src")
    //         .join("tools")
    //         .join("clippy"),
    // )?;

    // debug!("Creating clippy-local, and fetching from it");

    // let mut clippy_subtree_remote = clippy_subtree.remote(
    //     "clippy-local",
    //     &canonicalize(Path::new(CLIPPY_PATH))?.to_string_lossy(),
    // )?;

    // clippy_subtree_remote.fetch(&["master"], None, None)?;

    // fs::copy(
    //     CLIPPY_PATH,
    //     Path::new(RUST_TREE_PATH).join("src/tools/clippy"),
    // )?;

    Ok(())
}

fn read_toolchain_version() -> Result<RustVersion> {
    let rust_toolchain =
        &fs::read_to_string(Path::new(CLIPPY_PATH).join("rust-toolchain"))?[23..41];

    debug!("Installing {rust_toolchain}");
    let rustup_toolchain_install = Command::new("rustup")
        .args(["install", rust_toolchain])
        .output()?
        .stdout;

    let s = match std::str::from_utf8(&rustup_toolchain_install) {
        Ok(v) => v,
        Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
    };

    let re = Regex::new(r"rustc (\d\.\d+\.\d)-nightly")?;

    let cap = &re.captures(s).unwrap().get(0).unwrap().as_str();

    let version_str = &cap[6..cap.len() - 8];

    return Ok(RustVersion::new(version_str));
}

fn cleanup(rust_repo: &Repository, clippy_repo: &Repository, pr: usize) -> Result<()> {
    // Move both repos to master

    rust_repo.reset(
        &rust_repo.revparse_ext("HEAD")?.0,
        git2::ResetType::Hard,
        Some(CheckoutBuilder::new().remove_untracked(true).force()),
    )?;

    let rust_head = checkout_to_ref(rust_repo, "master")?;
    checkout_to_ref(clippy_repo, "master")?;

    warn!("Removing `sync-from-clippy` branch");

    rust_repo
        .find_branch("sync-from-clippy", git2::BranchType::Local)?
        .delete()?;

    debug!("Cleaning any untracked files from .rust-upstream__");
    rust_repo.reset(
        &rust_head,
        git2::ResetType::Hard,
        Some(CheckoutBuilder::new().remove_untracked(true)),
    )?;

    // warn!("Removing {branch_name} from clippy");
    // rust_repo
    //     .find_branch(branch_name, git2::BranchType::Local)?
    //     .delete()?;

    for status in rust_repo
        .statuses(Some(
            StatusOptions::new()
                .recurse_untracked_dirs(true)
                .include_untracked(true),
        ))?
        .iter()
    {
        let path_canon =
            canonicalize(Path::new(RUST_TREE_PATH).join(status.path().unwrap())).unwrap();
        trace!("Trying to remove {}", path_canon.to_string_lossy());
        if fs::remove_dir_all(&path_canon).is_err() {
            fs::remove_file(path_canon).unwrap();
        }
    }

    debug!("Archiving results");

    fs::rename(
        Path::new(RUSTC_PERF_PATH).join("results.db"),
        Path::new("archive").join(format!("results-{pr}.db")),
    )?;

    Ok(())
}

fn build_rust() -> Result<ExitStatus, std::io::Error> {
    debug!("Building Clippy");
    pause();

    let rustc_path= Path::new(RUST_TREE_PATH)
    .join("build")
    .join("host")
    .join("stage2");
dbg!("1");

    let _ = fs::remove_dir_all(Path::new(".").join("stage2"));
dbg!("1");
fs::rename( rustc_path, Path::new(".").join("stage2"))?;
dbg!("1");

    Command::new("./x.py")
        .args(&[
            "build",
            "src/tools/clippy",
            "--stage",
            "2",
            "--set",
            "build.build-stage=2",
            "--set",
            "rust.incremental=false",
            "--set",
            "llvm.download-ci-llvm=true",
            "--set",
            "build.rustc=/home/meow/git/becnhv2/stage2/bin/rustc",
            // "build",
            // "src/tools/clippy",
            // "--stage",
            // "2",
            // "--set",
            // "build.build-stage=2",
            // // "--set",
            // // "rust.lto=thin",
            // // "--set",
            // // "build.extended=false",
            // // "--set",
            // // "rust.jemalloc=true",
            // // "--set",
            // // "rust.codegen-units=1",
            // // "--set",
            // // "rust.codegen-units-std=1",
            // // "--set",
            // // "rust.debug=false",
            // // "--set",
            // // "rust.optimize=true",
            // "--set",
            // "rust.incremental=false",
            // "--set",
            // "llvm.download-ci-llvm=true", // don't need to build LLVM in this case
            // "--set",
            // &format!(
            //     "build.rustc=/home/meow/git/becnhv2/stage2/bin/rustc",
            //     //canonicalize(Path::new("/home/meow/").join("stage2").join("bin").join("rustc"))?
            //       //  .to_string_lossy()
            // ),
        ])
        .current_dir(RUST_TREE_PATH)
        .status()
}

fn bench_artifact(id: &str) -> Result<()> {
    debug!("Fetching from rustc-perf");

    let perf_repo = Repository::open(RUSTC_PERF_PATH)?;
    perf_repo
        .find_remote("origin")?
        .fetch(&["master"], None, None)?;

    debug!("Building the collector");
    dbg!("2");

    Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(RUSTC_PERF_PATH)
        .spawn()?
        .wait()?;

    debug!("Waiting for rust build to wait (this will take a gooooood while)");
    dbg!("2");
    
    // rust_build_artifact.wait()?;

    debug!("Starting benchmarks");
    pause();

    let rustc_canon = canonicalize(RUST_TREE_PATH)?.join("build").join("host");

    Command::new("./target/release/collector")
        .args([
            "bench_local",
            &Path::new(".")
                .join("rustc")
                .to_string_lossy(),
            "--profiles",
            "Clippy",
            "--clippy",
            &rustc_canon
                .join("stage1-tools-bin")
                .join("cargo-clippy")
                .to_string_lossy(),
            "--id",
            id,
        ])
        .current_dir(RUSTC_PERF_PATH)
        .spawn()?;

    Ok(())
}

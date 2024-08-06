use std::{
    env::current_dir,
    fs::{self, canonicalize, create_dir},
    io::{self, stdin, Stdout, Write},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
};

use anyhow::{anyhow, bail, Context, Result};
use datetime::{convenience::Today, ISO};
use git2::{
    self,
    build::{CheckoutBuilder, RepoBuilder},
    FetchOptions, Repository, StatusOptions,
};
use regex::Regex;
use semver::Version;

use crate::{
    checkout_to_ref, copy_dir_all, pause, CLIPPY_PATH, RUSTC_PERF_PATH, RUST_TREE_PATH,
    SETUP_COMPLETED_LOCK,
};

use log::{debug, trace, warn};

pub(crate) fn get_pr(
    number: usize,
    rust_repo: &Repository,
    clippy_repo: &Repository,
    master: bool,
) -> Result<()> {
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
    checkout_to_ref(clippy_repo, &branch_name)?;

    debug!("Migrating that PR to tree");
    migrate_pr_to_tree(/*branch_name,*/ rust_repo /*clippy_repo*/)?;

    pause();

    debug!("Building Rust on sync-from-clippy");

    let mut rust_child = build_rust()?;

    debug!("Benchmarking artifact as PR-{number}");
    bench_artifact(&mut rust_child, &format!("PR-{number}"))?;

    if master {
        debug!("");
        checkout_to_ref(rust_repo, "master")?;
        let mut rust_child = build_rust()?;
        let today = datetime::LocalDate::today();
        warn!("Benching master as `master-{}`", today.iso());
        bench_artifact(&mut rust_child, &format!("master-{}", today.iso()))?;
    }

    // Cleanup
    cleanup(rust_repo, clippy_repo, number)?;
    Ok(())
}

fn migrate_pr_to_tree(
    // branch_name: &str,
    rust_repo: &Repository,
    // clippy_repo: &Repository,
) -> Result<()> {
    // Now, let's get the according version of Rust's
    debug!("Getting necessary version, so that everything's correct");
    // We'll do it in a bit of brute force, honestly.

    debug!("Installing toolchain via rustup to get version");

    let mut version = Version::parse(&read_toolchain_version()?)?;
    version.minor -= 1;

    debug!("Got version `{version}`, trying to check out on that tag");

    let tag_names = rust_repo.tag_names(Some("1.*.*"))?;

    tag_names.iter().for_each(|meow| debug!("{:?}", meow));

    if tag_names
        .into_iter()
        .find(|tag| *tag == Some(&version.to_string()))
        .is_none()
    {
        let mut current_minor = 0;
        for ele in tag_names.iter() {
            if let Some(ele) = ele {
                let ver = Version::parse(ele)?;
                if ver.minor > current_minor {
                    current_minor = ver.minor
                };
            };
        }

        dbg!(&version, &current_minor);
        // We're on beta
        // rust_repo.set_head(rust_repo.find_branch("origin/beta", git2::BranchType::Remote)?.get().name().unwrap())?;
        // debug!("Checking out bet");
        // checkout_to_ref(rust_repo, "remotes/origin/stable")?;
        if version.minor - current_minor == 1 {
            debug!("Checking out beta");
            checkout_to_ref(rust_repo, "remotes/origin/beta")?;
        }
    } else {
        checkout_to_ref(rust_repo, &version.to_string())?;
    }

    debug!("Synching Clippy with Rust@{version}");

    let head_oid = rust_repo.refname_to_id("HEAD")?;
    let head_commit = rust_repo.find_commit(head_oid)?;

    debug!("Creating branch");
    let sync_branch = rust_repo.branch("sync-from-clippy", &head_commit, true)?;

    trace!("Checkout tree");

    rust_repo.checkout_tree(head_commit.as_object(), None)?;
    trace!("Set head to sync branch");
    rust_repo.set_head(sync_branch.get().name().unwrap())?;

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

fn read_toolchain_version() -> Result<String> {
    let rust_toolchain =
        &fs::read_to_string(Path::new(CLIPPY_PATH).join("rust-toolchain"))?[23..41];

    debug!("Installing {rust_toolchain}");
    let rustup_toolchain_install = Command::new("rustup")
        .args(&["install", rust_toolchain])
        .output()?
        .stdout;

    let s = match std::str::from_utf8(&rustup_toolchain_install) {
        Ok(v) => v,
        Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
    };

    let re = Regex::new(r"rustc (\d\.\d+\.\d)-nightly")?;

    return Ok(re.captures(s).unwrap().get(0).unwrap().as_str()[6..12].to_string());
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

fn build_rust() -> Result<Child, io::Error> {
    debug!("Building Rust");
    pause();

    #[cfg(not(debug_assertions))]
    return Command::new("./x")
        .args(&[
            "build",
            "src/tools/clippy",
            "--stage=1",
            "--set",
            "rust.lto=thin",
            "--set",
            "build.extended=false",
            "--set",
            "rust.jemalloc=true",
            "--set",
            "rust.codegen-units=1",
            "--set",
            "rust.codegen-units-std=1",
            "--set",
            "rust.debug=false",
            "--set",
            "rust.optimize=true",
            "--set",
            "rust.incremental=false",
            "--set",
            "llvm.download-ci-llvm=true", // don't need to build LLVM in this case
        ])
        .current_dir(RUST_TREE_PATH)
        .spawn();

    #[cfg(debug_assertions)]
    return Command::new("ls").spawn();
}

fn bench_artifact(rust_build_artifact: &mut Child, id: &str) -> Result<()> {
    debug!("Fetching from rustc-perf");

    let perf_repo = Repository::open(RUSTC_PERF_PATH)?;
    perf_repo
        .find_remote("origin")?
        .fetch(&["master"], None, None)?;

    debug!("Building the collector");

    Command::new("cargo")
        .args(&["build", "--release"])
        .current_dir(RUSTC_PERF_PATH)
        .spawn()?
        .wait()?;

    debug!("Waiting for rust build to wait (this will take a gooooood while)");
    rust_build_artifact.wait()?;

    debug!("Starting benchmarks");

    let build_path = Path::new(RUST_TREE_PATH)
        .join("build")
        .join("host")
        .join("stage2")
        .join("bin");

    Command::new("./target/release/collector")
        .args(&[
            "bench_local",
            &build_path.join("rustc").to_string_lossy(),
            "--profiles",
            "Clippy",
            "--clippy",
            &build_path.join("cargo-clippy").to_string_lossy(),
            "--id",
            id,
        ])
        .spawn()?;

    Ok(())
}

fn only_master() -> Result<()> {
    Ok(())
}

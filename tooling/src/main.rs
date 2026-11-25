mod bench_fixture;
mod bump;
mod generated;
mod glob_imports;
mod verify;

use clap::{Parser, Subcommand};
use std::process::{Command, ExitCode, Stdio};

#[derive(Parser)]
#[command(name = "tooling", about = "Evenframe development tooling")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Validate generated output with Deno, protoc, and flatc.
    Generated,
    /// Run tests (unit, snapshot, e2e)
    Test {
        /// Run only snapshot tests
        #[arg(long)]
        snapshot: bool,

        /// Run only e2e tests (testground)
        #[arg(long)]
        e2e: bool,

        /// Run only the derive crate's tests
        #[arg(long)]
        derive: bool,

        /// Extra arguments passed through to cargo test
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },

    /// Manage insta snapshots
    Snapshot {
        #[command(subcommand)]
        action: SnapshotAction,
    },

    /// Run full verification: fmt, clippy, and all tests, applying the
    /// formatter and clippy's fixes first. Each run keeps its progress in
    /// `.verify/`.
    Verify {
        /// Leave the tree as it is: report formatting and lints instead of
        /// fixing them
        #[arg(long)]
        check: bool,

        /// Stop on the first failure instead of running all steps
        #[arg(long)]
        fail_fast: bool,

        /// Resume a run, by id or else the latest, running every step that
        /// did not pass
        #[arg(long, value_name = "ID", conflicts_with = "pipeline")]
        resume: Option<Option<String>>,

        /// The steps to run, in order, comma separated (every step when
        /// left out)
        #[arg(long, value_enum, value_delimiter = ',')]
        pipeline: Vec<verify::Step>,
    },

    /// Bump the version of every published crate, repin dependencies on
    /// them, and refresh every tracked lockfile
    Bump {
        #[arg(value_enum)]
        level: bump::BumpLevel,
    },

    /// Write a synthetic project for timing evenframe at scale
    BenchFixture {
        /// Directory to write the project into
        out: std::path::PathBuf,

        /// Number of modules; each links into the one before it
        #[arg(long, default_value_t = 40)]
        modules: usize,

        /// Groups of four types (table, object, enum, recursive object) per module
        #[arg(long, default_value_t = 8)]
        types: usize,
    },
}

#[derive(Subcommand)]
enum SnapshotAction {
    /// Accept all pending snapshot changes
    Accept,
    /// Interactively review pending snapshot changes
    Review,
    /// Regenerate all snapshots by running tests then accepting
    Update,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let ok = match cli.command {
        Cmd::Generated => generated::check(),
        Cmd::Test {
            snapshot,
            e2e,
            derive,
            extra,
        } => cmd_test(snapshot, e2e, derive, &extra),
        Cmd::Snapshot { action } => cmd_snapshot(action),
        Cmd::Verify {
            check,
            fail_fast,
            resume,
            pipeline,
        } => verify::cmd_verify(
            match resume {
                Some(id) => verify::Source::Resume(id),
                None => verify::Source::Pipeline(pipeline),
            },
            if check {
                verify::Fixes::Check
            } else {
                verify::Fixes::Apply
            },
            fail_fast,
        ),
        Cmd::Bump { level } => bump::cmd_bump(level),
        Cmd::BenchFixture {
            out,
            modules,
            types,
        } => bench_fixture::cmd_bench_fixture(&out, modules, types),
    };

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The one feature resolution every test build uses, the one clippy checks.
/// Cargo unifies a dependency's features only within one invocation, so an
/// invocation selecting other packages or features rebuilds the whole tree.
const WORKSPACE_ALL_FEATURES: [&str; 2] = ["--workspace", "--all-features"];

fn cmd_test(snapshot: bool, e2e: bool, derive: bool, extra: &[String]) -> bool {
    let specific = snapshot || e2e || derive;

    if !specific {
        header("tests (workspace, all features)");
        if !run("cargo", |c| {
            c.arg("test").args(WORKSPACE_ALL_FEATURES).args(extra);
        }) {
            return false;
        }
    } else if snapshot {
        header("snapshot tests (workspace, all features)");
        if !run("cargo", |c| {
            c.arg("test")
                .args(WORKSPACE_ALL_FEATURES)
                .args(["--test", "snapshot_tests"])
                .args(extra);
        }) {
            return false;
        }
    }

    if !specific || snapshot {
        header("generated output (deno, protoc, flatc)");
        if !generated::check() {
            return false;
        }
    }

    if derive {
        header("derive tests (all features)");
        if !run("cargo", |c| {
            c.args(["test", "-p", "evenframe_derive", "--all-features"])
                .args(extra);
        }) {
            return false;
        }
    }

    if !specific || e2e {
        header("e2e tests (testground, all features)");
        if !run("cargo", |c| {
            c.args(["test", "--all-features"])
                .current_dir(testground_dir())
                .args(extra);
        }) {
            return false;
        }
    }

    true
}

fn cmd_snapshot(action: SnapshotAction) -> bool {
    match action {
        SnapshotAction::Accept => run("cargo", |c| {
            c.args(["insta", "accept"]);
        }),
        SnapshotAction::Review => run("cargo", |c| {
            c.args(["insta", "review"]);
        }),
        SnapshotAction::Update => {
            header("regenerating snapshots");
            // A snapshot that changed fails its test; accepting is the point.
            if !run("cargo", |c| {
                c.arg("test")
                    .args(WORKSPACE_ALL_FEATURES)
                    .args(["--test", "snapshot_tests"]);
            }) {
                println!("snapshot tests reported changes; accepting them");
            }
            header("accepting snapshots");
            run("cargo", |c| {
                c.args(["insta", "accept"]);
            })
        }
    }
}

fn header(label: &str) {
    println!("\n--- {label} ---");
}

fn testground_dir() -> std::path::PathBuf {
    project_root().join("tooling/testground")
}

fn project_root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tooling should be in workspace root")
        .to_path_buf()
}

fn run(program: &str, configure: impl FnOnce(&mut Command)) -> bool {
    let mut cmd = Command::new(program);
    configure(&mut cmd);
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    match cmd.status() {
        Ok(status) => {
            if !status.success() {
                eprintln!("command failed with {status}");
            }
            status.success()
        }
        Err(e) => {
            eprintln!("failed to run {program}: {e}");
            false
        }
    }
}

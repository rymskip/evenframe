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
    /// Run tests (unit, snapshot, e2e)
    Test {
        /// Run only snapshot tests
        #[arg(long)]
        snapshot: bool,

        /// Run only e2e tests (evenframe_playground)
        #[arg(long)]
        e2e: bool,

        /// Run only derive crate trybuild tests
        #[arg(long)]
        derive: bool,

        /// Feature set to use for evenframe_core tests
        #[arg(long, default_value = "typesync-all")]
        features: String,

        /// Extra arguments passed through to cargo test
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        extra: Vec<String>,
    },

    /// Manage insta snapshots
    Snapshot {
        #[command(subcommand)]
        action: SnapshotAction,
    },

    /// Run full verification: fmt, clippy, and all tests
    Verify {
        /// Stop on the first failure instead of running all steps
        #[arg(long)]
        fail_fast: bool,
    },
}

#[derive(Subcommand)]
enum SnapshotAction {
    /// Accept all pending snapshot changes
    Accept,
    /// Interactively review pending snapshot changes
    Review,
    /// Regenerate all snapshots by running tests then accepting
    Update {
        /// Feature set for snapshot tests
        #[arg(long, default_value = "typesync-all")]
        features: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let ok = match cli.command {
        Cmd::Test {
            snapshot,
            e2e,
            derive,
            features,
            extra,
        } => cmd_test(snapshot, e2e, derive, &features, &extra),
        Cmd::Snapshot { action } => cmd_snapshot(action),
        Cmd::Verify { fail_fast } => cmd_verify(fail_fast),
    };

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn cmd_test(snapshot: bool, e2e: bool, derive: bool, features: &str, extra: &[String]) -> bool {
    let specific = snapshot || e2e || derive;

    if !specific || snapshot {
        header(&format!(
            "snapshot tests (evenframe_core --features {features})"
        ));
        if !run("cargo", |c| {
            c.args([
                "test",
                "-p",
                "evenframe_core",
                "--features",
                features,
                "--test",
                "snapshot_tests",
            ])
            .args(extra);
        }) {
            return false;
        }
    }

    if !specific || derive {
        header("derive trybuild tests");
        if !run("cargo", |c| {
            c.args(["test", "-p", "evenframe_derive"]).args(extra);
        }) {
            return false;
        }
    }

    if !specific {
        header(&format!(
            "evenframe_core unit tests (--features {features})"
        ));
        if !run("cargo", |c| {
            c.args([
                "test",
                "-p",
                "evenframe_core",
                "--features",
                features,
                "--lib",
            ])
            .args(extra);
        }) {
            return false;
        }
    }

    if !specific || e2e {
        header("e2e tests (evenframe_playground)");
        if !run("cargo", |c| {
            c.arg("test").current_dir(playground_dir()).args(extra);
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
        SnapshotAction::Update { features } => {
            header("regenerating snapshots");
            let _ = run("cargo", |c| {
                c.args([
                    "test",
                    "-p",
                    "evenframe_core",
                    "--features",
                    &features,
                    "--test",
                    "snapshot_tests",
                ]);
            });
            header("accepting snapshots");
            run("cargo", |c| {
                c.args(["insta", "accept"]);
            })
        }
    }
}

fn cmd_verify(fail_fast: bool) -> bool {
    let steps: Vec<(&str, Box<dyn Fn() -> bool>)> = vec![
        (
            "fmt",
            Box::new(|| {
                run("cargo", |c| {
                    c.args(["fmt", "--all", "--", "--check"]);
                })
            }),
        ),
        (
            "clippy (all features, all targets)",
            Box::new(|| {
                run("cargo", |c| {
                    c.args([
                        "clippy",
                        "--workspace",
                        "--all-targets",
                        "--all-features",
                        "--",
                        "-D",
                        "warnings",
                    ]);
                })
            }),
        ),
        (
            "evenframe_core unit tests (full)",
            Box::new(|| {
                run("cargo", |c| {
                    c.args(["test", "-p", "evenframe_core", "--features", "full"]);
                })
            }),
        ),
        (
            "snapshot tests (typesync-all)",
            Box::new(|| {
                run("cargo", |c| {
                    c.args([
                        "test",
                        "-p",
                        "evenframe_core",
                        "--features",
                        "typesync-all",
                        "--test",
                        "snapshot_tests",
                    ]);
                })
            }),
        ),
        (
            "derive trybuild tests",
            Box::new(|| {
                run("cargo", |c| {
                    c.args(["test", "-p", "evenframe_derive"]);
                })
            }),
        ),
        (
            "e2e tests (evenframe_playground)",
            Box::new(|| {
                run("cargo", |c| {
                    c.arg("test").current_dir(playground_dir());
                })
            }),
        ),
    ];

    let mut failed: Vec<&str> = Vec::new();

    for (label, step) in &steps {
        header(label);
        if !step() {
            failed.push(label);
            if fail_fast {
                eprintln!("\n=== verify failed (--fail-fast) ===");
                return false;
            }
        }
    }

    if failed.is_empty() {
        println!("\n=== all checks passed ===");
        true
    } else {
        eprintln!(
            "\n=== verify failed ({}/{} steps) ===",
            failed.len(),
            steps.len()
        );
        for label in &failed {
            eprintln!("  FAIL: {label}");
        }
        false
    }
}

fn header(label: &str) {
    println!("\n--- {label} ---");
}

fn playground_dir() -> std::path::PathBuf {
    project_root().join("evenframe_playground")
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

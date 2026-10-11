//! `verify`: every check the repository holds itself to, as a pipeline of
//! named steps. Each run keeps its progress in `.verify/<id>.json`, so a run
//! stopped partway, by a failure or by its process ending, resumes with the
//! steps that did not pass.
//!
//! A run fixes what a tool can fix for it, the formatting and clippy's
//! machine-applicable lints, and then checks. `--check` leaves the tree as it
//! is and only reports.

use crate::{generated, glob_imports, header, project_root, run, testground_dir};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Whether a run may rewrite the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixes {
    /// Format the code and apply clippy's machine-applicable fixes, then check.
    Apply,
    /// Leave the tree as it is and report.
    Check,
}

/// `cargo fmt` over a workspace: applied, or only checked.
fn fmt_args(fixes: Fixes) -> &'static [&'static str] {
    match fixes {
        Fixes::Apply => &["fmt", "--all"],
        Fixes::Check => &["fmt", "--all", "--", "--check"],
    }
}

/// Clippy over `scope`, refusing every warning. With fixes, its
/// machine-applicable ones are applied and formatted first, so the gate sees
/// only what is left for a person.
fn clippy(cargo: impl Fn(&[&str]) -> bool, scope: &[&str], fixes: Fixes) -> bool {
    let gate = [&["clippy"][..], scope, &["--", "-D", "warnings"][..]].concat();
    match fixes {
        Fixes::Check => cargo(&gate),
        Fixes::Apply => {
            let fix = [
                &["clippy", "--fix", "--allow-dirty", "--allow-staged"][..],
                scope,
            ]
            .concat();
            cargo(&fix) && cargo(fmt_args(Fixes::Apply)) && cargo(&gate)
        }
    }
}

/// A verify step, by the name `--pipeline` takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum Step {
    GlobImports,
    Fmt,
    Clippy,
    TestgroundFmt,
    TestgroundClippy,
    CoreTests,
    Snapshots,
    Generated,
    CliTests,
    DeriveTests,
    DeriveCheck,
    E2e,
}

impl Step {
    /// Every step, in the order a full run takes them.
    const ALL: [Step; 12] = [
        Step::GlobImports,
        Step::Fmt,
        Step::Clippy,
        Step::TestgroundFmt,
        Step::TestgroundClippy,
        Step::CoreTests,
        Step::Snapshots,
        Step::Generated,
        Step::CliTests,
        Step::DeriveTests,
        Step::DeriveCheck,
        Step::E2e,
    ];

    fn label(self) -> &'static str {
        match self {
            Step::GlobImports => "glob imports",
            Step::Fmt => "fmt",
            Step::Clippy => "clippy (all features, all targets)",
            Step::TestgroundFmt => "fmt (testground)",
            Step::TestgroundClippy => "clippy (testground, all features, all targets)",
            Step::CoreTests => "evenframe_core unit tests (full)",
            Step::Snapshots => "snapshot tests (typesync-all)",
            Step::Generated => "generated output (deno, protoc, flatc)",
            Step::CliTests => "evenframe CLI tests",
            Step::DeriveTests => "derive trybuild tests",
            Step::DeriveCheck => {
                "derive without features, then with metadata, then with SurrealValue"
            }
            Step::E2e => "e2e tests (testground, all features)",
        }
    }

    fn run(self, fixes: Fixes) -> bool {
        let cargo = |args: &[&str]| {
            run("cargo", |command| {
                command.args(args);
            })
        };
        let testground = |args: &[&str]| {
            run("cargo", |command| {
                command.args(args).current_dir(testground_dir());
            })
        };
        match self {
            Step::GlobImports => glob_imports::check(),
            Step::Fmt => cargo(fmt_args(fixes)),
            Step::Clippy => clippy(
                cargo,
                &["--workspace", "--all-targets", "--all-features"],
                fixes,
            ),
            Step::TestgroundFmt => testground(fmt_args(fixes)),
            Step::TestgroundClippy => {
                clippy(testground, &["--all-targets", "--all-features"], fixes)
            }
            Step::CoreTests => cargo(&["test", "-p", "evenframe_core", "--features", "full"]),
            Step::Snapshots => cargo(&[
                "test",
                "-p",
                "evenframe_core",
                "--features",
                "typesync-all",
                "--test",
                "snapshot_tests",
            ]),
            Step::Generated => generated::check(),
            Step::CliTests => cargo(&["test", "-p", "evenframe"]),
            Step::DeriveTests => cargo(&["test", "-p", "evenframe_derive"]),
            Step::DeriveCheck => {
                cargo(&["test", "-p", "derive_check"])
                    && cargo(&["test", "-p", "derive_check", "--features", "metadata"])
                    && cargo(&[
                        "test",
                        "-p",
                        "derive_check",
                        "--features",
                        "surrealdb-types",
                    ])
            }
            Step::E2e => testground(&["test", "--all-features"]),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Outcome {
    Pending,
    /// Started and not finished: the run's process ended while it ran.
    Running,
    Passed,
    Failed,
}

#[derive(Debug, Serialize, Deserialize)]
struct StepRecord {
    step: Step,
    outcome: Outcome,
}

/// A run's progress, as `.verify/<id>.json` keeps it.
#[derive(Debug, Serialize, Deserialize)]
struct RunRecord {
    id: String,
    /// The commit the run started on.
    commit: String,
    steps: Vec<StepRecord>,
}

impl RunRecord {
    fn path(id: &str) -> PathBuf {
        runs_dir().join(format!("{id}.json"))
    }

    fn load(id: &str) -> Result<Self, String> {
        let path = Self::path(id);
        let text = fs::read_to_string(&path).map_err(|error| {
            format!("cannot read the run `{id}` at {}: {error}", path.display())
        })?;
        serde_json::from_str(&text).map_err(|error| {
            format!(
                "the run `{id}` at {} is unreadable: {error}",
                path.display()
            )
        })
    }

    /// Writes the record beside itself and renames it into place, so a run
    /// stopped mid-write keeps its previous state.
    fn save(&self) -> Result<(), String> {
        let path = Self::path(&self.id);
        let staged = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| format!("cannot encode the run `{}`: {error}", self.id))?;
        fs::write(&staged, text)
            .and_then(|()| fs::rename(&staged, &path))
            .map_err(|error| format!("cannot save the run at {}: {error}", path.display()))
    }
}

fn runs_dir() -> PathBuf {
    project_root().join(".verify")
}

/// The id of the newest run: ids are UTC timestamps, so they sort by age.
fn latest_run() -> Result<String, String> {
    let dir = runs_dir();
    let entries = fs::read_dir(&dir)
        .map_err(|error| format!("no runs to resume in {}: {error}", dir.display()))?;
    let mut ids = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("cannot list the runs in {}: {error}", dir.display()))?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
            && let Some(id) = path.file_stem().and_then(|stem| stem.to_str())
        {
            ids.push(id.to_owned());
        }
    }
    ids.into_iter()
        .max()
        .ok_or_else(|| format!("no runs to resume in {}", dir.display()))
}

fn head_commit() -> Result<String, String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_root())
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git rev-parse HEAD failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// A new run of `pipeline`, every step pending, saved under a fresh id.
fn start(pipeline: &[Step]) -> Result<RunRecord, String> {
    fs::create_dir_all(runs_dir())
        .map_err(|error| format!("cannot create {}: {error}", runs_dir().display()))?;
    let id = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    if RunRecord::path(&id).exists() {
        return Err(format!(
            "a run `{id}` already exists; start again in a second"
        ));
    }
    let record = RunRecord {
        id,
        commit: head_commit()?,
        steps: pipeline
            .iter()
            .map(|&step| StepRecord {
                step,
                outcome: Outcome::Pending,
            })
            .collect(),
    };
    record.save()?;
    Ok(record)
}

/// Where a verify run comes from.
pub enum Source {
    /// A new run of these steps, in order.
    Pipeline(Vec<Step>),
    /// An earlier run, by id, else the latest.
    Resume(Option<String>),
}

pub fn cmd_verify(source: Source, fixes: Fixes, fail_fast: bool) -> bool {
    match verify(source, fixes, fail_fast) {
        Ok(passed) => passed,
        Err(problem) => {
            eprintln!("verify: {problem}");
            false
        }
    }
}

fn verify(source: Source, fixes: Fixes, fail_fast: bool) -> Result<bool, String> {
    let mut record = match source {
        Source::Pipeline(pipeline) if pipeline.is_empty() => start(&Step::ALL)?,
        Source::Pipeline(pipeline) => start(&pipeline)?,
        Source::Resume(id) => {
            let id = match id {
                Some(id) => id,
                None => latest_run()?,
            };
            let record = RunRecord::load(&id)?;
            let head = head_commit()?;
            if head != record.commit {
                println!(
                    "resuming `{id}`, started on {}; HEAD is now {head}",
                    record.commit
                );
            }
            record
        }
    };
    println!("verify run `{}`", record.id);

    for position in 0..record.steps.len() {
        let Some(step) = record
            .steps
            .get(position)
            .filter(|step| step.outcome != Outcome::Passed)
            .map(|step| step.step)
        else {
            continue;
        };
        set_outcome(&mut record, position, Outcome::Running)?;
        header(step.label());
        let outcome = if step.run(fixes) {
            Outcome::Passed
        } else {
            Outcome::Failed
        };
        set_outcome(&mut record, position, outcome)?;
        if outcome == Outcome::Failed && fail_fast {
            break;
        }
    }

    let unfinished: Vec<&StepRecord> = record
        .steps
        .iter()
        .filter(|step| step.outcome != Outcome::Passed)
        .collect();
    if unfinished.is_empty() {
        println!("\n=== all checks passed (run `{}`) ===", record.id);
        return Ok(true);
    }
    eprintln!(
        "\n=== verify failed ({}/{} steps) ===",
        unfinished.len(),
        record.steps.len()
    );
    for step in &unfinished {
        let state = match step.outcome {
            Outcome::Failed => "FAIL",
            Outcome::Pending | Outcome::Running => "NOT RUN",
            Outcome::Passed => "PASS",
        };
        eprintln!("  {state}: {}", step.step.label());
    }
    eprintln!("resume with `verify --resume {}`", record.id);
    Ok(false)
}

fn set_outcome(record: &mut RunRecord, position: usize, outcome: Outcome) -> Result<(), String> {
    if let Some(step) = record.steps.get_mut(position) {
        step.outcome = outcome;
    }
    record.save()
}

#[cfg(test)]
mod tests {
    use super::{Fixes, clippy, fmt_args};
    use std::cell::RefCell;

    /// The cargo invocations `clippy` makes over a scope, each joined by
    /// spaces, with every one of them answering `passes`.
    fn invocations(fixes: Fixes, passes: bool) -> (bool, Vec<String>) {
        let made = RefCell::new(Vec::new());
        let passed = clippy(
            |args| {
                made.borrow_mut().push(args.join(" "));
                passes
            },
            &["--workspace"],
            fixes,
        );
        (passed, made.into_inner())
    }

    #[test]
    fn a_checking_run_formats_and_fixes_nothing() {
        assert_eq!(fmt_args(Fixes::Check), ["fmt", "--all", "--", "--check"]);
        let (passed, made) = invocations(Fixes::Check, true);
        assert!(passed);
        assert_eq!(made, ["clippy --workspace -- -D warnings"]);
    }

    #[test]
    fn a_fixing_run_fixes_then_formats_then_checks() {
        assert_eq!(fmt_args(Fixes::Apply), ["fmt", "--all"]);
        let (passed, made) = invocations(Fixes::Apply, true);
        assert!(passed);
        assert_eq!(
            made,
            [
                "clippy --fix --allow-dirty --allow-staged --workspace",
                "fmt --all",
                "clippy --workspace -- -D warnings",
            ]
        );
    }

    #[test]
    fn a_fix_that_fails_stops_the_step() {
        let (passed, made) = invocations(Fixes::Apply, false);
        assert!(!passed);
        assert_eq!(made.len(), 1, "nothing runs after the failure");
    }
}

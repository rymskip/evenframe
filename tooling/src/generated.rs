//! Checks the generated output recorded in the typesync snapshots with the
//! tools that consume it: deno type-checks the TypeScript and runs the serde
//! parity cases, protoc and flatc compile the schemas.

use crate::{project_root, run};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Each snapshot kind, its file extension, and what the generated file needs
/// around the snapshot body to stand alone.
const KINDS: [(&str, &str, &str, &str); 6] = [
    (
        "arktype",
        "ts",
        "import { scope } from 'arktype';\n\n",
        "\nexport const validator = exported;\n",
    ),
    ("effect", "ts", "import { Schema } from \"effect\";\n\n", ""),
    ("macroforge", "ts", "", ""),
    ("protobuf", "proto", "", ""),
    ("protobuf_validated", "proto", "", ""),
    ("flatbuffers", "fbs", "", ""),
];

pub fn check() -> bool {
    match write_outputs() {
        Ok(outputs) => compile(&outputs),
        Err(error) => {
            eprintln!("writing the generated output failed: {error}");
            false
        }
    }
}

/// Where each kind of generated file was written.
struct Outputs {
    typescript: PathBuf,
    typescript_files: Vec<String>,
    schemas: Vec<(&'static str, PathBuf)>,
}

fn write_outputs() -> std::io::Result<Outputs> {
    let root = project_root();
    let snapshots = root.join("evenframe_core/tests/snapshots");
    let out = root.join("target/generated-check");
    if out.exists() {
        fs::remove_dir_all(&out)?;
    }
    let typescript = out.join("typescript");
    fs::create_dir_all(&typescript)?;
    let check_dir = root.join("tooling/generated_check");
    for file in ["deno.json", "serde_parity.ts"] {
        fs::copy(check_dir.join(file), typescript.join(file))?;
    }

    let mut typescript_files = Vec::new();
    let mut schemas = Vec::new();
    for (kind, extension, prelude, suffix) in KINDS {
        let prefix = format!("snapshot_tests__{kind}__{kind}_");
        let mut snapshot_paths: Vec<PathBuf> = fs::read_dir(&snapshots)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<_>>()?;
        snapshot_paths.sort();
        for path in snapshot_paths {
            let Some(fixture) = file_name(&path)
                .strip_prefix(&prefix)
                .and_then(|rest| rest.strip_suffix(".snap"))
                .map(str::to_string)
            else {
                continue;
            };
            let body = snapshot_body(&path)?;
            let name = format!("{kind}_{fixture}.{extension}");
            let directory = if extension == "ts" {
                typescript_files.push(name.clone());
                typescript.clone()
            } else {
                let directory = out.join(kind);
                schemas.push((kind, directory.join(&name)));
                directory
            };
            fs::create_dir_all(&directory)?;
            fs::write(directory.join(&name), format!("{prelude}{body}{suffix}"))?;
        }
    }
    Ok(Outputs {
        typescript,
        typescript_files,
        schemas,
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The snapshot's recorded output, after insta's metadata header.
fn snapshot_body(path: &Path) -> std::io::Result<String> {
    let text = fs::read_to_string(path)?;
    text.splitn(3, "---\n")
        .nth(2)
        .map(str::to_string)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{} has no insta header", path.display()),
            )
        })
}

/// Runs a schema compiler, failing on a warning as well as an error: either
/// is a defect in the generated schema.
fn compile_schema(program: &str, configure: impl FnOnce(&mut Command)) -> bool {
    let mut command = Command::new(program);
    configure(&mut command);
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            eprintln!("failed to run {program}: {error}");
            return false;
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    print!("{stdout}");
    eprint!("{stderr}");
    if !output.status.success() {
        eprintln!("{program} failed with {}", output.status);
        return false;
    }
    let warnings = stdout
        .lines()
        .chain(stderr.lines())
        .filter(|line| line.contains("warning:"))
        .count();
    if warnings > 0 {
        eprintln!("{program} printed {warnings} warning(s)");
    }
    warnings == 0
}

fn compile(outputs: &Outputs) -> bool {
    // protoc-gen-validate's rules import protobuf's well-known types, which
    // ship in the pixi environment's include directory.
    let Some(prefix) = std::env::var_os("CONDA_PREFIX") else {
        eprintln!(
            "CONDA_PREFIX is not set; run this through `pixi run` so protoc finds its includes"
        );
        return false;
    };
    let well_known = PathBuf::from(prefix).join("include");
    let validate = project_root().join("tooling/generated_check");
    let mut ok = run("deno", |command| {
        command
            .args(["check", "--config", "deno.json"])
            .args(&outputs.typescript_files)
            .current_dir(&outputs.typescript);
    });
    ok &= run("deno", |command| {
        command
            .args(["run", "--config", "deno.json", "serde_parity.ts"])
            .current_dir(&outputs.typescript);
    });
    for (kind, path) in &outputs.schemas {
        let Some(directory) = path.parent() else {
            eprintln!("{} has no parent directory", path.display());
            ok = false;
            continue;
        };
        ok &= match *kind {
            "protobuf" | "protobuf_validated" => compile_schema("protoc", |command| {
                command
                    .arg(format!("--proto_path={}", directory.display()))
                    .arg(format!("--proto_path={}", validate.display()))
                    .arg(format!("--proto_path={}", well_known.display()))
                    .arg(format!(
                        "--descriptor_set_out={}",
                        path.with_extension("pb").display()
                    ))
                    .arg(path);
            }),
            _ => compile_schema("flatc", |command| {
                command
                    .args(["--cpp", "-o"])
                    .arg(directory.join("cpp"))
                    .arg(path);
            }),
        };
    }
    ok
}

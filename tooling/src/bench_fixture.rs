//! Writes a synthetic evenframe project for timing the scan, typesync,
//! schemasync and mockmake paths at a size the testground does not reach.
//!
//! Each module holds `types` groups of four types: a table with formatted
//! fields, validators and links into the previous module, an embedded
//! object, an enum with every variant shape, and a self-recursive object.
//! Type names use letters only, so their case conversions are stable.

use crate::project_root;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

pub fn cmd_bench_fixture(out: &Path, modules: usize, types: usize) -> bool {
    match write_fixture(out, modules, types) {
        Ok(()) => {
            println!(
                "wrote {} types over {modules} modules to {}",
                modules * types * 4,
                out.display()
            );
            true
        }
        Err(error) => {
            eprintln!("writing the bench fixture failed: {error}");
            false
        }
    }
}

fn write_fixture(out: &Path, modules: usize, types: usize) -> std::io::Result<()> {
    if modules == 0 || types == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "modules and types must both be at least 1",
        ));
    }
    let src = out.join("src");
    fs::create_dir_all(&src)?;
    fs::write(out.join("Cargo.toml"), manifest())?;
    fs::write(out.join("evenframe.toml"), CONFIG)?;

    let mut lib = String::new();
    for module in 0..modules {
        writeln!(lib, "pub mod {};", module_name(module)).map_err(std::io::Error::other)?;
        let source = module_source(module, types).map_err(std::io::Error::other)?;
        fs::write(src.join(format!("{}.rs", module_name(module))), source)?;
    }
    fs::write(src.join("lib.rs"), lib)
}

fn manifest() -> String {
    let evenframe = project_root().join("evenframe");
    format!(
        "[package]\nname = \"bench_fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
         [workspace]\n\n\
         [dependencies]\n\
         evenframe = {{ path = \"{}\", features = [\"schemasync\"] }}\n\
         serde = {{ version = \"1\", features = [\"derive\"] }}\n",
        evenframe.display()
    )
}

const CONFIG: &str = r#"[general]
apply_aliases = []

[schemasync]
should_generate_mocks = true

[schemasync.database]
provider = "surrealdb"
url = "${SURREALDB_URL:-http://localhost:8123}"
namespace = "${SURREALDB_NS:-bench}"
database = "${SURREALDB_DB:-bench}"
timeout = 60

[schemasync.mock_gen_config]
default_record_count = 50
default_preservation_mode = "Smart"
full_refresh_mode = false

[typesync]
outputs = [
  { kind = "arktype", dir = "./generated/arktype" },
  { kind = "effect", dir = "./generated/effect" },
  { kind = "effect", dir = "./generated/effect_per_file", mode = "per_file" },
  { kind = "macroforge", dir = "./generated/macroforge", mode = "per_file" },
]
"#;

/// `index` written in base 26 with letters, capitalized: 0 is `A`, 27 is `Bb`.
fn letters(index: usize) -> String {
    let mut digits = Vec::new();
    let mut rest = index;
    loop {
        digits.push(char::from(b'a' + (rest % 26) as u8));
        rest /= 26;
        if rest == 0 {
            break;
        }
    }
    digits.reverse();
    digits[0] = digits[0].to_ascii_uppercase();
    digits.into_iter().collect()
}

/// `Of` keeps the two letter groups apart, so no run of capitals reads as an
/// acronym and the name survives Pascal and snake case round trips.
fn type_suffix(module: usize, group: usize) -> String {
    format!("{}Of{}", letters(module), letters(group))
}

fn module_name(module: usize) -> String {
    format!("module_{}", letters(module).to_ascii_lowercase())
}

fn module_source(module: usize, types: usize) -> Result<String, std::fmt::Error> {
    let mut source = String::from(
        "use evenframe::Evenframe;\nuse evenframe::types::RecordLink;\nuse serde::Serialize;\n",
    );
    if module > 0 {
        let linked: Vec<String> = (0..types)
            .map(|group| format!("Record{}", type_suffix(module - 1, group)))
            .collect();
        writeln!(
            source,
            "use super::{}::{{{}}};",
            module_name(module - 1),
            linked.join(", ")
        )?;
    }
    for group in 0..types {
        let suffix = type_suffix(module, group);
        let previous = (module > 0).then(|| {
            (
                type_suffix(module - 1, group),
                type_suffix(module - 1, (group + 1) % types),
            )
        });
        write!(
            source,
            r#"
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Address{suffix} {{
    #[format(StreetAddress)]
    pub street: String,
    #[format(City)]
    pub city: String,
    #[format(PostalCode)]
    pub postal_code: Option<String>,
}}

#[derive(Debug, Clone, Serialize, Evenframe)]
pub enum Status{suffix} {{
    Active,
    Suspended {{ reason: String, until: Option<String> }},
    Archived(String),
}}

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Tree{suffix} {{
    pub label: String,
    pub children: Vec<Tree{suffix}>,
}}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 50)]
pub struct Record{suffix} {{
    pub id: String,
    #[format(FullName)]
    pub name: String,
    #[format(Email)]
    pub email: String,
    #[format(TimeZone)]
    pub time_zone: String,
    #[format(DateTime)]
    pub created_at: String,
    #[validators(NumberValidator::NonNegative)]
    pub score: u32,
    #[validators(StringValidator::MaxLength(200))]
    pub note: Option<String>,
    pub address: Address{suffix},
    pub status: Status{suffix},
"#
        )?;
        if let Some((parent, sibling)) = previous {
            write!(
                source,
                "    pub parent: Option<RecordLink<Record{parent}>>,\n    pub related: Vec<RecordLink<Record{sibling}>>,\n"
            )?;
        }
        source.push_str("}\n");
    }
    Ok(source)
}

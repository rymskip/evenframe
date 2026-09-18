//! Command-line interface definitions for Evenframe.

use clap::{Args, Parser, Subcommand, ValueEnum};
use evenframe_core::config::SourceOfTruth;
use evenframe_core::schemasync::config::DatabaseProvider;
use evenframe_core::typesync::config::OutputKind;
use std::path::PathBuf;

/// Evenframe - TypeScript type generation and database schema synchronization
#[derive(Parser, Debug)]
#[command(name = "evenframe")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    /// Path to evenframe.toml configuration file
    #[arg(short, long, global = true, env = "EVENFRAME_CONFIG")]
    pub config: Option<PathBuf>,

    /// Source of truth for type definitions
    #[arg(long, global = true, value_enum, default_value = "rust")]
    pub source: SourceOfTruth,

    // Its own id, apart from the subcommands' `-o <FILE>`: clap copies a
    // global argument's value up from any subcommand argument sharing its id.
    /// Directory for generated types, when one output is generated (overrides its `dir`)
    #[arg(long = "output", global = true)]
    pub output_dir: Option<PathBuf>,

    /// Increase logging verbosity (repeat for more: -v=info, -vv=debug, -vvv=trace)
    #[arg(
        short,
        long,
        global = true,
        action = clap::ArgAction::Count,
        conflicts_with = "quiet"
    )]
    pub verbose: u8,

    /// Silence all non-error logging
    #[arg(short, long, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

impl Cli {
    /// Returns the default `tracing_subscriber` env-filter directive for the
    /// current `--verbose`/`--quiet` settings. Callers can use this when
    /// `RUST_LOG` is unset.
    pub fn log_filter(&self) -> &'static str {
        if self.quiet {
            "evenframe=error,evenframe_core=error"
        } else {
            match self.verbose {
                0 => "evenframe=warn,evenframe_core=warn",
                1 => "evenframe=info,evenframe_core=info",
                2 => "evenframe=debug,evenframe_core=debug",
                _ => "evenframe=trace,evenframe_core=trace",
            }
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Generate TypeScript types and schemas
    Typesync(TypesyncArgs),

    /// Synchronize database schema
    Schemasync(SchemasyncArgs),

    /// Insert mock data using the scan cache (no source scan)
    ///
    /// Reads `.evenframe/cache.json`, written by every command that scans
    /// the workspace, and refuses to run if the sources changed since. The
    /// database schema must already be in place (see `schemasync`).
    Mockmake(MockmakeArgs),

    /// Run full generation pipeline (typesync + schemasync)
    Generate(GenerateArgs),

    /// Initialize a new evenframe.toml configuration file
    Init(InitArgs),

    /// Report whether the scan cache still matches the Rust sources
    ///
    /// Recomputes the scan fingerprint with a cheap file walk (no Rust
    /// parsing, no database) and compares it with `.evenframe/cache.json`.
    /// Exits with status 1 when the two disagree, so CI can gate on it.
    Check(CheckArgs),

    /// Validate configuration and detected types
    Validate(ValidateArgs),

    /// Display information about detected types and configuration
    Info(InfoArgs),

    /// Test an output rule plugin by running it against the project types
    /// and printing what it produces (JSON output for scripted assertions)
    TestPlugin(TestPluginArgs),

    /// Manage the macro expansion cache
    Expand(ExpandArgs),
}

// ============================================================================
// Typesync Arguments
// ============================================================================

#[derive(Args, Debug, Clone)]
pub struct TypesyncArgs {
    #[command(subcommand)]
    pub command: Option<TypesyncCommands>,

    /// Generate all enabled type outputs (default behavior)
    #[arg(long)]
    pub all: bool,

    /// Only generate the configured outputs of these kinds (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub formats: Option<Vec<OutputKind>>,

    /// Skip the configured outputs of these kinds (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub skip: Option<Vec<OutputKind>>,

    /// Write effect and macroforge outputs one file per type (overrides config)
    #[arg(long)]
    pub per_file: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum TypesyncCommands {
    /// Generate ArkType validator schemas
    Arktype(ArktypeArgs),

    /// Generate Effect-TS schemas
    Effect(EffectArgs),

    /// Generate Macroforge TypeScript interfaces
    Macroforge(MacroforgeArgs),

    /// Generate FlatBuffers schema file
    Flatbuffers(FlatbuffersArgs),

    /// Generate Protocol Buffers schema file
    Protobuf(ProtobufArgs),
}

#[derive(Args, Debug, Clone)]
pub struct ArktypeArgs {
    /// Output file path (default: arktype.ts in the output's dir)
    #[arg(short = 'o', long)]
    pub file: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct EffectArgs {
    /// Output file path (default: bindings.ts in the output's dir)
    #[arg(short = 'o', long)]
    pub file: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct MacroforgeArgs {
    /// Output file path (default: macroforge.ts in the output's dir)
    #[arg(short = 'o', long)]
    pub file: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct FlatbuffersArgs {
    /// Output file path (default: schema.fbs in the output's dir)
    #[arg(short = 'o', long)]
    pub file: Option<PathBuf>,

    /// Override namespace (e.g., "com.example.app")
    #[arg(long)]
    pub namespace: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct ProtobufArgs {
    /// Output file path (default: schema.proto in the output's dir)
    #[arg(short = 'o', long)]
    pub file: Option<PathBuf>,

    /// Override package name (e.g., "com.example.app")
    #[arg(long)]
    pub package: Option<String>,

    /// Include validate.proto import for validation rules
    #[arg(long)]
    pub import_validate: bool,

    /// Do not include validate.proto import
    #[arg(long, conflicts_with = "import_validate")]
    pub no_import_validate: bool,
}

// ============================================================================
// Schemasync Arguments
// ============================================================================

#[derive(Args, Debug, Clone)]
pub struct SchemasyncArgs {
    #[command(subcommand)]
    pub command: Option<SchemasyncCommands>,

    /// Database URL override
    #[arg(long, env = "SURREALDB_URL")]
    pub url: Option<String>,

    /// Database namespace override
    #[arg(long, env = "SURREALDB_NS")]
    pub namespace: Option<String>,

    /// Database name override
    #[arg(long, env = "SURREALDB_DB")]
    pub database: Option<String>,

    /// Skip mock data generation
    #[arg(long)]
    pub no_mocks: bool,

    /// Force full refresh mode
    #[arg(long)]
    pub full_refresh: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum SchemasyncCommands {
    /// Show schema differences without applying (dry-run)
    Diff(DiffArgs),

    /// Apply schema changes to the database
    Apply(ApplyArgs),

    /// Generate mock data only (skip schema sync)
    Mock(MockArgs),

    /// Dump the resolved schema SurrealQL to a file (offline, no DB connection)
    ///
    /// Without a subcommand, writes everything schemasync defines, in apply
    /// order: accesses, analyzers, tables and functions.
    Dump(DumpArgs),
}

#[derive(Subcommand, Debug, Clone)]
pub enum DumpCommands {
    /// Dump only the table DDL (DEFINE TABLE/FIELD/INDEX/EVENT)
    Tables(DumpTablesArgs),
}

#[derive(Args, Debug, Clone)]
pub struct DiffArgs {
    /// Output format for diff
    #[arg(long, value_enum, default_value = "pretty")]
    pub format: DiffFormat,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum DiffFormat {
    /// Human-readable colored output
    Pretty,
    /// JSON output
    Json,
    /// Plain text
    Plain,
}

#[derive(Args, Debug, Clone)]
pub struct ApplyArgs {
    /// Apply changes without confirmation prompt
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Dry run - show what would be applied
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Args, Debug, Clone)]
pub struct MockArgs {
    /// Number of records to generate per table (overrides config)
    #[arg(long)]
    pub count: Option<usize>,

    /// Specific tables to generate mocks for (comma-separated)
    #[arg(long, value_delimiter = ',')]
    pub tables: Option<Vec<String>>,
}

// ============================================================================
// Mockmake Arguments
// ============================================================================

#[derive(Args, Debug, Clone)]
pub struct MockmakeArgs {
    /// Number of records per table (overrides config and #[mock_data(n)])
    #[arg(long)]
    pub count: Option<usize>,

    /// Only insert into these tables (comma-separated); other tables are only
    /// used as link targets through their existing records
    #[arg(long, value_delimiter = ',')]
    pub tables: Option<Vec<String>>,

    /// Database URL override
    #[arg(long, env = "SURREALDB_URL")]
    pub url: Option<String>,

    /// Database namespace override
    #[arg(long, env = "SURREALDB_NS")]
    pub namespace: Option<String>,

    /// Database name override
    #[arg(long, env = "SURREALDB_DB")]
    pub database: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct DumpArgs {
    #[command(subcommand)]
    pub command: Option<DumpCommands>,

    /// Output file path (default: .evenframe/surql/schema.surql in the project root)
    #[arg(short = 'o', long)]
    pub file: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct DumpTablesArgs {
    /// Output file path (default: .evenframe/surql/tables.surql in the project root)
    #[arg(short = 'o', long)]
    pub file: Option<PathBuf>,
}

// ============================================================================
// Generate Arguments (Full Pipeline)
// ============================================================================

#[derive(Args, Debug, Clone)]
pub struct GenerateArgs {
    /// Skip type generation phase
    #[arg(long)]
    pub skip_typesync: bool,

    /// Skip database sync phase
    #[arg(long)]
    pub skip_schemasync: bool,

    /// Skip mock data generation
    #[arg(long)]
    pub no_mocks: bool,
}

// ============================================================================
// Init Arguments
// ============================================================================

#[derive(Args, Debug, Clone)]
pub struct InitArgs {
    /// Overwrite existing evenframe.toml if present
    #[arg(short, long)]
    pub force: bool,

    /// Database provider to configure
    #[arg(long, value_enum, default_value = "surrealdb")]
    pub provider: DatabaseProvider,

    /// Initialize with minimal configuration
    #[arg(long)]
    pub minimal: bool,
}

// ============================================================================
// Check Arguments
// ============================================================================

#[derive(Args, Debug, Clone)]
pub struct CheckArgs {
    /// Emit the result as JSON for scripted assertions
    #[arg(long)]
    pub json: bool,
}

// ============================================================================
// Validate Arguments
// ============================================================================

#[derive(Args, Debug, Clone)]
pub struct ValidateArgs {
    /// Validate configuration file only
    #[arg(long)]
    pub config_only: bool,

    /// Validate type definitions only
    #[arg(long)]
    pub types_only: bool,

    /// Check database connectivity
    #[arg(long)]
    pub check_db: bool,
}

// ============================================================================
// Info Arguments
// ============================================================================

#[derive(Args, Debug, Clone)]
pub struct InfoArgs {
    /// Show detected Evenframe types
    #[arg(long)]
    pub types: bool,

    /// Show the resolved configuration (the global --config picks the file)
    #[arg(long)]
    pub settings: bool,

    /// Show database schema information
    #[arg(long)]
    pub schema: bool,

    /// Output format
    #[arg(long, value_enum, default_value = "pretty")]
    pub format: InfoFormat,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum InfoFormat {
    Pretty,
    Json,
    Yaml,
}

#[derive(Args, Debug, Clone)]
pub struct TestPluginArgs {
    /// Filter to a specific type name (e.g., "Site", "Order")
    #[arg(long)]
    pub type_name: Option<String>,

    /// Only show types where the plugin produced output
    #[arg(long, default_value = "true")]
    pub changed_only: bool,
}

// ============================================================================
// Expand Arguments
// ============================================================================

#[derive(Args, Debug, Clone)]
pub struct ExpandArgs {
    #[command(subcommand)]
    pub command: ExpandCommands,
}

#[derive(Subcommand, Debug, Clone)]
pub enum ExpandCommands {
    /// Show expansion cache status (per-crate hit/miss counts, total size on disk)
    Status,

    /// Warm the expansion cache by expanding all workspace crates
    Warm,

    /// Clear the expansion cache
    Clear,
}

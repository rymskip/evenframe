//! Command-line interface definitions for Evenframe.

use clap::{Args, Parser, Subcommand, ValueEnum};
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

    /// Enable verbose output (-v, -vv, -vvv for increasing verbosity)
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Suppress all output except errors
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Output path override (overrides config file)
    #[arg(short, long, global = true)]
    pub output: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Source of truth for type definitions
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum SourceOfTruth {
    /// Rust structs with #[derive(Evenframe)] or #[apply(...)]
    Rust,
    /// FlatBuffers schema files (.fbs)
    Flatbuffers,
    /// Protocol Buffers schema files (.proto)
    Protobuf,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Generate TypeScript types and schemas
    Typesync(TypesyncArgs),

    /// Synchronize database schema
    Schemasync(SchemasyncArgs),

    /// Run full generation pipeline (typesync + schemasync)
    Generate(GenerateArgs),

    /// Initialize a new evenframe.toml configuration file
    Init(InitArgs),

    /// Validate configuration and detected types
    Validate(ValidateArgs),

    /// Display information about detected types and configuration
    Info(InfoArgs),
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

    /// Comma-separated list of formats to generate
    #[arg(long, value_delimiter = ',')]
    pub formats: Option<Vec<TypeFormat>>,

    /// Disable specific formats (overrides config)
    #[arg(long, value_delimiter = ',')]
    pub skip: Option<Vec<TypeFormat>>,
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, ValueEnum)]
pub enum TypeFormat {
    Arktype,
    Effect,
    Macroforge,
    Flatbuffers,
    Protobuf,
}

#[derive(Args, Debug, Clone)]
pub struct ArktypeArgs {
    /// Output file path (default: {output_path}/arktype.ts)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct EffectArgs {
    /// Output file path (default: {output_path}/bindings.ts)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct MacroforgeArgs {
    /// Output file path (default: {output_path}/macroforge.ts)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Args, Debug, Clone)]
pub struct FlatbuffersArgs {
    /// Output file path (default: {output_path}/schema.fbs)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Override namespace (e.g., "com.example.app")
    #[arg(long)]
    pub namespace: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct ProtobufArgs {
    /// Output file path (default: {output_path}/schema.proto)
    #[arg(short, long)]
    pub output: Option<PathBuf>,

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

    /// Watch mode - regenerate on file changes
    #[arg(short, long)]
    pub watch: bool,
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum DatabaseProvider {
    Surrealdb,
    Postgres,
    Mysql,
    Sqlite,
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

    /// Show configuration values
    #[arg(long)]
    pub config: bool,

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

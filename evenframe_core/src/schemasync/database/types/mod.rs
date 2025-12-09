//! Shared types for the database provider abstraction layer
//!
//! These types are used across all database providers to represent
//! database-agnostic concepts like record IDs, schema definitions, and relationships.

pub mod mapper;

pub use mapper::TypeMapper;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Universal record identifier that works across all database backends.
///
/// For SurrealDB: formatted as "table:id"
/// For SQL databases: typically just the id value with table context
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RecordId {
    /// The table this record belongs to
    pub table: String,
    /// The unique identifier within the table
    pub id: String,
}

impl RecordId {
    /// Create a new RecordId
    pub fn new(table: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            id: id.into(),
        }
    }

    /// Parse from SurrealDB format "table:id"
    pub fn from_surreal(s: &str) -> Option<Self> {
        let (table, id) = s.split_once(':')?;
        Some(Self::new(table, id))
    }

    /// Format for SurrealDB as "table:id"
    pub fn to_surreal(&self) -> String {
        format!("{}:{}", self.table, self.id)
    }

    /// Get just the ID portion (for SQL databases)
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the table name
    pub fn table(&self) -> &str {
        &self.table
    }
}

impl std::fmt::Display for RecordId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.table, self.id)
    }
}

/// Schema export format - database agnostic representation of a schema
#[derive(Debug, Clone, Default)]
pub struct SchemaExport {
    /// Table definitions
    pub tables: Vec<TableSchema>,
    /// Index definitions
    pub indexes: Vec<IndexSchema>,
    /// Foreign key/relationship definitions
    pub relationships: Vec<RelationshipSchema>,
    /// Raw export statements (for databases that provide them)
    pub raw_statements: Option<String>,
}

/// Abstract table schema definition
#[derive(Debug, Clone)]
pub struct TableSchema {
    /// Table name
    pub name: String,
    /// Column/field definitions
    pub columns: Vec<ColumnSchema>,
    /// Primary key column(s)
    pub primary_key: Vec<String>,
    /// Whether this is a relationship/edge table
    pub is_relation: bool,
    /// Unique constraints
    pub unique_constraints: Vec<Vec<String>>,
    /// Check constraints
    pub check_constraints: Vec<String>,
}

/// Abstract column/field schema definition
#[derive(Debug, Clone)]
pub struct ColumnSchema {
    /// Column name
    pub name: String,
    /// Database-specific type string
    pub data_type: String,
    /// Abstract database type
    pub database_type: DatabaseType,
    /// Whether the column allows NULL values
    pub nullable: bool,
    /// Default value expression
    pub default: Option<String>,
    /// Column constraints
    pub constraints: Vec<ColumnConstraint>,
}

/// Abstract database type enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum DatabaseType {
    // Scalar types
    Boolean,
    Integer { bits: u8, signed: bool },
    Float { bits: u8 },
    Decimal { precision: Option<u8>, scale: Option<u8> },
    String { max_length: Option<u32> },
    Text,
    Binary { max_length: Option<u32> },

    // Temporal types
    Date,
    Time,
    DateTime,
    Timestamp,
    Duration,

    // Complex types
    Json,
    Array(Box<DatabaseType>),
    Record(String), // Reference to another table

    // Database-specific (fallback)
    Custom(String),
}

/// Column constraint types
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnConstraint {
    PrimaryKey,
    NotNull,
    Unique,
    Check(String),
    Default(String),
    ForeignKey {
        table: String,
        column: String,
        on_delete: ForeignKeyAction,
        on_update: ForeignKeyAction,
    },
}

/// Foreign key action on delete/update
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ForeignKeyAction {
    #[default]
    NoAction,
    Restrict,
    Cascade,
    SetNull,
    SetDefault,
}

impl std::fmt::Display for ForeignKeyAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ForeignKeyAction::NoAction => write!(f, "NO ACTION"),
            ForeignKeyAction::Restrict => write!(f, "RESTRICT"),
            ForeignKeyAction::Cascade => write!(f, "CASCADE"),
            ForeignKeyAction::SetNull => write!(f, "SET NULL"),
            ForeignKeyAction::SetDefault => write!(f, "SET DEFAULT"),
        }
    }
}

/// Index schema definition
#[derive(Debug, Clone)]
pub struct IndexSchema {
    /// Index name
    pub name: String,
    /// Table the index belongs to
    pub table: String,
    /// Columns in the index
    pub columns: Vec<String>,
    /// Whether this is a unique index
    pub unique: bool,
    /// Index type (btree, hash, etc.)
    pub index_type: Option<String>,
}

/// Relationship/edge schema definition
#[derive(Debug, Clone)]
pub struct RelationshipSchema {
    /// Relationship/edge table name
    pub name: String,
    /// Source table
    pub from_table: String,
    /// Target table
    pub to_table: String,
    /// Additional properties on the relationship
    pub properties: Vec<ColumnSchema>,
}

/// Table information returned by get_table_info
#[derive(Debug, Clone)]
pub struct TableInfo {
    /// Table name
    pub name: String,
    /// Column information
    pub columns: HashMap<String, ColumnInfo>,
    /// Primary key columns
    pub primary_key: Vec<String>,
    /// Foreign key constraints
    pub foreign_keys: Vec<ForeignKeyInfo>,
    /// Indexes on the table
    pub indexes: Vec<IndexInfo>,
    /// Record count (if available)
    pub row_count: Option<u64>,
}

/// Column information
#[derive(Debug, Clone)]
pub struct ColumnInfo {
    /// Column name
    pub name: String,
    /// Database-native type string
    pub data_type: String,
    /// Whether nullable
    pub nullable: bool,
    /// Default value
    pub default: Option<String>,
    /// Whether this is part of the primary key
    pub is_primary_key: bool,
    /// Maximum length (for string types)
    pub max_length: Option<u32>,
    /// Numeric precision
    pub numeric_precision: Option<u8>,
    /// Numeric scale
    pub numeric_scale: Option<u8>,
}

/// Foreign key information
#[derive(Debug, Clone)]
pub struct ForeignKeyInfo {
    /// Constraint name
    pub name: String,
    /// Local column(s)
    pub columns: Vec<String>,
    /// Referenced table
    pub referenced_table: String,
    /// Referenced column(s)
    pub referenced_columns: Vec<String>,
    /// On delete action
    pub on_delete: ForeignKeyAction,
    /// On update action
    pub on_update: ForeignKeyAction,
}

/// Index information
#[derive(Debug, Clone)]
pub struct IndexInfo {
    /// Index name
    pub name: String,
    /// Columns in the index
    pub columns: Vec<String>,
    /// Whether unique
    pub unique: bool,
    /// Index type
    pub index_type: Option<String>,
}

/// Relationship direction for querying edges
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationshipDirection {
    /// Outgoing relationships (from this record)
    Outgoing,
    /// Incoming relationships (to this record)
    Incoming,
    /// Both directions
    Both,
}

/// A relationship between two records
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    /// Relationship ID (for the join/edge record)
    pub id: String,
    /// Source record ID
    pub from_id: String,
    /// Target record ID
    pub to_id: String,
    /// Additional data on the relationship
    pub data: Option<serde_json::Value>,
}

/// Query response wrapper
#[derive(Debug, Clone)]
pub struct QueryResponse {
    /// Affected row count (for INSERT/UPDATE/DELETE)
    pub affected_rows: Option<u64>,
    /// Returned records (for SELECT)
    pub records: Vec<serde_json::Value>,
    /// Warnings or messages
    pub warnings: Vec<String>,
}

/// Schema change detection types
#[derive(Debug, Clone, Default)]
pub struct ProviderSchemaChanges {
    /// New tables to create
    pub new_tables: Vec<String>,
    /// Tables to remove
    pub removed_tables: Vec<String>,
    /// Modified tables
    pub modified_tables: Vec<TableChanges>,
}

/// Changes to a specific table
#[derive(Debug, Clone)]
pub struct TableChanges {
    /// Table name
    pub table_name: String,
    /// New columns to add
    pub new_columns: Vec<String>,
    /// Columns to remove
    pub removed_columns: Vec<String>,
    /// Modified columns
    pub modified_columns: Vec<ColumnChanges>,
}

/// Changes to a specific column
#[derive(Debug, Clone)]
pub struct ColumnChanges {
    /// Column name
    pub column_name: String,
    /// Old type (if changed)
    pub old_type: Option<String>,
    /// New type (if changed)
    pub new_type: Option<String>,
    /// Nullability changed
    pub nullable_changed: Option<bool>,
    /// Default changed
    pub default_changed: Option<String>,
}

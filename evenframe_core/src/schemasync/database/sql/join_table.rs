//! Join Table Generation for SQL Databases
//!
//! Converts SurrealDB-style edges into SQL join tables with foreign key constraints.

use crate::schemasync::EdgeConfig;
use crate::schemasync::database::types::*;

/// Configuration for generating join tables from edge definitions
pub struct JoinTableConfig {
    /// Whether to use UUIDs for IDs (vs auto-increment integers)
    pub use_uuid: bool,
    /// Database-specific quote character
    pub quote_char: char,
    /// UUID generation expression (database-specific)
    pub uuid_expr: Option<String>,
    /// Timestamp generation expression for created_at
    pub timestamp_expr: String,
}

impl Default for JoinTableConfig {
    fn default() -> Self {
        Self {
            use_uuid: true,
            quote_char: '"',
            uuid_expr: Some("gen_random_uuid()".to_string()),
            timestamp_expr: "NOW()".to_string(),
        }
    }
}

impl JoinTableConfig {
    /// Create config for PostgreSQL
    pub fn postgres() -> Self {
        Self {
            use_uuid: true,
            quote_char: '"',
            uuid_expr: Some("gen_random_uuid()".to_string()),
            timestamp_expr: "NOW()".to_string(),
        }
    }

    /// Create config for MySQL
    pub fn mysql() -> Self {
        Self {
            use_uuid: false, // MySQL uses AUTO_INCREMENT more commonly
            quote_char: '`',
            uuid_expr: Some("UUID()".to_string()),
            timestamp_expr: "NOW()".to_string(),
        }
    }

    /// Create config for SQLite
    pub fn sqlite() -> Self {
        Self {
            use_uuid: false, // SQLite uses INTEGER PRIMARY KEY
            quote_char: '"',
            uuid_expr: None,
            timestamp_expr: "datetime('now')".to_string(),
        }
    }
}

/// Generate a join table schema from an edge configuration
pub fn generate_join_table_schema(
    edge: &EdgeConfig,
    config: &JoinTableConfig,
) -> TableSchema {
    let mut columns = Vec::new();

    // Primary key column
    if config.use_uuid {
        columns.push(ColumnSchema {
            name: "id".to_string(),
            data_type: "UUID".to_string(),
            database_type: DatabaseType::String { max_length: Some(36) },
            nullable: false,
            default: config.uuid_expr.clone(),
            constraints: vec![ColumnConstraint::PrimaryKey],
        });
    } else {
        columns.push(ColumnSchema {
            name: "id".to_string(),
            data_type: "INTEGER".to_string(),
            database_type: DatabaseType::Integer { bits: 64, signed: true },
            nullable: false,
            default: None, // AUTO_INCREMENT handled separately
            constraints: vec![ColumnConstraint::PrimaryKey],
        });
    }

    // Determine the source table(s)
    let from_table = edge.from.first().cloned().unwrap_or_else(|| "unknown".to_string());
    let to_table = edge.to.first().cloned().unwrap_or_else(|| "unknown".to_string());

    // from_id foreign key column
    columns.push(ColumnSchema {
        name: "from_id".to_string(),
        data_type: if config.use_uuid { "UUID" } else { "INTEGER" }.to_string(),
        database_type: if config.use_uuid {
            DatabaseType::String { max_length: Some(36) }
        } else {
            DatabaseType::Integer { bits: 64, signed: true }
        },
        nullable: false,
        default: None,
        constraints: vec![
            ColumnConstraint::NotNull,
            ColumnConstraint::ForeignKey {
                table: from_table.clone(),
                column: "id".to_string(),
                on_delete: ForeignKeyAction::Cascade,
                on_update: ForeignKeyAction::Cascade,
            },
        ],
    });

    // to_id foreign key column
    columns.push(ColumnSchema {
        name: "to_id".to_string(),
        data_type: if config.use_uuid { "UUID" } else { "INTEGER" }.to_string(),
        database_type: if config.use_uuid {
            DatabaseType::String { max_length: Some(36) }
        } else {
            DatabaseType::Integer { bits: 64, signed: true }
        },
        nullable: false,
        default: None,
        constraints: vec![
            ColumnConstraint::NotNull,
            ColumnConstraint::ForeignKey {
                table: to_table.clone(),
                column: "id".to_string(),
                on_delete: ForeignKeyAction::Cascade,
                on_update: ForeignKeyAction::Cascade,
            },
        ],
    });

    // created_at timestamp column
    columns.push(ColumnSchema {
        name: "created_at".to_string(),
        data_type: "TIMESTAMP".to_string(),
        database_type: DatabaseType::Timestamp,
        nullable: false,
        default: Some(config.timestamp_expr.clone()),
        constraints: vec![],
    });

    TableSchema {
        name: edge.edge_name.clone(),
        columns,
        primary_key: vec!["id".to_string()],
        is_relation: true,
        unique_constraints: vec![vec!["from_id".to_string(), "to_id".to_string()]],
        check_constraints: vec![],
    }
}

/// Generate SQL statements for creating a join table
pub fn generate_join_table_sql(
    edge: &EdgeConfig,
    config: &JoinTableConfig,
) -> Vec<String> {
    let q = |name: &str| format!("{}{}{}", config.quote_char, name, config.quote_char);
    let schema = generate_join_table_schema(edge, config);

    let mut statements = Vec::new();

    // Determine source tables for foreign keys
    let from_table = edge.from.first().cloned().unwrap_or_else(|| "unknown".to_string());
    let to_table = edge.to.first().cloned().unwrap_or_else(|| "unknown".to_string());

    // Build CREATE TABLE statement
    let mut create_table = format!("CREATE TABLE IF NOT EXISTS {} (\n", q(&schema.name));

    let mut column_defs = Vec::new();

    for col in &schema.columns {
        let mut def = format!("    {} {}", q(&col.name), col.data_type);

        if col.name == "id" && !config.use_uuid {
            // For non-UUID IDs, add AUTO_INCREMENT or equivalent
            def.push_str(" PRIMARY KEY");
            if config.quote_char == '`' {
                // MySQL syntax
                def.push_str(" AUTO_INCREMENT");
            }
        }

        if !col.nullable && col.name != "id" {
            def.push_str(" NOT NULL");
        }

        if let Some(default) = &col.default && (col.name != "id" || config.use_uuid) {
            def.push_str(&format!(" DEFAULT {}", default));
        }

        column_defs.push(def);
    }

    // Add primary key constraint for UUID IDs
    if config.use_uuid {
        column_defs.push(format!("    PRIMARY KEY ({})", q("id")));
    }

    // Add foreign key constraints
    column_defs.push(format!(
        "    FOREIGN KEY ({}) REFERENCES {}({}) ON DELETE CASCADE ON UPDATE CASCADE",
        q("from_id"), q(&from_table), q("id")
    ));
    column_defs.push(format!(
        "    FOREIGN KEY ({}) REFERENCES {}({}) ON DELETE CASCADE ON UPDATE CASCADE",
        q("to_id"), q(&to_table), q("id")
    ));

    // Add unique constraint
    column_defs.push(format!("    UNIQUE ({}, {})", q("from_id"), q("to_id")));

    create_table.push_str(&column_defs.join(",\n"));
    create_table.push_str("\n);");
    statements.push(create_table);

    // Add indexes for efficient lookups
    statements.push(format!(
        "CREATE INDEX IF NOT EXISTS {} ON {} ({});",
        q(&format!("idx_{}_from_id", schema.name)),
        q(&schema.name),
        q("from_id")
    ));

    statements.push(format!(
        "CREATE INDEX IF NOT EXISTS {} ON {} ({});",
        q(&format!("idx_{}_to_id", schema.name)),
        q(&schema.name),
        q("to_id")
    ));

    statements
}

/// Generate INSERT statement for adding a relationship
pub fn generate_relationship_insert(
    edge_table: &str,
    from_id: &str,
    to_id: &str,
    additional_data: Option<&serde_json::Value>,
    config: &JoinTableConfig,
) -> String {
    let q = |name: &str| format!("{}{}{}", config.quote_char, name, config.quote_char);

    let mut columns = vec!["from_id", "to_id"];
    let mut values = vec![
        format!("'{}'", from_id),
        format!("'{}'", to_id),
    ];

    if let Some(data) = additional_data && let Some(obj) = data.as_object() {
        for (key, value) in obj {
            if key != "id" && key != "from_id" && key != "to_id" && key != "created_at" {
                columns.push(key);
                values.push(format_value_for_sql(value));
            }
        }
    }

    let cols: Vec<String> = columns.iter().map(|c| q(c)).collect();

    format!(
        "INSERT INTO {} ({}) VALUES ({});",
        q(edge_table),
        cols.join(", "),
        values.join(", ")
    )
}

/// Format a JSON value for SQL
fn format_value_for_sql(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        _ => format!("'{}'", value.to_string().replace('\'', "''")),
    }
}

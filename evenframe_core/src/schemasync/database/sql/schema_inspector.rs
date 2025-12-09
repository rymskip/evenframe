//! Schema Inspector for SQL Databases
//!
//! Provides utilities for introspecting database schemas using information_schema
//! queries (PostgreSQL, MySQL) or PRAGMA statements (SQLite).

use crate::schemasync::database::types::*;

/// Trait for schema inspection operations
pub trait SchemaInspector {
    /// Get the SQL query to list all tables
    fn list_tables_query(&self) -> String;

    /// Get the SQL query to list columns for a table
    fn list_columns_query(&self, table_name: &str) -> String;

    /// Get the SQL query to list indexes for a table
    fn list_indexes_query(&self, table_name: &str) -> String;

    /// Get the SQL query to list foreign keys for a table
    fn list_foreign_keys_query(&self, table_name: &str) -> String;

    /// Parse a row from the tables query into a table name
    fn parse_table_row(&self, row: &serde_json::Value) -> Option<String>;

    /// Parse a row from the columns query into ColumnInfo
    fn parse_column_row(&self, row: &serde_json::Value) -> Option<ColumnInfo>;

    /// Parse a row from the indexes query into IndexInfo
    fn parse_index_row(&self, row: &serde_json::Value) -> Option<IndexInfo>;

    /// Parse a row from the foreign keys query into ForeignKeyInfo
    fn parse_foreign_key_row(&self, row: &serde_json::Value) -> Option<ForeignKeyInfo>;
}

/// PostgreSQL schema inspector
pub struct PostgresSchemaInspector {
    pub schema: String,
}

impl PostgresSchemaInspector {
    pub fn new(schema: &str) -> Self {
        Self {
            schema: schema.to_string(),
        }
    }
}

impl SchemaInspector for PostgresSchemaInspector {
    fn list_tables_query(&self) -> String {
        format!(
            r#"
            SELECT table_name
            FROM information_schema.tables
            WHERE table_schema = '{}'
              AND table_type = 'BASE TABLE'
            ORDER BY table_name
            "#,
            self.schema
        )
    }

    fn list_columns_query(&self, table_name: &str) -> String {
        format!(
            r#"
            SELECT
                column_name,
                data_type,
                is_nullable,
                column_default,
                character_maximum_length,
                numeric_precision,
                numeric_scale
            FROM information_schema.columns
            WHERE table_schema = '{}'
              AND table_name = '{}'
            ORDER BY ordinal_position
            "#,
            self.schema, table_name
        )
    }

    fn list_indexes_query(&self, table_name: &str) -> String {
        format!(
            r#"
            SELECT
                i.relname as index_name,
                array_agg(a.attname ORDER BY array_position(ix.indkey, a.attnum)) as columns,
                ix.indisunique as is_unique,
                am.amname as index_type
            FROM pg_index ix
            JOIN pg_class i ON i.oid = ix.indexrelid
            JOIN pg_class t ON t.oid = ix.indrelid
            JOIN pg_namespace n ON n.oid = t.relnamespace
            JOIN pg_am am ON am.oid = i.relam
            JOIN pg_attribute a ON a.attrelid = t.oid AND a.attnum = ANY(ix.indkey)
            WHERE t.relname = '{}'
              AND n.nspname = '{}'
              AND NOT ix.indisprimary
            GROUP BY i.relname, ix.indisunique, am.amname
            "#,
            table_name, self.schema
        )
    }

    fn list_foreign_keys_query(&self, table_name: &str) -> String {
        format!(
            r#"
            SELECT
                tc.constraint_name,
                kcu.column_name,
                ccu.table_name AS foreign_table_name,
                ccu.column_name AS foreign_column_name,
                rc.delete_rule,
                rc.update_rule
            FROM information_schema.table_constraints AS tc
            JOIN information_schema.key_column_usage AS kcu
                ON tc.constraint_name = kcu.constraint_name
                AND tc.table_schema = kcu.table_schema
            JOIN information_schema.constraint_column_usage AS ccu
                ON ccu.constraint_name = tc.constraint_name
                AND ccu.table_schema = tc.table_schema
            JOIN information_schema.referential_constraints AS rc
                ON rc.constraint_name = tc.constraint_name
                AND rc.constraint_schema = tc.table_schema
            WHERE tc.constraint_type = 'FOREIGN KEY'
              AND tc.table_name = '{}'
              AND tc.table_schema = '{}'
            "#,
            table_name, self.schema
        )
    }

    fn parse_table_row(&self, row: &serde_json::Value) -> Option<String> {
        row.get("table_name")?.as_str().map(|s| s.to_string())
    }

    fn parse_column_row(&self, row: &serde_json::Value) -> Option<ColumnInfo> {
        Some(ColumnInfo {
            name: row.get("column_name")?.as_str()?.to_string(),
            data_type: row.get("data_type")?.as_str()?.to_string(),
            nullable: row.get("is_nullable")?.as_str()? == "YES",
            default: row.get("column_default").and_then(|v| v.as_str()).map(|s| s.to_string()),
            is_primary_key: false, // Will be set separately
            max_length: row.get("character_maximum_length").and_then(|v| v.as_u64()).map(|v| v as u32),
            numeric_precision: row.get("numeric_precision").and_then(|v| v.as_u64()).map(|v| v as u8),
            numeric_scale: row.get("numeric_scale").and_then(|v| v.as_u64()).map(|v| v as u8),
        })
    }

    fn parse_index_row(&self, row: &serde_json::Value) -> Option<IndexInfo> {
        let columns = row.get("columns")?
            .as_array()?
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();

        Some(IndexInfo {
            name: row.get("index_name")?.as_str()?.to_string(),
            columns,
            unique: row.get("is_unique")?.as_bool()?,
            index_type: row.get("index_type").and_then(|v| v.as_str()).map(|s| s.to_string()),
        })
    }

    fn parse_foreign_key_row(&self, row: &serde_json::Value) -> Option<ForeignKeyInfo> {
        let delete_rule = row.get("delete_rule")
            .and_then(|v| v.as_str())
            .map(parse_fk_action)
            .unwrap_or_default();

        let update_rule = row.get("update_rule")
            .and_then(|v| v.as_str())
            .map(parse_fk_action)
            .unwrap_or_default();

        Some(ForeignKeyInfo {
            name: row.get("constraint_name")?.as_str()?.to_string(),
            columns: vec![row.get("column_name")?.as_str()?.to_string()],
            referenced_table: row.get("foreign_table_name")?.as_str()?.to_string(),
            referenced_columns: vec![row.get("foreign_column_name")?.as_str()?.to_string()],
            on_delete: delete_rule,
            on_update: update_rule,
        })
    }
}

/// MySQL schema inspector
pub struct MysqlSchemaInspector {
    pub database: String,
}

impl MysqlSchemaInspector {
    pub fn new(database: &str) -> Self {
        Self {
            database: database.to_string(),
        }
    }
}

impl SchemaInspector for MysqlSchemaInspector {
    fn list_tables_query(&self) -> String {
        format!(
            r#"
            SELECT table_name
            FROM information_schema.tables
            WHERE table_schema = '{}'
              AND table_type = 'BASE TABLE'
            ORDER BY table_name
            "#,
            self.database
        )
    }

    fn list_columns_query(&self, table_name: &str) -> String {
        format!(
            r#"
            SELECT
                column_name,
                data_type,
                is_nullable,
                column_default,
                character_maximum_length,
                numeric_precision,
                numeric_scale,
                column_type
            FROM information_schema.columns
            WHERE table_schema = '{}'
              AND table_name = '{}'
            ORDER BY ordinal_position
            "#,
            self.database, table_name
        )
    }

    fn list_indexes_query(&self, table_name: &str) -> String {
        format!(
            r#"
            SELECT
                index_name,
                GROUP_CONCAT(column_name ORDER BY seq_in_index) as columns,
                NOT non_unique as is_unique,
                index_type
            FROM information_schema.statistics
            WHERE table_schema = '{}'
              AND table_name = '{}'
              AND index_name != 'PRIMARY'
            GROUP BY index_name, non_unique, index_type
            "#,
            self.database, table_name
        )
    }

    fn list_foreign_keys_query(&self, table_name: &str) -> String {
        format!(
            r#"
            SELECT
                constraint_name,
                column_name,
                referenced_table_name,
                referenced_column_name
            FROM information_schema.key_column_usage
            WHERE table_schema = '{}'
              AND table_name = '{}'
              AND referenced_table_name IS NOT NULL
            "#,
            self.database, table_name
        )
    }

    fn parse_table_row(&self, row: &serde_json::Value) -> Option<String> {
        row.get("table_name")
            .or_else(|| row.get("TABLE_NAME"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    fn parse_column_row(&self, row: &serde_json::Value) -> Option<ColumnInfo> {
        Some(ColumnInfo {
            name: row.get("column_name")
                .or_else(|| row.get("COLUMN_NAME"))
                ?.as_str()?.to_string(),
            data_type: row.get("data_type")
                .or_else(|| row.get("DATA_TYPE"))
                ?.as_str()?.to_string(),
            nullable: row.get("is_nullable")
                .or_else(|| row.get("IS_NULLABLE"))
                ?.as_str()? == "YES",
            default: row.get("column_default")
                .or_else(|| row.get("COLUMN_DEFAULT"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            is_primary_key: false,
            max_length: row.get("character_maximum_length")
                .or_else(|| row.get("CHARACTER_MAXIMUM_LENGTH"))
                .and_then(|v| v.as_u64())
                .map(|v| v as u32),
            numeric_precision: row.get("numeric_precision")
                .or_else(|| row.get("NUMERIC_PRECISION"))
                .and_then(|v| v.as_u64())
                .map(|v| v as u8),
            numeric_scale: row.get("numeric_scale")
                .or_else(|| row.get("NUMERIC_SCALE"))
                .and_then(|v| v.as_u64())
                .map(|v| v as u8),
        })
    }

    fn parse_index_row(&self, row: &serde_json::Value) -> Option<IndexInfo> {
        let columns_str = row.get("columns")?.as_str()?;
        let columns: Vec<String> = columns_str.split(',').map(|s| s.trim().to_string()).collect();

        Some(IndexInfo {
            name: row.get("index_name")?.as_str()?.to_string(),
            columns,
            unique: row.get("is_unique")?.as_bool().unwrap_or(false),
            index_type: row.get("index_type").and_then(|v| v.as_str()).map(|s| s.to_string()),
        })
    }

    fn parse_foreign_key_row(&self, row: &serde_json::Value) -> Option<ForeignKeyInfo> {
        Some(ForeignKeyInfo {
            name: row.get("constraint_name")?.as_str()?.to_string(),
            columns: vec![row.get("column_name")?.as_str()?.to_string()],
            referenced_table: row.get("referenced_table_name")?.as_str()?.to_string(),
            referenced_columns: vec![row.get("referenced_column_name")?.as_str()?.to_string()],
            on_delete: ForeignKeyAction::NoAction,
            on_update: ForeignKeyAction::NoAction,
        })
    }
}

/// SQLite schema inspector
pub struct SqliteSchemaInspector;

impl SqliteSchemaInspector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for SqliteSchemaInspector {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaInspector for SqliteSchemaInspector {
    fn list_tables_query(&self) -> String {
        r#"
        SELECT name as table_name
        FROM sqlite_master
        WHERE type = 'table'
          AND name NOT LIKE 'sqlite_%'
        ORDER BY name
        "#.to_string()
    }

    fn list_columns_query(&self, table_name: &str) -> String {
        format!("PRAGMA table_info({})", table_name)
    }

    fn list_indexes_query(&self, table_name: &str) -> String {
        format!("PRAGMA index_list({})", table_name)
    }

    fn list_foreign_keys_query(&self, table_name: &str) -> String {
        format!("PRAGMA foreign_key_list({})", table_name)
    }

    fn parse_table_row(&self, row: &serde_json::Value) -> Option<String> {
        row.get("table_name")
            .or_else(|| row.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    fn parse_column_row(&self, row: &serde_json::Value) -> Option<ColumnInfo> {
        Some(ColumnInfo {
            name: row.get("name")?.as_str()?.to_string(),
            data_type: row.get("type")?.as_str()?.to_string(),
            nullable: row.get("notnull")?.as_i64()? == 0,
            default: row.get("dflt_value").and_then(|v| v.as_str()).map(|s| s.to_string()),
            is_primary_key: row.get("pk")?.as_i64()? == 1,
            max_length: None,
            numeric_precision: None,
            numeric_scale: None,
        })
    }

    fn parse_index_row(&self, row: &serde_json::Value) -> Option<IndexInfo> {
        Some(IndexInfo {
            name: row.get("name")?.as_str()?.to_string(),
            columns: vec![], // Would need a separate PRAGMA index_info call
            unique: row.get("unique")?.as_i64()? == 1,
            index_type: None,
        })
    }

    fn parse_foreign_key_row(&self, row: &serde_json::Value) -> Option<ForeignKeyInfo> {
        Some(ForeignKeyInfo {
            name: format!("fk_{}", row.get("id")?.as_i64()?),
            columns: vec![row.get("from")?.as_str()?.to_string()],
            referenced_table: row.get("table")?.as_str()?.to_string(),
            referenced_columns: vec![row.get("to")?.as_str()?.to_string()],
            on_delete: row.get("on_delete")
                .and_then(|v| v.as_str())
                .map(parse_fk_action)
                .unwrap_or_default(),
            on_update: row.get("on_update")
                .and_then(|v| v.as_str())
                .map(parse_fk_action)
                .unwrap_or_default(),
        })
    }
}

/// Parse a foreign key action string into ForeignKeyAction
fn parse_fk_action(s: &str) -> ForeignKeyAction {
    match s.to_uppercase().as_str() {
        "CASCADE" => ForeignKeyAction::Cascade,
        "SET NULL" => ForeignKeyAction::SetNull,
        "SET DEFAULT" => ForeignKeyAction::SetDefault,
        "RESTRICT" => ForeignKeyAction::Restrict,
        _ => ForeignKeyAction::NoAction,
    }
}

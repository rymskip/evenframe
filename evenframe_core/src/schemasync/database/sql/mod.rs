//! SQL Database Providers
//!
//! This module provides implementations of the DatabaseProvider trait for
//! SQL databases: PostgreSQL, MySQL, and SQLite.

#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(feature = "mysql")]
pub mod mysql;

#[cfg(feature = "sqlite")]
pub mod sqlite;

mod schema_inspector;
mod join_table;
mod type_mapper;

pub use schema_inspector::*;
pub use join_table::*;
pub use type_mapper::*;

use crate::schemasync::database::types::*;

/// Common SQL query builder utilities
pub struct SqlQueryBuilder;

impl SqlQueryBuilder {
    /// Generate a CREATE TABLE statement
    pub fn create_table(
        table: &TableSchema,
        quote_char: char,
    ) -> String {
        let q = |name: &str| format!("{}{}{}", quote_char, name, quote_char);

        let mut sql = format!("CREATE TABLE IF NOT EXISTS {} (\n", q(&table.name));

        let mut column_defs: Vec<String> = table.columns.iter().map(|col| {
            let mut def = format!("    {} {}", q(&col.name), &col.data_type);
            if !col.nullable {
                def.push_str(" NOT NULL");
            }
            if let Some(default) = &col.default {
                def.push_str(&format!(" DEFAULT {}", default));
            }
            def
        }).collect();

        // Add primary key constraint
        if !table.primary_key.is_empty() {
            let pk_cols: Vec<String> = table.primary_key.iter().map(|c| q(c)).collect();
            column_defs.push(format!("    PRIMARY KEY ({})", pk_cols.join(", ")));
        }

        // Add unique constraints
        for unique_cols in &table.unique_constraints {
            let cols: Vec<String> = unique_cols.iter().map(|c| q(c)).collect();
            column_defs.push(format!("    UNIQUE ({})", cols.join(", ")));
        }

        sql.push_str(&column_defs.join(",\n"));
        sql.push_str("\n);");

        sql
    }

    /// Generate ALTER TABLE ADD COLUMN statement
    pub fn add_column(
        table_name: &str,
        column: &ColumnSchema,
        quote_char: char,
    ) -> String {
        let q = |name: &str| format!("{}{}{}", quote_char, name, quote_char);

        let mut sql = format!(
            "ALTER TABLE {} ADD COLUMN {} {}",
            q(table_name),
            q(&column.name),
            &column.data_type
        );

        if !column.nullable {
            sql.push_str(" NOT NULL");
        }
        if let Some(default) = &column.default {
            sql.push_str(&format!(" DEFAULT {}", default));
        }

        sql.push(';');
        sql
    }

    /// Generate DROP COLUMN statement
    pub fn drop_column(
        table_name: &str,
        column_name: &str,
        quote_char: char,
    ) -> String {
        let q = |name: &str| format!("{}{}{}", quote_char, name, quote_char);
        format!(
            "ALTER TABLE {} DROP COLUMN IF EXISTS {};",
            q(table_name),
            q(column_name)
        )
    }

    /// Generate CREATE INDEX statement
    pub fn create_index(
        index: &IndexSchema,
        quote_char: char,
    ) -> String {
        let q = |name: &str| format!("{}{}{}", quote_char, name, quote_char);

        let unique = if index.unique { "UNIQUE " } else { "" };
        let cols: Vec<String> = index.columns.iter().map(|c| q(c)).collect();

        format!(
            "CREATE {}INDEX IF NOT EXISTS {} ON {} ({});",
            unique,
            q(&index.name),
            q(&index.table),
            cols.join(", ")
        )
    }

    /// Generate DROP INDEX statement
    pub fn drop_index(
        index_name: &str,
        table_name: &str,
        quote_char: char,
    ) -> String {
        let q = |name: &str| format!("{}{}{}", quote_char, name, quote_char);
        format!("DROP INDEX IF EXISTS {} ON {};", q(index_name), q(table_name))
    }

    /// Generate INSERT statement
    pub fn insert(
        table_name: &str,
        columns: &[&str],
        quote_char: char,
    ) -> String {
        let q = |name: &str| format!("{}{}{}", quote_char, name, quote_char);
        let cols: Vec<String> = columns.iter().map(|c| q(c)).collect();
        let placeholders: Vec<String> = (1..=columns.len())
            .map(|i| format!("${}", i))
            .collect();

        format!(
            "INSERT INTO {} ({}) VALUES ({});",
            q(table_name),
            cols.join(", "),
            placeholders.join(", ")
        )
    }

    /// Generate SELECT statement
    pub fn select(
        table_name: &str,
        columns: Option<&[&str]>,
        filter: Option<&str>,
        quote_char: char,
    ) -> String {
        let q = |name: &str| format!("{}{}{}", quote_char, name, quote_char);

        let cols = match columns {
            Some(cols) => cols.iter().map(|c| q(c)).collect::<Vec<_>>().join(", "),
            None => "*".to_string(),
        };

        let mut sql = format!("SELECT {} FROM {}", cols, q(table_name));

        if let Some(f) = filter {
            sql.push_str(&format!(" WHERE {}", f));
        }

        sql.push(';');
        sql
    }

    /// Generate DELETE statement
    pub fn delete(
        table_name: &str,
        filter: Option<&str>,
        quote_char: char,
    ) -> String {
        let q = |name: &str| format!("{}{}{}", quote_char, name, quote_char);

        let mut sql = format!("DELETE FROM {}", q(table_name));

        if let Some(f) = filter {
            sql.push_str(&format!(" WHERE {}", f));
        }

        sql.push(';');
        sql
    }

    /// Generate COUNT query
    pub fn count(
        table_name: &str,
        filter: Option<&str>,
        quote_char: char,
    ) -> String {
        let q = |name: &str| format!("{}{}{}", quote_char, name, quote_char);

        let mut sql = format!("SELECT COUNT(*) as count FROM {}", q(table_name));

        if let Some(f) = filter {
            sql.push_str(&format!(" WHERE {}", f));
        }

        sql.push(';');
        sql
    }
}

/// Escape a string value for SQL (double single quotes)
pub fn escape_sql_string(value: &str) -> String {
    value.replace('\'', "''")
}

/// Format a JSON value as a SQL literal
pub fn format_sql_literal(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("'{}'", escape_sql_string(s)),
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(format_sql_literal).collect();
            format!("[{}]", items.join(", "))
        }
        serde_json::Value::Object(_) => {
            // For objects, use JSON format
            format!("'{}'", escape_sql_string(&value.to_string()))
        }
    }
}

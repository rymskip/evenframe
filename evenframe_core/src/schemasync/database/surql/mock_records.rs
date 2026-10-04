use crate::{
    error::{EvenframeError, Result},
    schemasync::database::surql::execute::RPC_SIZE_LIMIT,
    schemasync::mockmake::unique::{PlannedRecord, PlannedValue},
    schemasync::mockmake::{Mockmaker, TableMocks, field_value::FieldValueGenerator},
    schemasync::table::{TableConfig, surql_ident},
    types::{FieldType, StructField},
};
use std::fmt::Write;
use tracing::{debug, info};

fn write_failed(error: std::fmt::Error) -> EvenframeError {
    EvenframeError::mock_generation(format!("writing mock statements failed: {error}"))
}

/// `records` grouped so each group's `INSERT` statement, with `overhead`
/// bytes of its own, stays under the request size limit. A record larger
/// than that on its own is a group by itself.
fn insert_chunks(records: &[String], overhead: usize) -> Vec<&[String]> {
    let budget = RPC_SIZE_LIMIT.saturating_sub(overhead + 32);
    let mut chunks = Vec::new();
    let mut start = 0;
    let mut size = 0;
    for (position, record) in records.iter().enumerate() {
        if position > start && size + record.len() + 2 > budget {
            chunks.push(&records[start..position]);
            start = position;
            size = 0;
        }
        size += record.len() + 2;
    }
    if start < records.len() {
        chunks.push(&records[start..]);
    }
    chunks
}

impl Mockmaker<'_> {
    /// The statements that write `mocks` to `table_name`: existing records
    /// get their rewritten fields replaced whole, and new records are
    /// inserted with every field, as many per statement as fit a request.
    /// Every record keeps within the table's unique indexes.
    pub async fn generate_mock_statements(
        &self,
        table_name: &str,
        mocks: &TableMocks,
    ) -> Result<String> {
        info!(table_name = %table_name, "Generating mock statements for table");
        debug!("Table mocks: {:?}", mocks);
        let table = self.table(table_name)?;
        let mut records = self.plan_records(table_name, table, mocks)?;
        self.keep_unique(table_name, table, &mut records).await?;

        let mut output = String::new();
        let mut inserted = Vec::new();
        for record in &records {
            if record.existing {
                let assignments = record
                    .values
                    .iter()
                    .map(|(name, value)| {
                        let name = surql_ident(name);
                        match value {
                            PlannedValue::Set(literal) => format!("{name} = {literal}"),
                            PlannedValue::UnlessNull(literal) => {
                                format!("{name} = (IF {name} != NULL THEN {literal} ELSE NULL END)")
                            }
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(output, "UPDATE {} SET {assignments};", record.id)
                    .map_err(write_failed)?;
            } else {
                let fields = record
                    .values
                    .iter()
                    .map(|(name, value)| format!("{}: {}", surql_ident(name), value.literal()))
                    .collect::<Vec<_>>()
                    .join(", ");
                inserted.push(format!("{{ id: {}, {fields} }}", record.id));
            }
        }
        let into = if table.relation.is_some() {
            "INSERT RELATION INTO"
        } else {
            "INSERT INTO"
        };
        for chunk in insert_chunks(&inserted, into.len() + table_name.len()) {
            writeln!(
                output,
                "{into} {table_name} [{}] RETURN NONE;",
                chunk.join(", ")
            )
            .map_err(write_failed)?;
        }
        Ok(output)
    }

    /// The values this run writes: the rewritten fields of the existing
    /// records, then every written field of the new ones.
    fn plan_records(
        &self,
        table_name: &str,
        table: &TableConfig,
        mocks: &TableMocks,
    ) -> Result<Vec<PlannedRecord>> {
        let ids = self.id_map.get(table_name).map_or(&[][..], Vec::as_slice);
        let existing = ids.len().saturating_sub(mocks.new_records);
        let mut records = Vec::new();

        if !mocks.rewrite_fields.is_empty() {
            for (position, record_id) in ids.iter().enumerate().take(existing) {
                let values = self.rewrite_values(table, &mocks.rewrite_fields, position)?;
                if !values.is_empty() {
                    records.push(PlannedRecord {
                        id: record_id.clone(),
                        position,
                        existing: true,
                        values,
                    });
                }
            }
        }

        for (position, default_id) in ids.iter().enumerate().skip(existing) {
            #[cfg(feature = "wasm-plugins")]
            let record_id =
                self.plugin_record_id(table_name, table, position, default_id, ids.len())?;
            #[cfg(not(feature = "wasm-plugins"))]
            let record_id = default_id.clone();
            let values = table
                .struct_config
                .fields
                .iter()
                .filter(|field| field.is_mock_written())
                .map(|field| {
                    Ok((
                        field.db_name().to_owned(),
                        PlannedValue::Set(self.generate_value(table, field, position)?),
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            records.push(PlannedRecord {
                id: record_id,
                position,
                existing: false,
                values,
            });
        }
        Ok(records)
    }

    /// A value for `field` of the record at `position` in the id pool.
    pub(crate) fn generate_value(
        &self,
        table: &TableConfig,
        field: &StructField,
        position: usize,
    ) -> Result<String> {
        FieldValueGenerator::builder()
            .field(field)
            .id_index(&position)
            .mockmaker(self)
            .table_config(table)
            .registry(self.registry)
            .build()
            .run()
    }

    /// A present value for the optional `field`.
    pub(crate) fn generate_present(
        &self,
        table: &TableConfig,
        field: &StructField,
        position: usize,
    ) -> Result<String> {
        let FieldType::Option(inner) = &field.field_type else {
            return self.generate_value(table, field, position);
        };
        let present = StructField {
            field_type: inner.as_ref().clone(),
            ..field.clone()
        };
        self.generate_value(table, &present, position)
    }

    /// The rewritten values of the existing record at `position` in the id
    /// pool. SET replaces a value whole, where MERGE would keep an object's
    /// stale keys. An optional field that is NULL stays NULL, and a removed
    /// field (typed `Unit`) is set to NONE, which unsets it.
    fn rewrite_values(
        &self,
        table: &TableConfig,
        rewrite_fields: &[StructField],
        position: usize,
    ) -> Result<Vec<(String, PlannedValue)>> {
        rewrite_fields
            .iter()
            .filter(|field| field.is_mock_written())
            // A relation's endpoints are fixed once it exists.
            .filter(|field| table.relation.is_none() || !matches!(field.db_name(), "in" | "out"))
            .map(|field| {
                let value = match &field.field_type {
                    // A present value is rewritten with a present one,
                    // unless no present value can be generated.
                    FieldType::Option(inner) if !self.has_unfillable_link(inner) => {
                        PlannedValue::UnlessNull(self.generate_present(table, field, position)?)
                    }
                    _ => PlannedValue::Set(self.generate_value(table, field, position)?),
                };
                Ok((field.db_name().to_owned(), value))
            })
            .collect()
    }

    /// The id of a new record: the pool's id, unless the table's mock plugin
    /// supplies one. A plugin that skips the id keeps the pool's.
    #[cfg(feature = "wasm-plugins")]
    fn plugin_record_id(
        &self,
        table_name: &str,
        table: &TableConfig,
        index: usize,
        default_id: &str,
        total_records: usize,
    ) -> Result<String> {
        let Some(plugin_name) = table
            .mock_generation_config
            .as_ref()
            .and_then(|config| config.plugin.as_ref())
        else {
            return Ok(default_id.to_string());
        };
        let pm_cell = self.plugin_manager.as_ref().ok_or_else(|| {
            crate::error::EvenframeError::mock_generation(format!(
                "`{table_name}` uses mock-data plugin `{plugin_name}`, but no plugins were loaded"
            ))
        })?;
        let input = crate::schemasync::mockmake::plugin_types::PluginFieldInput {
            table_name: table_name.to_string(),
            field_name: "id".to_string(),
            field_type: "RecordId".to_string(),
            record_index: index,
            total_records,
            record_id: default_id.to_string(),
        };
        let id = pm_cell
            .borrow_mut()
            .generate_field_value(plugin_name, &input)
            .map_err(|error| {
                crate::error::EvenframeError::mock_generation(format!(
                    "`{table_name}` record id: {error}"
                ))
            })?;
        Ok(id.unwrap_or_else(|| default_id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::{RPC_SIZE_LIMIT, insert_chunks};

    #[test]
    fn inserts_stay_under_the_request_limit_in_order() {
        let record = "x".repeat(RPC_SIZE_LIMIT / 4);
        let records: Vec<String> = (0..10)
            .map(|position| format!("{position}{record}"))
            .collect();
        let chunks = insert_chunks(&records, 40);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(
            |chunk| chunk.iter().map(|entry| entry.len() + 2).sum::<usize>() < RPC_SIZE_LIMIT
        ));
        let flattened: Vec<&String> = chunks.iter().flat_map(|chunk| chunk.iter()).collect();
        assert_eq!(flattened, records.iter().collect::<Vec<_>>());
    }

    #[test]
    fn a_record_larger_than_a_request_is_inserted_alone() {
        let records = vec![
            "a".to_string(),
            "b".repeat(RPC_SIZE_LIMIT + 1),
            "c".to_string(),
        ];
        let chunks = insert_chunks(&records, 40);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[1].len(), 1);
    }
}

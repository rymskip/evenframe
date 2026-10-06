use crate::{
    error::{EvenframeError, Result},
    schemasync::database::surql::execute::RPC_SIZE_LIMIT,
    schemasync::mockmake::unique::{PlannedRecord, PlannedValue},
    schemasync::mockmake::{Mockmaker, TableMocks, field_value::FieldValueGenerator},
    schemasync::table::{TableConfig, surql_ident},
    types::{EnumRepresentation, FieldType, StructField, TaggedUnion, VariantData},
};
use rand::{RngExt, seq::IndexedRandom};
use std::fmt::Write;
use tracing::{debug, info};

fn write_failed(error: std::fmt::Error) -> EvenframeError {
    EvenframeError::mock_generation(format!("writing mock statements failed: {error}"))
}

/// A SurrealQL object literal of `values`.
fn object_literal(values: &[(String, PlannedValue)]) -> String {
    let entries: Vec<String> = values
        .iter()
        .map(|(name, value)| format!("{}: {}", surql_ident(name), value.literal()))
        .collect();
    format!("{{ {} }}", entries.join(", "))
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
                            PlannedValue::UnlessAbsent(literal) => {
                                format!("{name} = (IF {name} != NONE AND {name} != NULL THEN {literal} ELSE {} END)", self.schemasync_config.option_none.literal())
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
            let mut values = Vec::new();
            for field in table
                .struct_config
                .fields
                .iter()
                .filter(|field| field.is_mock_written())
            {
                if field.effective().wire.serde_flatten {
                    values.extend(self.flattened_values(table, field, position)?);
                } else {
                    values.push((
                        field.db_name().to_owned(),
                        PlannedValue::Set(self.generate_value(table, field, position)?),
                    ));
                }
            }
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

    /// The keys a flattened field writes beside the record's own: none for a
    /// map, which may be empty, or an absent `Option`, and for an enum one
    /// variant's keys as its representation writes them.
    fn flattened_values(
        &self,
        table: &TableConfig,
        field: &StructField,
        position: usize,
    ) -> Result<Vec<(String, PlannedValue)>> {
        let mut rng = rand::rng();
        let held = match &field.field_type {
            FieldType::Option(_) if rng.random_bool(0.5) => return Ok(Vec::new()),
            FieldType::Option(inner) => inner.as_ref(),
            held => held,
        };
        let FieldType::Other(name) = held else {
            return Ok(Vec::new());
        };
        let Some(tagged_union) = self.enums.get(name).map(TaggedUnion::effective) else {
            return Ok(Vec::new());
        };
        // A unit variant leaves keys beside the record's only under a tag.
        let candidates: Vec<_> = tagged_union
            .variants
            .iter()
            .map(|variant| variant.effective())
            .filter(|variant| {
                variant.data.is_some()
                    || matches!(
                        variant.stored_representation(&tagged_union.representation),
                        EnumRepresentation::InternallyTagged { .. }
                            | EnumRepresentation::AdjacentlyTagged { .. }
                    )
            })
            .collect();
        let variant = candidates.choose(&mut rng).ok_or_else(|| {
            EvenframeError::mock_generation(format!(
                "`{}.{}` flattens the enum `{name}`, none of whose variants serde writes as keys \
                 beside others",
                table.table_name, field.field_name
            ))
        })?;
        let tag_value = PlannedValue::Set(format!("'{}'", variant.db_name()));
        let value_of = |field_type: &FieldType| {
            self.generate_value(
                table,
                &StructField {
                    field_name: field.field_name.clone(),
                    field_type: field_type.clone(),
                    ..StructField::default()
                },
                position,
            )
        };
        let payload_fields = |data: &VariantData| -> Result<Vec<(String, PlannedValue)>> {
            let fields: &[StructField] = match data {
                VariantData::InlineStruct(inline) => &inline.effective().fields,
                VariantData::DataStructureRef(FieldType::Other(held)) => self
                    .objects
                    .get(held)
                    .map(|object| object.effective().fields.as_slice())
                    .ok_or_else(|| self.not_an_object(table, field, name))?,
                VariantData::DataStructureRef(_) => {
                    return Err(self.not_an_object(table, field, name));
                }
            };
            fields
                .iter()
                .map(StructField::effective)
                .filter(|held_field| held_field.is_mock_written())
                .map(|held_field| {
                    Ok((
                        held_field.db_name().to_owned(),
                        PlannedValue::Set(self.generate_value(table, held_field, position)?),
                    ))
                })
                .collect()
        };
        let payload_value = |data: &VariantData| -> Result<String> {
            match data {
                VariantData::InlineStruct(_) => Ok(object_literal(&payload_fields(data)?)),
                VariantData::DataStructureRef(field_type) => value_of(field_type),
            }
        };
        Ok(
            match (
                variant.stored_representation(&tagged_union.representation),
                &variant.data,
            ) {
                (EnumRepresentation::ExternallyTagged, Some(data)) => {
                    vec![(
                        variant.db_name().to_owned(),
                        PlannedValue::Set(payload_value(data)?),
                    )]
                }
                (EnumRepresentation::InternallyTagged { tag }, data) => {
                    let mut values = vec![(tag.clone(), tag_value)];
                    if let Some(data) = data {
                        values.extend(payload_fields(data)?);
                    }
                    values
                }
                (EnumRepresentation::AdjacentlyTagged { tag, content }, data) => {
                    let mut values = vec![(tag.clone(), tag_value)];
                    if let Some(data) = data {
                        values.push((content.clone(), PlannedValue::Set(payload_value(data)?)));
                    }
                    values
                }
                (EnumRepresentation::Untagged, Some(data)) => payload_fields(data)?,
                (EnumRepresentation::ExternallyTagged | EnumRepresentation::Untagged, None) => {
                    Vec::new()
                }
            },
        )
    }

    fn not_an_object(
        &self,
        table: &TableConfig,
        field: &StructField,
        name: &str,
    ) -> EvenframeError {
        EvenframeError::mock_generation(format!(
            "`{}.{}` flattens the enum `{name}`, whose variant holds no struct for serde to \
             write beside other keys",
            table.table_name, field.field_name
        ))
    }

    /// The rewritten values of the existing record at `position` in the id
    /// pool. SET replaces a value whole, where MERGE would keep an object's
    /// stale keys. An optional field that is unset stays unset, and a field
    /// the table no longer has is set to NONE, which unsets it.
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
                let removed = !table
                    .effective()
                    .struct_config
                    .fields
                    .iter()
                    .any(|kept| kept.db_name() == field.db_name());
                let value = match &field.field_type {
                    _ if removed => PlannedValue::Set("NONE".to_owned()),
                    // A present value is rewritten with a present one,
                    // unless no present value can be generated.
                    FieldType::Option(inner) if !self.has_unfillable_link(inner) => {
                        PlannedValue::UnlessAbsent(self.generate_present(table, field, position)?)
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

#[cfg(all(test, feature = "scan"))]
mod flatten_tests {
    use crate::scan::{ScanConfig, build_all_configs};
    use crate::schemasync::{config::SchemasyncConfig, mockmake::Mockmaker};
    use crate::types::ForeignTypeRegistry;
    use std::fs;
    use surrealdb::{Surreal, engine::remote::http::Client};
    use tempfile::TempDir;

    const SOURCE: &str = r#"
        use evenframe::Evenframe;
        use std::collections::HashMap;

        #[derive(Evenframe)]
        #[serde(tag = "kind")]
        pub enum Payload { Click { x: i32 }, Close }

        #[derive(Evenframe)]
        pub struct Event {
            pub id: String,
            pub at: String,
            #[serde(flatten)]
            pub payload: Payload,
            #[serde(flatten)]
            pub extra: HashMap<String, String>,
        }
    "#;

    #[test]
    fn a_flattened_enum_writes_a_variants_keys_beside_the_records() {
        let project = TempDir::new().unwrap();
        fs::write(
            project.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::create_dir_all(project.path().join("src")).unwrap();
        fs::write(project.path().join("src/lib.rs"), SOURCE).unwrap();
        let types = build_all_configs(&ScanConfig {
            scan_path: project.path().to_path_buf(),
            ..ScanConfig::default()
        })
        .unwrap()
        .into_schemasync()
        .unwrap();
        let db = Surreal::<Client>::init();
        let config: SchemasyncConfig =
            toml::from_str("should_generate_mocks = true\n[database]\nurl = \"x\"\n").unwrap();
        let registry = ForeignTypeRegistry::default();
        let mockmaker = Mockmaker::new(
            &db,
            &types.tables,
            &types.objects,
            &types.enums,
            &types.declared,
            &config,
            &registry,
        )
        .unwrap();
        let event = &types.tables["event"];
        let field = |name: &str| {
            event
                .struct_config
                .fields
                .iter()
                .find(|field| field.field_name == name)
                .unwrap()
        };
        for _ in 0..20 {
            let keys: Vec<(String, String)> = mockmaker
                .flattened_values(event, field("payload"), 0)
                .unwrap()
                .into_iter()
                .map(|(key, value)| (key, value.literal().to_owned()))
                .collect();
            match keys.as_slice() {
                [(tag, close)] => assert_eq!((tag.as_str(), close.as_str()), ("kind", "'Close'")),
                [(tag, click), (x, _)] => {
                    assert_eq!(
                        (tag.as_str(), click.as_str(), x.as_str()),
                        ("kind", "'Click'", "x")
                    );
                }
                other => panic!("unexpected keys {other:?}"),
            }
            assert!(
                mockmaker
                    .flattened_values(event, field("extra"), 0)
                    .unwrap()
                    .is_empty(),
                "a flattened map adds no keys"
            );
        }
    }
}

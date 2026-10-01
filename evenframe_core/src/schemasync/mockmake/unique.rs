//! Keeps mock records inside their table's unique indexes: no two records a
//! run leaves in a table share a unique index's values.

use super::Mockmaker;
use crate::error::{EvenframeError, Result};
use crate::schemasync::database::surql::execute::RPC_SIZE_LIMIT;
use crate::schemasync::table::{IndexKind, TableConfig};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use surrealdb::types::Value;

/// How often a record whose values collide is regenerated before the run
/// gives up on the index.
const UNIQUE_ROUNDS: usize = 20;

/// A value a record this run writes gives a field.
#[derive(Debug, Clone)]
pub(crate) enum PlannedValue {
    /// The field takes this SurrealQL literal.
    Set(String),
    /// An optional field keeps NULL, and otherwise takes this literal.
    UnlessNull(String),
}

impl PlannedValue {
    pub(crate) fn literal(&self) -> &str {
        match self {
            PlannedValue::Set(literal) | PlannedValue::UnlessNull(literal) => literal,
        }
    }
}

/// A record this run writes: a new one with every written field, or an
/// existing one with its rewritten fields.
#[derive(Debug)]
pub(crate) struct PlannedRecord {
    pub id: String,
    /// The record's position in its table's id pool.
    pub position: usize,
    pub existing: bool,
    /// Each written field's value, in the struct's field order.
    pub values: Vec<(String, PlannedValue)>,
}

/// One unique index, as field paths split into their segments.
struct UniqueIndex {
    name: String,
    paths: Vec<Vec<String>>,
}

impl UniqueIndex {
    /// The top-level fields the index reads.
    fn fields(&self) -> impl Iterator<Item = &str> {
        self.paths
            .iter()
            .filter_map(|segments| segments.first())
            .map(String::as_str)
    }

    /// Every key a record with `fields` puts in the index: one per
    /// combination of the values its paths reach. A key holding NULL or
    /// NONE constrains nothing, since the index lets such records repeat.
    fn keys(&self, fields: &BTreeMap<String, Value>) -> Vec<Vec<Value>> {
        let mut keys: Vec<Vec<Value>> = vec![Vec::new()];
        for segments in &self.paths {
            let reached = reach(fields, segments);
            keys = keys
                .into_iter()
                .flat_map(|key| {
                    reached.iter().map(move |value| {
                        let mut extended = key.clone();
                        extended.push(value.clone());
                        extended
                    })
                })
                .collect();
        }
        keys.retain(|key| {
            !key.iter()
                .any(|value| matches!(value, Value::Null | Value::None))
        });
        keys
    }
}

/// The values `segments` reaches in a record's fields, where `*` steps into
/// each element of an array.
fn reach(fields: &BTreeMap<String, Value>, segments: &[String]) -> Vec<Value> {
    let Some((first, rest)) = segments.split_first() else {
        return Vec::new();
    };
    let mut reached = vec![fields.get(first).cloned().unwrap_or(Value::None)];
    for segment in rest {
        reached = reached
            .into_iter()
            .flat_map(|value| match (segment.as_str(), value) {
                ("*", Value::Array(elements)) => elements.to_vec(),
                (key, Value::Object(object)) => {
                    vec![object.get(key).cloned().unwrap_or(Value::None)]
                }
                _ => vec![Value::None],
            })
            .collect();
    }
    reached
}

fn unique_indexes(table: &TableConfig, table_name: &str) -> Vec<UniqueIndex> {
    table
        .all_indexes(table_name)
        .into_iter()
        .filter(|index| matches!(index.kind, IndexKind::Unique))
        .map(|index| UniqueIndex {
            name: index.index_name(table_name),
            paths: index
                .fields
                .iter()
                .map(|path| path.split('.').map(str::to_string).collect())
                .collect(),
        })
        .collect()
}

impl Mockmaker<'_> {
    /// Regenerates the values of any record in `records` that would share a
    /// unique index's key with another record the table ends up holding,
    /// and fails naming the index when that keeps happening.
    pub(crate) async fn keep_unique(
        &self,
        table_name: &str,
        table: &TableConfig,
        records: &mut [PlannedRecord],
    ) -> Result<()> {
        let indexes = unique_indexes(table, table_name);
        if indexes.is_empty() || records.is_empty() {
            return Ok(());
        }
        let fields: BTreeSet<&str> = indexes.iter().flat_map(UniqueIndex::fields).collect();
        let stored = self.stored_values(table_name, &fields).await?;
        let mut evaluated: Vec<Option<BTreeMap<String, Value>>> =
            records.iter().map(|_| None).collect();

        for _ in 0..UNIQUE_ROUNDS {
            self.evaluate(records, &mut evaluated, &fields).await?;
            let colliding = colliding_records(&indexes, &stored, records, &evaluated);
            if colliding.is_empty() {
                return Ok(());
            }
            for (position, index_name) in colliding {
                let index = indexes
                    .iter()
                    .find(|index| index.name == index_name)
                    .ok_or_else(|| {
                        EvenframeError::mock_generation(format!(
                            "unique index `{index_name}` of `{table_name}` went missing"
                        ))
                    })?;
                let index_fields: BTreeSet<&str> = index.fields().collect();
                self.regenerate(table, &mut records[position], &index_fields)
                    .map_err(|error| {
                        EvenframeError::mock_generation(format!(
                            "`{table_name}` cannot regenerate the values of `{}` for unique index `{index_name}`: {error}",
                            records[position].id
                        ))
                    })?;
                evaluated[position] = None;
            }
        }
        let remaining = colliding_records(&indexes, &stored, records, &evaluated);
        let names: BTreeSet<&str> = remaining.iter().map(|(_, name)| name.as_str()).collect();
        Err(EvenframeError::mock_generation(format!(
            "`{table_name}` found no unique values for index {} after {UNIQUE_ROUNDS} attempts; \
             its fields generate too few distinct values for the record count",
            names.into_iter().collect::<Vec<_>>().join(", ")
        )))
    }

    /// Every stored record's values for `fields`, by its id as a record
    /// literal.
    async fn stored_values(
        &self,
        table_name: &str,
        fields: &BTreeSet<&str>,
    ) -> Result<BTreeMap<String, BTreeMap<String, Value>>> {
        let has_records = self
            .id_map
            .get(table_name)
            .is_some_and(|ids| ids.len() > self.new_records.get(table_name).copied().unwrap_or(0));
        if !has_records {
            return Ok(BTreeMap::new());
        }
        let selected = fields.iter().copied().collect::<Vec<_>>().join(", ");
        let query = format!("SELECT <string> id AS __id, {selected} FROM {table_name};");
        let rows: Vec<Value> = self
            .db
            .query(query)
            .await
            .and_then(|mut response| response.take(0))
            .map_err(|error| {
                EvenframeError::mock_generation(format!(
                    "reading the unique index values of `{table_name}`: {error}"
                ))
            })?;
        Ok(rows
            .into_iter()
            .filter_map(|row| match row {
                Value::Object(object) => {
                    let mut values = object.into_inner();
                    match values.remove("__id") {
                        Some(Value::String(id)) => Some((id, values)),
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect())
    }

    /// Evaluates the literals of every record not yet evaluated, in requests
    /// under the size limit, as `fields` to their values.
    async fn evaluate(
        &self,
        records: &[PlannedRecord],
        evaluated: &mut [Option<BTreeMap<String, Value>>],
        fields: &BTreeSet<&str>,
    ) -> Result<()> {
        let pending: Vec<usize> = (0..records.len())
            .filter(|position| evaluated[*position].is_none())
            .collect();
        let mut batch: Vec<(usize, Vec<String>, String)> = Vec::new();
        let mut size = 0;
        for position in pending {
            let names: Vec<String> = records[position]
                .values
                .iter()
                .filter(|(name, _)| fields.contains(name.as_str()))
                .map(|(name, _)| name.clone())
                .collect();
            let literal = format!(
                "[{}]",
                records[position]
                    .values
                    .iter()
                    .filter(|(name, _)| fields.contains(name.as_str()))
                    .map(|(_, value)| value.literal())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if !batch.is_empty() && size + literal.len() > RPC_SIZE_LIMIT {
                self.evaluate_batch(&batch, evaluated).await?;
                batch.clear();
                size = 0;
            }
            size += literal.len() + 2;
            batch.push((position, names, literal));
        }
        if !batch.is_empty() {
            self.evaluate_batch(&batch, evaluated).await?;
        }
        Ok(())
    }

    async fn evaluate_batch(
        &self,
        batch: &[(usize, Vec<String>, String)],
        evaluated: &mut [Option<BTreeMap<String, Value>>],
    ) -> Result<()> {
        let query = format!(
            "RETURN [{}];",
            batch
                .iter()
                .map(|(_, _, literal)| literal.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let returned: Value = self
            .db
            .query(query)
            .await
            .and_then(|mut response| response.take(0))
            .map_err(|error| {
                EvenframeError::mock_generation(format!(
                    "evaluating generated values for unique indexes: {error}"
                ))
            })?;
        let Value::Array(rows) = returned else {
            return Err(EvenframeError::mock_generation(
                "evaluating generated values for unique indexes returned no array",
            ));
        };
        if rows.len() != batch.len() {
            return Err(EvenframeError::mock_generation(format!(
                "evaluating {} records' generated values returned {} rows",
                batch.len(),
                rows.len()
            )));
        }
        for ((position, names, _), row) in batch.iter().zip(rows.iter()) {
            let Value::Array(values) = row else {
                return Err(EvenframeError::mock_generation(
                    "an evaluated record's values are not an array",
                ));
            };
            evaluated[*position] =
                Some(names.iter().cloned().zip(values.iter().cloned()).collect());
        }
        Ok(())
    }

    /// Generates new values for `record`'s `fields`, keeping how each is
    /// written.
    fn regenerate(
        &self,
        table: &TableConfig,
        record: &mut PlannedRecord,
        fields: &BTreeSet<&str>,
    ) -> Result<()> {
        let mut regenerated = false;
        for (name, value) in &mut record.values {
            if !fields.contains(name.as_str()) {
                continue;
            }
            let field = table
                .struct_config
                .fields
                .iter()
                .find(|field| field.field_name == *name)
                .ok_or_else(|| EvenframeError::mock_generation(format!("no field `{name}`")))?;
            *value =
                match value {
                    PlannedValue::Set(_) => {
                        PlannedValue::Set(self.generate_value(table, field, record.position)?)
                    }
                    PlannedValue::UnlessNull(_) => PlannedValue::UnlessNull(
                        self.generate_present(table, field, record.position)?,
                    ),
                };
            regenerated = true;
        }
        if regenerated {
            Ok(())
        } else {
            Err(EvenframeError::mock_generation(
                "none of the index's fields is written by this run",
            ))
        }
    }
}

/// Each record whose values would repeat another's key, with the index,
/// checking stored records the run leaves alone first, then the planned
/// ones in order.
fn colliding_records(
    indexes: &[UniqueIndex],
    stored: &BTreeMap<String, BTreeMap<String, Value>>,
    records: &[PlannedRecord],
    evaluated: &[Option<BTreeMap<String, Value>>],
) -> Vec<(usize, String)> {
    let planned_ids: HashSet<&str> = records
        .iter()
        .filter(|record| record.existing)
        .map(|record| record.id.as_str())
        .collect();
    let mut seen: Vec<HashSet<Vec<Value>>> = indexes.iter().map(|_| HashSet::new()).collect();
    for (id, values) in stored {
        if planned_ids.contains(id.as_str()) {
            continue;
        }
        for (index, keys) in indexes.iter().zip(seen.iter_mut()) {
            keys.extend(index.keys(values));
        }
    }

    let mut colliding = Vec::new();
    for (position, record) in records.iter().enumerate() {
        let Some(written) = &evaluated[position] else {
            continue;
        };
        let mut fields = if record.existing {
            stored.get(&record.id).cloned().unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        for (name, value) in &record.values {
            let Some(evaluated_value) = written.get(name) else {
                continue;
            };
            // `IF field != NULL` keeps only a NULL; an absent (NONE) field
            // takes the value.
            let keeps_null = matches!(value, PlannedValue::UnlessNull(_))
                && matches!(fields.get(name), Some(Value::Null));
            if !keeps_null {
                fields.insert(name.clone(), evaluated_value.clone());
            }
        }
        for (index, keys) in indexes.iter().zip(seen.iter_mut()) {
            let record_keys = index.keys(&fields);
            if record_keys.iter().any(|key| keys.contains(key)) {
                colliding.push((position, index.name.clone()));
            } else {
                keys.extend(record_keys);
            }
        }
    }
    colliding
}

#[cfg(test)]
mod tests {
    use super::{PlannedRecord, PlannedValue, UniqueIndex, colliding_records};
    use std::collections::BTreeMap;
    use surrealdb::Surreal;
    use surrealdb::engine::local::Mem;
    use surrealdb::types::{Array, Value};

    fn index(name: &str, paths: &[&str]) -> UniqueIndex {
        UniqueIndex {
            name: name.to_string(),
            paths: paths
                .iter()
                .map(|path| path.split('.').map(str::to_string).collect())
                .collect(),
        }
    }

    fn fields(entries: &[(&str, Value)]) -> BTreeMap<String, Value> {
        entries
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect()
    }

    fn new_record(id: &str, values: &[&str]) -> PlannedRecord {
        PlannedRecord {
            id: id.to_string(),
            position: 0,
            existing: false,
            values: values
                .iter()
                .map(|name| (name.to_string(), PlannedValue::Set(String::new())))
                .collect(),
        }
    }

    #[test]
    fn element_paths_key_each_element_and_nulls_constrain_nothing() {
        let tags = index("tags", &["tags.*"]);
        let record = fields(&[(
            "tags",
            Value::Array(Array::from(vec![
                Value::String("a".into()),
                Value::String("b".into()),
            ])),
        )]);
        assert_eq!(tags.keys(&record).len(), 2);

        let pair = index("pair", &["slug", "published"]);
        let with_null = fields(&[("slug", Value::Null), ("published", Value::Bool(false))]);
        assert!(pair.keys(&with_null).is_empty());
    }

    #[test]
    fn a_repeated_key_marks_the_later_record_and_stored_keys_count() {
        let pair = index("pair", &["slug", "published"]);
        let slug = |text: &str| Value::String(text.into());
        let stored = BTreeMap::from([(
            "post:1".to_string(),
            fields(&[("slug", slug("r")), ("published", Value::Bool(false))]),
        )]);
        let records = vec![
            new_record("post:2", &["slug", "published"]),
            new_record("post:3", &["slug", "published"]),
            new_record("post:4", &["slug", "published"]),
        ];
        let evaluated = vec![
            Some(fields(&[
                ("slug", slug("s")),
                ("published", Value::Bool(false)),
            ])),
            Some(fields(&[
                ("slug", slug("s")),
                ("published", Value::Bool(false)),
            ])),
            Some(fields(&[
                ("slug", slug("r")),
                ("published", Value::Bool(false)),
            ])),
        ];
        let colliding = colliding_records(&[pair], &stored, &records, &evaluated);
        assert_eq!(
            colliding,
            vec![(1, "pair".to_string()), (2, "pair".to_string())]
        );
    }

    #[tokio::test]
    async fn a_unique_index_lets_records_share_null() {
        let db = Surreal::new::<Mem>(()).await.unwrap();
        db.use_ns("test").use_db("test").await.unwrap();
        db.query(
            "DEFINE TABLE post SCHEMAFULL;\
             DEFINE FIELD slug ON post TYPE null | string;\
             DEFINE INDEX post_slug ON post FIELDS slug UNIQUE;\
             CREATE post:1 SET slug = NULL;\
             CREATE post:2 SET slug = NULL;",
        )
        .await
        .unwrap()
        .check()
        .unwrap();
    }
}

use super::Mockmaker;
use crate::evenframe_log;
use crate::types::{EnumRepresentation, FieldType, StructField, TaggedUnion, VariantData};
use std::collections::BTreeSet;

enum RepointError {
    /// A link that must point at a record, to tables that keep none.
    Unfillable(String),
    /// A value whose links cannot be told apart or reached.
    Unsupported(String),
}

/// A named type a value can hold.
enum Named<'t> {
    Object(&'t [StructField]),
    Enum(&'t TaggedUnion),
}

/// `parts` joined with OR, or `None` when no part can hold a deleted id.
fn any_of(parts: Vec<Option<String>>) -> Option<String> {
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    (!parts.is_empty()).then(|| format!("({})", parts.join(" OR ")))
}

impl Mockmaker<'_> {
    /// Point stored links at records deleted as excess to kept records of
    /// the same tables. Where nothing is kept, an optional link becomes
    /// NULL and a list drops it. Relations whose ends were deleted are
    /// removed by the database.
    pub(super) async fn repoint_links_to_excess(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.excess_ids.values().all(Vec::is_empty) {
            return Ok(());
        }
        let defined = self.defined_tables().await?;
        let mut statements = String::new();
        for (table_name, table) in self.tables.iter().filter(|(t, _)| defined.contains(*t)) {
            let table = table.effective();
            for field in &table.struct_config.fields {
                let endpoint =
                    table.relation.is_some() && matches!(field.field_name.as_str(), "in" | "out");
                if endpoint || field.edge_config.is_some() {
                    continue;
                }
                let name = &field.field_name;
                let describe = |e: RepointError| match e {
                    RepointError::Unfillable(targets) => format!(
                        "`{table_name}.{name}` must link to {targets}, which keeps no records; \
                         give it records or make the link optional"
                    ),
                    RepointError::Unsupported(reason) => {
                        format!("`{table_name}.{name}` links to deleted records, but {reason}")
                    }
                };
                let Some(found) = self
                    .holds_excess(&field.field_type, name, 0, &mut Vec::new())
                    .map_err(describe)?
                else {
                    continue;
                };
                if !field.is_mock_written() {
                    return Err(describe(RepointError::Unsupported(
                        "it is readonly, computed or skipped, so it cannot be rewritten".into(),
                    ))
                    .into());
                }
                let value = match self.repointed(&field.field_type, name, 0, &mut Vec::new()) {
                    Ok(value) => value,
                    // Only an error when a record holds such a link.
                    Err(RepointError::Unfillable(targets)) => {
                        let holders: Option<i64> = self
                            .db
                            .query(format!(
                                "RETURN count(SELECT id FROM {table_name} WHERE {found});"
                            ))
                            .await
                            .and_then(|mut response| response.take(0))
                            .map_err(|e| format!("finding links to deleted records: {e}"))?;
                        if holders.ok_or("counting links to deleted records returned nothing")? == 0
                        {
                            continue;
                        }
                        return Err(describe(RepointError::Unfillable(targets)).into());
                    }
                    Err(e) => return Err(describe(e).into()),
                };
                statements.push_str(&format!(
                    "UPDATE {table_name} SET {name} = {value} WHERE {found};\n"
                ));
            }
        }
        if statements.is_empty() {
            return Ok(());
        }
        evenframe_log!(&statements, "repoint_statements.surql");
        self.db
            .query(statements)
            .await
            .and_then(|response| response.check())
            .map_err(|e| format!("pointing links away from deleted records: {e}"))?;
        Ok(())
    }

    fn named(&self, name: &str) -> Option<Named<'_>> {
        if let Some(object) = self.objects.get(name) {
            return Some(Named::Object(&object.effective().fields));
        }
        if let Some(table) = self
            .tables
            .values()
            .find(|t| t.effective().struct_config.struct_name == name)
        {
            return Some(Named::Object(&table.effective().struct_config.fields));
        }
        self.enums
            .values()
            .find(|e| e.enum_name == name)
            .map(Named::Enum)
    }

    /// The ids deleted as excess from the tables a link to `type_name`
    /// can point at.
    fn excess_of(&self, type_name: &str) -> Vec<&str> {
        self.link_target_tables(type_name)
            .iter()
            .filter_map(|t| self.excess_ids.get(t))
            .flatten()
            .map(String::as_str)
            .collect()
    }

    /// The ids kept in the tables a link to `type_name` can point at.
    fn kept_of(&self, type_name: &str) -> Vec<&str> {
        self.link_target_tables(type_name)
            .iter()
            .filter_map(|t| self.id_map.get(t))
            .flatten()
            .map(String::as_str)
            .collect()
    }

    /// A SurrealQL condition true when the value at `v` holds a deleted id,
    /// or `None` when a value of `ty` cannot hold one.
    fn holds_excess(
        &self,
        ty: &FieldType,
        v: &str,
        depth: usize,
        visiting: &mut Vec<String>,
    ) -> Result<Option<String>, RepointError> {
        let x = format!("$x{depth}");
        Ok(match ty {
            FieldType::RecordLink(inner) => match inner.as_ref() {
                FieldType::Other(type_name) => {
                    let deleted = self.excess_of(type_name);
                    (!deleted.is_empty()).then(|| format!("({v} IN [{}])", deleted.join(", ")))
                }
                _ => None,
            },
            FieldType::Option(inner) => self
                .holds_excess(inner, v, depth, visiting)?
                .map(|c| format!("({v} != NULL AND {v} != NONE AND {c})")),
            FieldType::Vec(inner) => self
                .holds_excess(inner, &x, depth + 1, visiting)?
                .map(|c| format!("array::any({v}, |{x}| {c})")),
            FieldType::HashMap(_, value) | FieldType::BTreeMap(_, value) => self
                .holds_excess(value, &x, depth + 1, visiting)?
                .map(|c| format!("array::any(object::values({v}), |{x}| {c})")),
            FieldType::Tuple(types) => any_of(
                types
                    .iter()
                    .enumerate()
                    .map(|(i, t)| self.holds_excess(t, &format!("{v}[{i}]"), depth, visiting))
                    .collect::<Result<_, _>>()?,
            ),
            FieldType::Struct(fields) => any_of(
                fields
                    .iter()
                    .map(|(n, t)| self.holds_excess(t, &format!("{v}.{n}"), depth, visiting))
                    .collect::<Result<_, _>>()?,
            ),
            FieldType::Other(name) => self.named_holds_excess(name, v, depth, visiting)?,
            _ => None,
        })
    }

    fn named_holds_excess(
        &self,
        name: &str,
        v: &str,
        depth: usize,
        visiting: &mut Vec<String>,
    ) -> Result<Option<String>, RepointError> {
        let Some(named) = self.named(name) else {
            return Ok(None);
        };
        if visiting.iter().any(|n| n == name) {
            if self.reaches_excess(&FieldType::Other(name.to_string()), &mut BTreeSet::new()) {
                return Err(RepointError::Unsupported(format!(
                    "`{name}` contains itself, so its links cannot all be reached"
                )));
            }
            return Ok(None);
        }
        visiting.push(name.to_string());
        let found = match named {
            Named::Object(fields) => self.fields_hold_excess(fields, v, depth, visiting)?,
            Named::Enum(union) => {
                let mut parts = Vec::new();
                for (variant, payload) in variants(union) {
                    let found = match (&union.representation, payload) {
                        (EnumRepresentation::Untagged, payload) => {
                            if self
                                .payload_holds_excess(payload, v, depth, visiting)?
                                .is_some()
                            {
                                return Err(RepointError::Unsupported(format!(
                                    "the untagged enum `{name}` does not say which variant a value is"
                                )));
                            }
                            None
                        }
                        (EnumRepresentation::InternallyTagged { tag }, Payload::Inline(fields)) => {
                            self.fields_hold_excess(fields, v, depth, visiting)?
                                .map(|c| format!("({v}.{tag} = '{variant}' AND {c})"))
                        }
                        (EnumRepresentation::AdjacentlyTagged { tag, content }, payload) => self
                            .payload_holds_excess(
                                payload,
                                &format!("{v}.{content}"),
                                depth,
                                visiting,
                            )?
                            .map(|c| format!("({v}.{tag} = '{variant}' AND {c})")),
                        // Externally tagged, and internally tagged newtype
                        // variants, which serialize externally tagged.
                        (_, payload) => {
                            let at = format!("{v}.{variant}");
                            self.payload_holds_excess(payload, &at, depth, visiting)?
                                .map(|c| format!("({at} != NONE AND {c})"))
                        }
                    };
                    parts.push(found);
                }
                any_of(parts)
            }
        };
        visiting.pop();
        Ok(found)
    }

    /// Whether a value of `ty` can hold a deleted id at any depth.
    fn reaches_excess(&self, ty: &FieldType, seen: &mut BTreeSet<String>) -> bool {
        match ty {
            FieldType::RecordLink(inner) => match inner.as_ref() {
                FieldType::Other(type_name) => !self.excess_of(type_name).is_empty(),
                _ => false,
            },
            FieldType::Option(inner) | FieldType::Vec(inner) => self.reaches_excess(inner, seen),
            FieldType::HashMap(_, value) | FieldType::BTreeMap(_, value) => {
                self.reaches_excess(value, seen)
            }
            FieldType::Tuple(types) => types.iter().any(|t| self.reaches_excess(t, seen)),
            FieldType::Struct(fields) => fields.iter().any(|(_, t)| self.reaches_excess(t, seen)),
            FieldType::Other(name) if seen.insert(name.clone()) => match self.named(name) {
                Some(Named::Object(fields)) => fields
                    .iter()
                    .any(|f| self.reaches_excess(&f.field_type, seen)),
                Some(Named::Enum(union)) => variants(union).any(|(_, payload)| match payload {
                    Payload::Inline(fields) => fields
                        .iter()
                        .any(|f| self.reaches_excess(&f.field_type, seen)),
                    Payload::Type(ty) => self.reaches_excess(ty, seen),
                }),
                None => false,
            },
            _ => false,
        }
    }

    fn fields_hold_excess(
        &self,
        fields: &[StructField],
        v: &str,
        depth: usize,
        visiting: &mut Vec<String>,
    ) -> Result<Option<String>, RepointError> {
        Ok(any_of(
            fields
                .iter()
                .map(|f| {
                    self.holds_excess(
                        &f.field_type,
                        &format!("{v}.{}", f.field_name),
                        depth,
                        visiting,
                    )
                })
                .collect::<Result<_, _>>()?,
        ))
    }

    fn payload_holds_excess(
        &self,
        payload: Payload<'_>,
        v: &str,
        depth: usize,
        visiting: &mut Vec<String>,
    ) -> Result<Option<String>, RepointError> {
        match payload {
            Payload::Inline(fields) => self.fields_hold_excess(fields, v, depth, visiting),
            Payload::Type(ty) => self.holds_excess(ty, v, depth, visiting),
        }
    }

    /// The value at `v` with its deleted ids replaced, for a `ty` that can
    /// hold them.
    fn repointed(
        &self,
        ty: &FieldType,
        v: &str,
        depth: usize,
        visiting: &mut Vec<String>,
    ) -> Result<String, RepointError> {
        let x = format!("$x{depth}");
        match ty {
            FieldType::RecordLink(inner) => {
                let FieldType::Other(type_name) = inner.as_ref() else {
                    return Ok(v.to_string());
                };
                let kept = self.kept_of(type_name);
                if kept.is_empty() {
                    return Err(RepointError::Unfillable(
                        self.link_target_tables(type_name).join(" or "),
                    ));
                }
                Ok(format!(
                    "(IF {v} IN [{}] THEN rand::enum([{}]) ELSE {v} END)",
                    self.excess_of(type_name).join(", "),
                    kept.join(", ")
                ))
            }
            FieldType::Option(inner) => match self.repointed(inner, v, depth, visiting) {
                Ok(r) => Ok(format!(
                    "(IF {v} = NULL OR {v} = NONE THEN {v} ELSE {r} END)"
                )),
                Err(RepointError::Unfillable(_)) => {
                    let c = self.required_holds(inner, v, depth, visiting)?;
                    Ok(format!(
                        "(IF {v} != NULL AND {v} != NONE AND {c} THEN NULL ELSE {v} END)"
                    ))
                }
                Err(e) => Err(e),
            },
            FieldType::Vec(inner) => match self.repointed(inner, &x, depth + 1, visiting) {
                Ok(r) => Ok(format!("array::map({v}, |{x}| {r})")),
                Err(RepointError::Unfillable(_)) => {
                    let c = self.required_holds(inner, &x, depth + 1, visiting)?;
                    Ok(format!("array::filter({v}, |{x}| !{c})"))
                }
                Err(e) => Err(e),
            },
            FieldType::HashMap(_, value) | FieldType::BTreeMap(_, value) => {
                let at = format!("{x}[1]");
                match self.repointed(value, &at, depth + 1, visiting) {
                    Ok(r) => Ok(format!(
                        "object::from_entries(array::map(object::entries({v}), |{x}| [{x}[0], {r}]))"
                    )),
                    Err(RepointError::Unfillable(_)) => {
                        let c = self.required_holds(value, &at, depth + 1, visiting)?;
                        Ok(format!(
                            "object::from_entries(array::filter(object::entries({v}), |{x}| !{c}))"
                        ))
                    }
                    Err(e) => Err(e),
                }
            }
            FieldType::Tuple(types) => {
                let items = types
                    .iter()
                    .enumerate()
                    .map(|(i, t)| self.repointed_or_same(t, &format!("{v}[{i}]"), depth, visiting))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!("[{}]", items.join(", ")))
            }
            FieldType::Struct(fields) => {
                let mut assignments = Vec::new();
                for (n, t) in fields {
                    let at = format!("{v}.{n}");
                    if self.holds_excess(t, &at, depth, visiting)?.is_some() {
                        assignments
                            .push(format!("{n}: {}", self.repointed(t, &at, depth, visiting)?));
                    }
                }
                Ok(format!(
                    "object::extend({v}, {{ {} }})",
                    assignments.join(", ")
                ))
            }
            FieldType::Other(name) => self.named_repointed(name, v, depth, visiting),
            _ => Ok(v.to_string()),
        }
    }

    /// The condition for a part that `repointed` found unfillable, which
    /// therefore holds deleted ids.
    fn required_holds(
        &self,
        ty: &FieldType,
        v: &str,
        depth: usize,
        visiting: &mut Vec<String>,
    ) -> Result<String, RepointError> {
        self.holds_excess(ty, v, depth, visiting)?
            .ok_or_else(|| RepointError::Unsupported(format!("no deleted id was found at `{v}`")))
    }

    fn repointed_or_same(
        &self,
        ty: &FieldType,
        v: &str,
        depth: usize,
        visiting: &mut Vec<String>,
    ) -> Result<String, RepointError> {
        match self.holds_excess(ty, v, depth, visiting)? {
            Some(_) => self.repointed(ty, v, depth, visiting),
            None => Ok(v.to_string()),
        }
    }

    fn named_repointed(
        &self,
        name: &str,
        v: &str,
        depth: usize,
        visiting: &mut Vec<String>,
    ) -> Result<String, RepointError> {
        let Some(named) = self.named(name) else {
            return Ok(v.to_string());
        };
        visiting.push(name.to_string());
        let value = match named {
            Named::Object(fields) => self.fields_repointed(fields, v, depth, visiting)?,
            Named::Enum(union) => {
                let mut branches = Vec::new();
                for (variant, payload) in variants(union) {
                    let branch = match (&union.representation, payload) {
                        (EnumRepresentation::InternallyTagged { tag }, Payload::Inline(fields)) => {
                            match self.fields_hold_excess(fields, v, depth, visiting)? {
                                Some(c) => Some((
                                    format!("{v}.{tag} = '{variant}' AND {c}"),
                                    self.fields_repointed(fields, v, depth, visiting)?,
                                )),
                                None => None,
                            }
                        }
                        (EnumRepresentation::AdjacentlyTagged { tag, content }, payload) => {
                            let at = format!("{v}.{content}");
                            match self.payload_holds_excess(payload, &at, depth, visiting)? {
                                Some(c) => Some((
                                    format!("{v}.{tag} = '{variant}' AND {c}"),
                                    format!(
                                        "object::extend({v}, {{ {content}: {} }})",
                                        self.payload_repointed(payload, &at, depth, visiting)?
                                    ),
                                )),
                                None => None,
                            }
                        }
                        (_, payload) => {
                            let at = format!("{v}.{variant}");
                            match self.payload_holds_excess(payload, &at, depth, visiting)? {
                                Some(c) => Some((
                                    format!("{at} != NONE AND {c}"),
                                    format!(
                                        "{{ {variant}: {} }}",
                                        self.payload_repointed(payload, &at, depth, visiting)?
                                    ),
                                )),
                                None => None,
                            }
                        }
                    };
                    branches.extend(branch);
                }
                let chain = branches
                    .iter()
                    .map(|(c, r)| format!("IF {c} THEN {r}"))
                    .collect::<Vec<_>>()
                    .join(" ELSE ");
                format!("({chain} ELSE {v} END)")
            }
        };
        visiting.pop();
        Ok(value)
    }

    fn fields_repointed(
        &self,
        fields: &[StructField],
        v: &str,
        depth: usize,
        visiting: &mut Vec<String>,
    ) -> Result<String, RepointError> {
        let mut assignments = Vec::new();
        for field in fields {
            let at = format!("{v}.{}", field.field_name);
            if self
                .holds_excess(&field.field_type, &at, depth, visiting)?
                .is_some()
            {
                assignments.push(format!(
                    "{}: {}",
                    field.field_name,
                    self.repointed(&field.field_type, &at, depth, visiting)?
                ));
            }
        }
        Ok(format!(
            "object::extend({v}, {{ {} }})",
            assignments.join(", ")
        ))
    }

    fn payload_repointed(
        &self,
        payload: Payload<'_>,
        v: &str,
        depth: usize,
        visiting: &mut Vec<String>,
    ) -> Result<String, RepointError> {
        match payload {
            Payload::Inline(fields) => self.fields_repointed(fields, v, depth, visiting),
            Payload::Type(ty) => self.repointed(ty, v, depth, visiting),
        }
    }
}

/// What a variant carries.
#[derive(Clone, Copy)]
enum Payload<'t> {
    Inline(&'t [StructField]),
    Type(&'t FieldType),
}

/// The variants of `union` that carry data, by name.
fn variants(union: &TaggedUnion) -> impl Iterator<Item = (&str, Payload<'_>)> {
    union.variants.iter().filter_map(|variant| {
        let variant = variant.effective();
        let payload = match variant.data.as_ref()? {
            VariantData::InlineStruct(config) => Payload::Inline(&config.effective().fields),
            VariantData::DataStructureRef(ty) => Payload::Type(ty),
        };
        Some((variant.name.as_str(), payload))
    })
}

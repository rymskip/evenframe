use super::Mockmaker;
use crate::evenframe_log;
use crate::schemasync::database::surql::execute::execute_bound;
use crate::schemasync::table::surql_ident;
use crate::types::{EnumRepresentation, FieldType, StructField, TaggedUnion, VariantData};
use std::collections::BTreeSet;
use surrealdb::types::Variables;

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

/// The state of one walk over a field's type: the named types it is inside,
/// and the linked types whose ids its SurrealQL names as query parameters.
#[derive(Default)]
struct Walk {
    visiting: Vec<String>,
    linked: BTreeSet<String>,
}

/// The query parameter suffix for a linked type's id lists.
fn parameter_suffix(type_name: &str) -> String {
    type_name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '_'
            }
        })
        .collect()
}

impl Mockmaker<'_> {
    /// Point stored links at records deleted as excess to kept records of
    /// the same tables. Where nothing is kept, an optional link becomes
    /// NULL and a list drops it. Relations whose ends were deleted are
    /// removed by the database. Each linked type's deleted and kept ids are
    /// bound as query parameters rather than written into every statement.
    pub(super) async fn repoint_links_to_excess(&self) -> Result<(), Box<dyn std::error::Error>> {
        if self.excess_ids.values().all(Vec::is_empty) {
            return Ok(());
        }
        let defined = self.defined_tables().await?;
        for (table_name, table) in self
            .tables
            .iter()
            .filter(|(table_name, _)| defined.contains(*table_name))
        {
            let table = table.effective();
            for field in &table.struct_config.fields {
                let endpoint = table.relation.is_some() && matches!(field.db_name(), "in" | "out");
                if endpoint || field.edge_config.is_some() {
                    continue;
                }
                let name = surql_ident(field.db_name());
                let describe = |error: RepointError| match error {
                    RepointError::Unfillable(targets) => format!(
                        "`{table_name}.{name}` must link to {targets}, which keeps no records; \
                         give it records or make the link optional"
                    ),
                    RepointError::Unsupported(reason) => {
                        format!("`{table_name}.{name}` links to deleted records, but {reason}")
                    }
                };
                let mut walk = Walk::default();
                let Some(found) = self
                    .holds_excess(&field.field_type, &name, 0, &mut walk)
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
                let value = match self.repointed(&field.field_type, &name, 0, &mut walk) {
                    Ok(value) => value,
                    // Only an error when a record holds such a link.
                    Err(RepointError::Unfillable(targets)) => {
                        let holders: Option<i64> = self
                            .db
                            .query(format!(
                                "RETURN count(SELECT id FROM {table_name} WHERE {found});"
                            ))
                            .bind(self.link_ids(&walk))
                            .await
                            .and_then(|mut response| response.take(0))
                            .map_err(|error| {
                                format!("finding links to deleted records: {error}")
                            })?;
                        if holders.ok_or("counting links to deleted records returned nothing")? == 0
                        {
                            continue;
                        }
                        return Err(describe(RepointError::Unfillable(targets)).into());
                    }
                    Err(error) => return Err(describe(error).into()),
                };
                let statement = format!("UPDATE {table_name} SET {name} = {value} WHERE {found};");
                evenframe_log!(&statement, "repoint_statements.surql", true);
                execute_bound(
                    self.db,
                    &statement,
                    &self.link_ids(&walk),
                    "update",
                    "repoint links",
                )
                .await
                .map_err(|error| format!("pointing links away from deleted records: {error}"))?;
            }
        }
        Ok(())
    }

    /// The deleted and kept ids of every type `walk` linked through, as the
    /// query parameters its SurrealQL names.
    fn link_ids(&self, walk: &Walk) -> Variables {
        let mut variables = Variables::new();
        for type_name in &walk.linked {
            let suffix = parameter_suffix(type_name);
            let deleted: Vec<String> = self
                .excess_of(type_name)
                .into_iter()
                .map(str::to_string)
                .collect();
            let kept: Vec<String> = self
                .kept_of(type_name)
                .into_iter()
                .map(str::to_string)
                .collect();
            variables.insert(format!("deleted_{suffix}"), deleted);
            variables.insert(format!("kept_{suffix}"), kept);
        }
        variables
    }

    /// Whether any table a link to `type_name` can point at lost records.
    fn has_excess(&self, type_name: &str) -> bool {
        self.link_target_tables(type_name).iter().any(|table| {
            self.excess_ids
                .get(table)
                .is_some_and(|ids| !ids.is_empty())
        })
    }

    /// Whether any table a link to `type_name` can point at keeps records.
    fn has_kept(&self, type_name: &str) -> bool {
        self.link_target_tables(type_name)
            .iter()
            .any(|table| self.id_map.get(table).is_some_and(|ids| !ids.is_empty()))
    }

    /// The type a value of `ty` links through when it is stored as a link: a
    /// `RecordLink`, or a table held by value, which the schema stores as a
    /// link to it.
    fn linked_type<'t>(&self, ty: &'t FieldType) -> Option<&'t str> {
        match ty {
            FieldType::RecordLink(inner) => match inner.as_ref() {
                FieldType::Other(type_name) => Some(type_name),
                _ => None,
            },
            FieldType::Other(name)
                if !self.enums.contains_key(name) && !self.link_target_tables(name).is_empty() =>
            {
                Some(name)
            }
            _ => None,
        }
    }

    fn named(&self, name: &str) -> Option<Named<'_>> {
        if let Some(object) = self.objects.get(name) {
            return Some(Named::Object(&object.effective().fields));
        }
        self.enums
            .values()
            .find(|union| union.enum_name == name)
            .map(Named::Enum)
    }

    /// The ids deleted as excess from the tables a link to `type_name`
    /// can point at.
    fn excess_of(&self, type_name: &str) -> Vec<&str> {
        self.link_target_tables(type_name)
            .iter()
            .filter_map(|table| self.excess_ids.get(table))
            .flatten()
            .map(String::as_str)
            .collect()
    }

    /// The ids kept in the tables a link to `type_name` can point at.
    fn kept_of(&self, type_name: &str) -> Vec<&str> {
        self.link_target_tables(type_name)
            .iter()
            .filter_map(|table| self.id_map.get(table))
            .flatten()
            .map(String::as_str)
            .collect()
    }

    /// A SurrealQL condition true when the value at `v` holds a deleted id,
    /// or `None` when a value of `ty` cannot hold one.
    fn holds_excess(
        &self,
        ty: &FieldType,
        place: &str,
        depth: usize,
        walk: &mut Walk,
    ) -> Result<Option<String>, RepointError> {
        if let Some(type_name) = self.linked_type(ty) {
            if !self.has_excess(type_name) {
                return Ok(None);
            }
            walk.linked.insert(type_name.to_string());
            let suffix = parameter_suffix(type_name);
            return Ok(Some(format!("(<string> {place} IN $deleted_{suffix})")));
        }
        let item = format!("$x{depth}");
        Ok(match ty {
            FieldType::Option(inner) => self
                .holds_excess(inner, place, depth, walk)?
                .map(|condition| format!("({place} != NULL AND {place} != NONE AND {condition})")),
            FieldType::Vec(inner) => self
                .holds_excess(inner, &item, depth + 1, walk)?
                .map(|condition| format!("array::any({place}, |{item}| {condition})")),
            FieldType::HashMap(_, value) | FieldType::BTreeMap(_, value) => self
                .holds_excess(value, &item, depth + 1, walk)?
                .map(|condition| {
                    format!("array::any(object::values({place}), |{item}| {condition})")
                }),
            FieldType::Tuple(types) => any_of(
                types
                    .iter()
                    .enumerate()
                    .map(|(index, member)| {
                        self.holds_excess(member, &format!("{place}[{index}]"), depth, walk)
                    })
                    .collect::<Result<_, _>>()?,
            ),
            FieldType::Struct(fields) => any_of(
                fields
                    .iter()
                    .map(|(key, member)| {
                        self.holds_excess(member, &format!("{place}.{key}"), depth, walk)
                    })
                    .collect::<Result<_, _>>()?,
            ),
            FieldType::Other(name) => self.named_holds_excess(name, place, depth, walk)?,
            _ => None,
        })
    }

    fn named_holds_excess(
        &self,
        name: &str,
        place: &str,
        depth: usize,
        walk: &mut Walk,
    ) -> Result<Option<String>, RepointError> {
        let Some(named) = self.named(name) else {
            return Ok(None);
        };
        if walk.visiting.iter().any(|visited| visited == name) {
            if self.reaches_excess(&FieldType::Other(name.to_string()), &mut BTreeSet::new()) {
                return Err(RepointError::Unsupported(format!(
                    "`{name}` contains itself, so its links cannot all be reached"
                )));
            }
            return Ok(None);
        }
        walk.visiting.push(name.to_string());
        let found = match named {
            Named::Object(fields) => self.fields_hold_excess(fields, place, depth, walk)?,
            Named::Enum(union) => {
                let mut parts = Vec::new();
                for (variant, payload) in variants(union) {
                    let found = match (&union.representation, payload) {
                        (EnumRepresentation::Untagged, payload) => {
                            if self
                                .payload_holds_excess(payload, place, depth, walk)?
                                .is_some()
                            {
                                return Err(RepointError::Unsupported(format!(
                                    "the untagged enum `{name}` does not say which variant a value is"
                                )));
                            }
                            None
                        }
                        (EnumRepresentation::InternallyTagged { tag }, Payload::Inline(fields)) => {
                            self.fields_hold_excess(fields, place, depth, walk)?
                                .map(|condition| {
                                    format!("({place}.{tag} = '{variant}' AND {condition})")
                                })
                        }
                        (EnumRepresentation::AdjacentlyTagged { tag, content }, payload) => self
                            .payload_holds_excess(
                                payload,
                                &format!("{place}.{content}"),
                                depth,
                                walk,
                            )?
                            .map(|condition| {
                                format!("({place}.{tag} = '{variant}' AND {condition})")
                            }),
                        // Externally tagged, and internally tagged newtype
                        // variants, which serialize externally tagged.
                        (_, payload) => {
                            let at = format!("{place}.{}", surql_ident(variant));
                            self.payload_holds_excess(payload, &at, depth, walk)?
                                .map(|condition| format!("({at} != NONE AND {condition})"))
                        }
                    };
                    parts.push(found);
                }
                any_of(parts)
            }
        };
        walk.visiting.pop();
        Ok(found)
    }

    /// Whether a value of `ty` can hold a deleted id at any depth.
    fn reaches_excess(&self, ty: &FieldType, seen: &mut BTreeSet<String>) -> bool {
        if let Some(type_name) = self.linked_type(ty) {
            return self.has_excess(type_name);
        }
        match ty {
            FieldType::Option(inner) | FieldType::Vec(inner) => self.reaches_excess(inner, seen),
            FieldType::HashMap(_, value) | FieldType::BTreeMap(_, value) => {
                self.reaches_excess(value, seen)
            }
            FieldType::Tuple(types) => types.iter().any(|member| self.reaches_excess(member, seen)),
            FieldType::Struct(fields) => fields
                .iter()
                .any(|(_, member)| self.reaches_excess(member, seen)),
            FieldType::Other(name) if seen.insert(name.clone()) => match self.named(name) {
                Some(Named::Object(fields)) => fields
                    .iter()
                    .any(|field| self.reaches_excess(&field.field_type, seen)),
                Some(Named::Enum(union)) => variants(union).any(|(_, payload)| match payload {
                    Payload::Inline(fields) => fields
                        .iter()
                        .any(|field| self.reaches_excess(&field.field_type, seen)),
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
        place: &str,
        depth: usize,
        walk: &mut Walk,
    ) -> Result<Option<String>, RepointError> {
        Ok(any_of(
            fields
                .iter()
                .map(|field| {
                    self.holds_excess(
                        &field.field_type,
                        &format!("{place}.{}", surql_ident(field.db_name())),
                        depth,
                        walk,
                    )
                })
                .collect::<Result<_, _>>()?,
        ))
    }

    fn payload_holds_excess(
        &self,
        payload: Payload<'_>,
        place: &str,
        depth: usize,
        walk: &mut Walk,
    ) -> Result<Option<String>, RepointError> {
        match payload {
            Payload::Inline(fields) => self.fields_hold_excess(fields, place, depth, walk),
            Payload::Type(ty) => self.holds_excess(ty, place, depth, walk),
        }
    }

    /// The value at `v` with its deleted ids replaced, for a `ty` that can
    /// hold them.
    fn repointed(
        &self,
        ty: &FieldType,
        place: &str,
        depth: usize,
        walk: &mut Walk,
    ) -> Result<String, RepointError> {
        if let Some(type_name) = self.linked_type(ty) {
            if !self.has_kept(type_name) {
                return Err(RepointError::Unfillable(
                    self.link_target_tables(type_name).join(" or "),
                ));
            }
            walk.linked.insert(type_name.to_string());
            let suffix = parameter_suffix(type_name);
            return Ok(format!(
                "(IF <string> {place} IN $deleted_{suffix} THEN <record> rand::enum($kept_{suffix}) \
                 ELSE {place} END)"
            ));
        }
        let item = format!("$x{depth}");
        match ty {
            FieldType::Option(inner) => match self.repointed(inner, place, depth, walk) {
                Ok(replacement) => Ok(format!(
                    "(IF {place} = NULL OR {place} = NONE THEN {place} ELSE {replacement} END)"
                )),
                Err(RepointError::Unfillable(_)) => {
                    let condition = self.required_holds(inner, place, depth, walk)?;
                    Ok(format!(
                        "(IF {place} != NULL AND {place} != NONE AND {condition} THEN NULL ELSE {place} END)"
                    ))
                }
                Err(union) => Err(union),
            },
            FieldType::Vec(inner) => match self.repointed(inner, &item, depth + 1, walk) {
                Ok(replacement) => Ok(format!("array::map({place}, |{item}| {replacement})")),
                Err(RepointError::Unfillable(_)) => {
                    let condition = self.required_holds(inner, &item, depth + 1, walk)?;
                    Ok(format!("array::filter({place}, |{item}| !{condition})"))
                }
                Err(union) => Err(union),
            },
            FieldType::HashMap(_, value) | FieldType::BTreeMap(_, value) => {
                let at = format!("{item}[1]");
                match self.repointed(value, &at, depth + 1, walk) {
                    Ok(replacement) => Ok(format!(
                        "object::from_entries(array::map(object::entries({place}), |{item}| [{item}[0], {replacement}]))"
                    )),
                    Err(RepointError::Unfillable(_)) => {
                        let condition = self.required_holds(value, &at, depth + 1, walk)?;
                        Ok(format!(
                            "object::from_entries(array::filter(object::entries({place}), |{item}| !{condition}))"
                        ))
                    }
                    Err(union) => Err(union),
                }
            }
            FieldType::Tuple(types) => {
                let items = types
                    .iter()
                    .enumerate()
                    .map(|(index, member)| {
                        self.repointed_or_same(member, &format!("{place}[{index}]"), depth, walk)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(format!("[{}]", items.join(", ")))
            }
            FieldType::Struct(fields) => {
                let mut assignments = Vec::new();
                for (key, member) in fields {
                    let at = format!("{place}.{key}");
                    if self.holds_excess(member, &at, depth, walk)?.is_some() {
                        assignments.push(format!(
                            "{key}: {}",
                            self.repointed(member, &at, depth, walk)?
                        ));
                    }
                }
                Ok(format!(
                    "object::extend({place}, {{ {} }})",
                    assignments.join(", ")
                ))
            }
            FieldType::Other(name) => self.named_repointed(name, place, depth, walk),
            _ => Ok(place.to_string()),
        }
    }

    /// The condition for a part that `repointed` found unfillable, which
    /// therefore holds deleted ids.
    fn required_holds(
        &self,
        ty: &FieldType,
        place: &str,
        depth: usize,
        walk: &mut Walk,
    ) -> Result<String, RepointError> {
        self.holds_excess(ty, place, depth, walk)?.ok_or_else(|| {
            RepointError::Unsupported(format!("no deleted id was found at `{place}`"))
        })
    }

    fn repointed_or_same(
        &self,
        ty: &FieldType,
        place: &str,
        depth: usize,
        walk: &mut Walk,
    ) -> Result<String, RepointError> {
        match self.holds_excess(ty, place, depth, walk)? {
            Some(_) => self.repointed(ty, place, depth, walk),
            None => Ok(place.to_string()),
        }
    }

    fn named_repointed(
        &self,
        name: &str,
        place: &str,
        depth: usize,
        walk: &mut Walk,
    ) -> Result<String, RepointError> {
        let Some(named) = self.named(name) else {
            return Ok(place.to_string());
        };
        walk.visiting.push(name.to_string());
        let value = match named {
            Named::Object(fields) => self.fields_repointed(fields, place, depth, walk)?,
            Named::Enum(union) => {
                let mut branches = Vec::new();
                for (variant, payload) in variants(union) {
                    let branch = match (&union.representation, payload) {
                        (EnumRepresentation::InternallyTagged { tag }, Payload::Inline(fields)) => {
                            match self.fields_hold_excess(fields, place, depth, walk)? {
                                Some(condition) => Some((
                                    format!("{place}.{tag} = '{variant}' AND {condition}"),
                                    self.fields_repointed(fields, place, depth, walk)?,
                                )),
                                None => None,
                            }
                        }
                        (EnumRepresentation::AdjacentlyTagged { tag, content }, payload) => {
                            let at = format!("{place}.{content}");
                            match self.payload_holds_excess(payload, &at, depth, walk)? {
                                Some(condition) => Some((
                                    format!("{place}.{tag} = '{variant}' AND {condition}"),
                                    format!(
                                        "object::extend({place}, {{ {content}: {} }})",
                                        self.payload_repointed(payload, &at, depth, walk)?
                                    ),
                                )),
                                None => None,
                            }
                        }
                        (_, payload) => {
                            let at = format!("{place}.{}", surql_ident(variant));
                            match self.payload_holds_excess(payload, &at, depth, walk)? {
                                Some(condition) => Some((
                                    format!("{at} != NONE AND {condition}"),
                                    format!(
                                        "{{ {}: {} }}",
                                        surql_ident(variant),
                                        self.payload_repointed(payload, &at, depth, walk)?
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
                    .map(|(condition, replacement)| format!("IF {condition} THEN {replacement}"))
                    .collect::<Vec<_>>()
                    .join(" ELSE ");
                format!("({chain} ELSE {place} END)")
            }
        };
        walk.visiting.pop();
        Ok(value)
    }

    fn fields_repointed(
        &self,
        fields: &[StructField],
        place: &str,
        depth: usize,
        walk: &mut Walk,
    ) -> Result<String, RepointError> {
        let mut assignments = Vec::new();
        for field in fields {
            let at = format!("{place}.{}", surql_ident(field.db_name()));
            if self
                .holds_excess(&field.field_type, &at, depth, walk)?
                .is_some()
            {
                assignments.push(format!(
                    "{}: {}",
                    surql_ident(field.db_name()),
                    self.repointed(&field.field_type, &at, depth, walk)?
                ));
            }
        }
        Ok(format!(
            "object::extend({place}, {{ {} }})",
            assignments.join(", ")
        ))
    }

    fn payload_repointed(
        &self,
        payload: Payload<'_>,
        place: &str,
        depth: usize,
        walk: &mut Walk,
    ) -> Result<String, RepointError> {
        match payload {
            Payload::Inline(fields) => self.fields_repointed(fields, place, depth, walk),
            Payload::Type(ty) => self.repointed(ty, place, depth, walk),
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
        Some((variant.db_name(), payload))
    })
}

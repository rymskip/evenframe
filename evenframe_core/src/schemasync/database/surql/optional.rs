//! Translating stored absence values where a Rust `Option` sits, following
//! the configured NULL or NONE representation.

use crate::error::{EvenframeError, Result};
use crate::schemasync::config::OptionNone;
use crate::schemasync::table::surql_ident;
use crate::types::{
    EnumRepresentation, FieldType, StructConfig, StructField, TaggedUnion, VariantData,
};
use std::collections::BTreeMap;

/// The named types a field's type can refer to.
pub struct NamedTypes<'a> {
    pub objects: &'a BTreeMap<String, StructConfig>,
    pub enums: &'a BTreeMap<String, TaggedUnion>,
}

impl NamedTypes<'_> {
    /// Rewrites absence only where `ty` holds an `Option`, leaving other NULLs intact.
    pub fn option_absence(
        &self,
        ty: &FieldType,
        place: &str,
        target: OptionNone,
    ) -> Result<Option<String>> {
        Rewrite {
            types: self,
            visiting: Vec::new(),
            target,
        }
        .value(ty, place, 0)
    }
}

struct Rewrite<'a, 't> {
    types: &'a NamedTypes<'t>,
    visiting: Vec<String>,
    target: OptionNone,
}

impl Rewrite<'_, '_> {
    fn value(&mut self, ty: &FieldType, place: &str, depth: usize) -> Result<Option<String>> {
        let item = format!("$item{depth}");
        Ok(match ty {
            FieldType::Option(inner) => {
                let present = self
                    .value(inner, place, depth)?
                    .unwrap_or_else(|| place.to_owned());
                Some(format!(
                    "(IF {place} = NULL OR {place} = NONE THEN {} ELSE {present} END)",
                    self.target.literal(),
                ))
            }
            FieldType::Vec(inner) => self
                .value(inner, &item, depth + 1)?
                .map(|rewritten| format!("array::map({place}, |{item}| {rewritten})")),
            FieldType::HashMap(_, value) | FieldType::BTreeMap(_, value) => {
                let at = format!("{item}[1]");
                self.value(value, &at, depth + 1)?.map(|rewritten| {
                    format!(
                        "object::from_entries(array::map(object::entries({place}), |{item}| [{item}[0], {rewritten}]))"
                    )
                })
            }
            FieldType::Tuple(members) => {
                let mut rewrote = false;
                let mut items = Vec::new();
                for (index, member) in members.iter().enumerate() {
                    let at = format!("{place}[{index}]");
                    match self.value(member, &at, depth)? {
                        Some(rewritten) => {
                            rewrote = true;
                            items.push(rewritten);
                        }
                        None => items.push(at),
                    }
                }
                rewrote.then(|| format!("[{}]", items.join(", ")))
            }
            FieldType::Struct(fields) => {
                let keyed: Vec<(&str, &FieldType)> = fields
                    .iter()
                    .map(|(name, member)| (name.as_str(), member))
                    .collect();
                self.object(&keyed, place, depth)?
            }
            FieldType::Other(name) => self.named(name, place, depth)?,
            _ => None,
        })
    }

    /// An object rebuilt key by key, its rewritten keys rewritten; NONE
    /// values drop out of the literal.
    fn object(
        &mut self,
        fields: &[(&str, &FieldType)],
        place: &str,
        depth: usize,
    ) -> Result<Option<String>> {
        let mut rewrote = false;
        let mut entries = Vec::new();
        for (key, member) in fields {
            let key = surql_ident(key);
            let at = format!("{place}.{key}");
            let value = match self.value(member, &at, depth)? {
                Some(rewritten) => {
                    rewrote = true;
                    rewritten
                }
                None => at,
            };
            entries.push(format!("{key}: {value}"));
        }
        Ok(rewrote.then(|| format!("{{ {} }}", entries.join(", "))))
    }

    fn struct_fields(
        &mut self,
        fields: &[StructField],
        place: &str,
        depth: usize,
    ) -> Result<Option<String>> {
        let effective: Vec<&StructField> = fields.iter().map(StructField::effective).collect();
        let keyed: Vec<(&str, &FieldType)> = effective
            .iter()
            .map(|field| (field.db_name(), &field.field_type))
            .collect();
        self.object(&keyed, place, depth)
    }

    fn named(&mut self, name: &str, place: &str, depth: usize) -> Result<Option<String>> {
        let types = self.types;
        let object = types.objects.get(name);
        let union = types.enums.values().find(|union| union.enum_name == name);
        if object.is_none() && union.is_none() {
            return Ok(None);
        }
        if self.visiting.iter().any(|visiting| visiting == name) {
            return Err(EvenframeError::SchemaSync(format!(
                "`{name}` holds itself, so its stored option absence cannot be rewritten to {} in one \
                 statement; rewrite it before syncing",
                self.target.literal()
            )));
        }
        self.visiting.push(name.to_owned());
        let rewritten = match (object, union) {
            (Some(object), _) => self.struct_fields(&object.effective().fields, place, depth),
            (None, Some(union)) => self.union(union.effective(), place, depth),
            (None, None) => Ok(None),
        };
        self.visiting.pop();
        rewritten
    }

    fn payload(&mut self, data: &VariantData, place: &str, depth: usize) -> Result<Option<String>> {
        match data {
            VariantData::InlineStruct(config) => {
                self.struct_fields(&config.effective().fields, place, depth)
            }
            VariantData::DataStructureRef(ty) => self.value(ty, place, depth),
        }
    }

    /// Each variant that holds an `Option` rewritten where the stored value
    /// is that variant, as its representation tells.
    fn union(&mut self, union: &TaggedUnion, place: &str, depth: usize) -> Result<Option<String>> {
        let mut branches = Vec::new();
        for variant in &union.variants {
            let variant = variant.effective();
            let Some(data) = &variant.data else {
                continue;
            };
            let name = variant.db_name();
            let branch = match variant.stored_representation(&union.representation) {
                EnumRepresentation::ExternallyTagged => {
                    let key = surql_ident(name);
                    let at = format!("{place}.{key}");
                    self.payload(data, &at, depth)?.map(|rewritten| {
                        (format!("{at} != NONE"), format!("{{ {key}: {rewritten} }}"))
                    })
                }
                EnumRepresentation::InternallyTagged { tag } => {
                    let tag_key = surql_ident(tag);
                    self.payload(data, place, depth)?.map(|rewritten| {
                        (
                            format!("{place}.{tag_key} = '{name}'"),
                            format!("object::extend({rewritten}, {{ {tag_key}: '{name}' }})"),
                        )
                    })
                }
                EnumRepresentation::AdjacentlyTagged { tag, content } => {
                    let (tag_key, content_key) = (surql_ident(tag), surql_ident(content));
                    let at = format!("{place}.{content_key}");
                    self.payload(data, &at, depth)?.map(|rewritten| {
                        (
                            format!("{place}.{tag_key} = '{name}'"),
                            format!("{{ {tag_key}: '{name}', {content_key}: {rewritten} }}"),
                        )
                    })
                }
                EnumRepresentation::Untagged => {
                    if self.payload(data, place, depth)?.is_some() {
                        return Err(EvenframeError::SchemaSync(format!(
                            "`{}` is untagged, so a stored value does not say which variant's \
                             NULLs to rewrite to NONE; rewrite them before syncing",
                            union.enum_name
                        )));
                    }
                    None
                }
            };
            branches.extend(branch);
        }
        if branches.is_empty() {
            return Ok(None);
        }
        let conditions = branches
            .into_iter()
            .map(|(condition, rewritten)| format!("IF {condition} THEN {rewritten}"))
            .collect::<Vec<_>>()
            .join(" ELSE ");
        Ok(Some(format!("({conditions} ELSE {place} END)")))
    }
}

#[cfg(test)]
mod tests {
    use super::NamedTypes;
    use crate::schemasync::config::OptionNone;
    use crate::types::{FieldType, StructConfig, StructField};
    use std::collections::BTreeMap;

    fn field(name: &str, field_type: FieldType) -> StructField {
        StructField {
            field_name: name.to_owned(),
            field_type,
            ..StructField::default()
        }
    }

    #[test]
    fn options_inside_lists_and_objects_are_rewritten() {
        let address = StructConfig {
            struct_name: "Address".to_owned(),
            fields: vec![
                field("city", FieldType::String),
                field("zip", FieldType::Option(Box::new(FieldType::String))),
            ],
            ..StructConfig::default()
        };
        let objects = BTreeMap::from([("Address".to_owned(), address)]);
        let enums = BTreeMap::new();
        let types = NamedTypes {
            objects: &objects,
            enums: &enums,
        };
        let rewritten = types
            .option_absence(
                &FieldType::Vec(Box::new(FieldType::Other("Address".to_owned()))),
                "addresses",
                OptionNone::None,
            )
            .expect("the rewrite builds");
        assert_eq!(
            rewritten.as_deref(),
            Some(
                "array::map(addresses, |$item0| { city: $item0.city, zip: (IF $item0.zip = NULL OR \
                  $item0.zip = NONE THEN NONE ELSE $item0.zip END) })"
            )
        );
        assert_eq!(
            types
                .option_absence(&FieldType::String, "name", OptionNone::None)
                .expect("nothing to rewrite"),
            None
        );
        assert_eq!(
            types
                .option_absence(
                    &FieldType::Vec(Box::new(FieldType::Other("Address".to_owned()))),
                    "addresses",
                    OptionNone::Null,
                )
                .unwrap()
                .as_deref(),
            Some(
                "array::map(addresses, |$item0| { city: $item0.city, zip: (IF $item0.zip = NULL OR $item0.zip = NONE THEN NULL ELSE $item0.zip END) })"
            )
        );
    }
}

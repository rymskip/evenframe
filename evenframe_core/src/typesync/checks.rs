//! Checks the scanned types before any typesync output is written: every
//! type a generated type names must itself be generated or foreign, and every
//! map key must have a JSON object-key form.

use crate::error::{EvenframeError, Result};
use crate::types::{
    AllConfigs, EnumRepresentation, FieldType, ForeignTypeRegistry, NewtypeConfig, Pipeline,
    StructField, TaggedUnion, VariantData,
};
use crate::typesync::map_key::{MapKey, UNSUPPORTED};
use convert_case::{Case, Casing};
use std::collections::{BTreeMap, BTreeSet};

/// How a scanned type takes part in typesync.
#[derive(Clone, Copy)]
struct Scanned {
    pipeline: Pipeline,
    resolve_only: bool,
}

fn is_emitted(pipeline: Pipeline, resolve_only: bool) -> bool {
    pipeline.includes_typesync() && !resolve_only
}

/// Rejects every field of an emitted type that no output can generate,
/// listing each with the reason and its fix.
pub fn check_types(configs: &AllConfigs, registry: &ForeignTypeRegistry) -> Result<()> {
    let enums = &configs.enums;
    let structs = configs
        .objects
        .values()
        .chain(configs.tables.values().map(|table| &table.struct_config));
    let struct_names: BTreeSet<String> = structs
        .clone()
        .map(|struct_config| struct_config.struct_name.to_case(Case::Pascal))
        .collect();
    let mut scanned: BTreeMap<String, Scanned> = BTreeMap::new();
    for struct_config in structs.clone() {
        record(
            &mut scanned,
            &struct_config.struct_name,
            struct_config.pipeline,
            struct_config.resolve_only,
        );
    }
    for tagged_union in enums.values() {
        record(
            &mut scanned,
            &tagged_union.enum_name,
            tagged_union.pipeline,
            tagged_union.resolve_only,
        );
    }
    for newtype in configs.newtypes.values() {
        record(
            &mut scanned,
            &newtype.name,
            newtype.pipeline,
            newtype.resolve_only,
        );
    }
    let newtypes_by_name: BTreeMap<String, &NewtypeConfig> = configs
        .newtypes
        .values()
        .map(|newtype| (newtype.name.to_case(Case::Pascal), newtype))
        .collect();
    // A newtype is written as its inner type, so what it holds decides.
    let underlying = |field_type: &FieldType| -> FieldType {
        let mut current = field_type.clone();
        for _ in 0..=newtypes_by_name.len() {
            match &current {
                FieldType::Other(name) => match newtypes_by_name.get(&name.to_case(Case::Pascal)) {
                    Some(newtype) => current = newtype.inner.clone(),
                    None => break,
                },
                _ => break,
            }
        }
        current
    };

    let enums_by_name: BTreeMap<String, &TaggedUnion> = enums
        .values()
        .map(|tagged_union| {
            let tagged_union = tagged_union.effective();
            (tagged_union.enum_name.to_case(Case::Pascal), tagged_union)
        })
        .collect();

    let mut problems = BTreeSet::new();
    let mut check = |location: String, field_type: &FieldType| {
        let mut names = BTreeSet::new();
        let mut keys = Vec::new();
        collect(field_type, &mut names, &mut keys);
        for key in keys {
            if let Some((described, reason)) =
                unkeyable(&underlying(key), registry, &scanned, &enums_by_name)
            {
                problems.insert(format!(
                    "{location} holds a map keyed by {described}, so it {reason}."
                ));
            }
        }
        for name in names {
            if registry.lookup(&name).is_some() {
                continue;
            }
            match scanned.get(&name.to_case(Case::Pascal)) {
                Some(found) if is_emitted(found.pipeline, found.resolve_only) => {}
                Some(found) if found.resolve_only => {
                    problems.insert(format!(
                        "{location} names `{name}`, which comes from a `resolve_only` entry in \
                         `general.include_files`, so no output declares it. Remove `resolve_only` \
                         from that entry to generate it here."
                    ));
                }
                Some(_) => {
                    problems.insert(format!(
                        "{location} names `{name}`, which derives `Schemasync`, so it is left out \
                         of typesync. Derive `Evenframe` or `Typesync` on it instead."
                    ));
                }
                None => {
                    problems.insert(format!(
                        "{location} names `{name}`, which is neither a scanned type nor a foreign \
                         type. Derive `Evenframe` on it, directly or through an `#[apply(alias)]` \
                         whose alias is listed in `general.apply_aliases`; if it is defined \
                         outside the scanned directory, add its file or directory to \
                         `general.include_files`; if it comes from another crate, map it in \
                         `general.foreign_types`."
                    ));
                }
            }
        }
    };

    for struct_config in structs {
        if !is_emitted(struct_config.pipeline, struct_config.resolve_only) {
            continue;
        }
        let struct_config = struct_config.effective();
        for field in &struct_config.fields {
            check(
                field_location(&struct_config.struct_name, field),
                &field.effective().field_type,
            );
        }
    }
    for newtype in configs.newtypes.values() {
        if is_emitted(newtype.pipeline, newtype.resolve_only) {
            check(format!("`{}.0`", newtype.name), &newtype.inner);
        }
    }
    let mut untaggable = Vec::new();
    for tagged_union in enums.values() {
        if !is_emitted(tagged_union.pipeline, tagged_union.resolve_only) {
            continue;
        }
        let tagged_union = tagged_union.effective();
        for variant in &tagged_union.variants {
            let variant = variant.effective();
            let internally_tagged = matches!(
                variant.serde_representation(&tagged_union.representation),
                EnumRepresentation::InternallyTagged { .. }
            );
            let owner = format!("{}::{}", tagged_union.enum_name, variant.name);
            match &variant.data {
                Some(VariantData::InlineStruct(inline)) => {
                    for field in &inline.fields {
                        check(field_location(&owner, field), &field.effective().field_type);
                    }
                }
                Some(VariantData::DataStructureRef(field_type)) => {
                    check(format!("`{owner}`"), field_type);
                    let holds_struct = match &underlying(field_type) {
                        FieldType::Other(name) => {
                            struct_names.contains(&name.to_case(Case::Pascal))
                        }
                        FieldType::Struct(_) => true,
                        _ => false,
                    };
                    if internally_tagged && !holds_struct {
                        untaggable.push(format!(
                            "`{owner}` holds `{}` in an internally tagged enum, but serde writes \
                             the tag into the value the variant holds, which only a struct can \
                             take. Hold a struct, or make it a struct variant.",
                            field_type.canonical_name()
                        ));
                    }
                }
                None => {}
            }
        }
    }
    problems.extend(untaggable);

    if problems.is_empty() {
        return Ok(());
    }
    let subject = match problems.len() {
        1 => "1 problem stops".to_string(),
        count => format!("{count} problems stop"),
    };
    Err(EvenframeError::type_sync(format!(
        "{subject} typesync before it writes anything:\n  - {}",
        problems.into_iter().collect::<Vec<_>>().join("\n  - ")
    )))
}

fn field_location(owner: &str, field: &StructField) -> String {
    format!("`{owner}.{}`", field.effective().field_name)
}

/// Records a scanned type under its PascalCase name. A name scanned more than
/// once counts as emitted when any of its definitions is.
fn record(
    scanned: &mut BTreeMap<String, Scanned>,
    name: &str,
    pipeline: Pipeline,
    resolve_only: bool,
) {
    let entry = Scanned {
        pipeline,
        resolve_only,
    };
    scanned
        .entry(name.to_case(Case::Pascal))
        .and_modify(|existing| {
            if is_emitted(pipeline, resolve_only) {
                *existing = entry;
            }
        })
        .or_insert(entry);
}

/// The key described and why no map can be keyed by it, or `None` when one
/// can. A name that resolves to nothing is reported as an unknown name instead.
fn unkeyable(
    key: &FieldType,
    registry: &ForeignTypeRegistry,
    scanned: &BTreeMap<String, Scanned>,
    enums_by_name: &BTreeMap<String, &TaggedUnion>,
) -> Option<(String, &'static str)> {
    let name = match MapKey::of(key) {
        Err(reason) => return Some((format!("`{}`", key.canonical_name()), reason)),
        Ok(MapKey::Named(name)) => name,
        Ok(_) => return None,
    };
    if registry.lookup(name).is_some() {
        return None;
    }
    let pascal = name.to_case(Case::Pascal);
    if let Some(tagged_union) = enums_by_name.get(&pascal) {
        let representation = match &tagged_union.representation {
            EnumRepresentation::ExternallyTagged => None,
            EnumRepresentation::InternallyTagged { .. } => Some("internally tagged"),
            EnumRepresentation::AdjacentlyTagged { .. } => Some("adjacently tagged"),
            EnumRepresentation::Untagged => Some("untagged"),
        };
        if let Some(representation) = representation {
            return Some((format!("`{name}`, an {representation} enum"), UNSUPPORTED));
        }
        if let Some(variant) = tagged_union
            .variants
            .iter()
            .map(|variant| variant.effective())
            .find(|variant| variant.wire.serde_untagged)
        {
            return Some((
                format!("`{name}`, whose variant `{}` is untagged", variant.name),
                UNSUPPORTED,
            ));
        }
        return tagged_union
            .variants
            .iter()
            .map(|variant| variant.effective())
            .find(|variant| variant.data.is_some())
            .map(|variant| {
                (
                    format!("`{name}`, whose variant `{}` carries data", variant.name),
                    UNSUPPORTED,
                )
            });
    }
    scanned
        .contains_key(&pascal)
        .then(|| (format!("the struct `{name}`"), UNSUPPORTED))
}

/// Every named type in `field_type`, and every map key in it.
fn collect<'a>(
    field_type: &'a FieldType,
    names: &mut BTreeSet<String>,
    keys: &mut Vec<&'a FieldType>,
) {
    match field_type {
        FieldType::Tuple(items) => items.iter().for_each(|item| collect(item, names, keys)),
        FieldType::Struct(fields) => fields
            .iter()
            .for_each(|(_, field)| collect(field, names, keys)),
        FieldType::Option(inner) | FieldType::Vec(inner) | FieldType::RecordLink(inner) => {
            collect(inner, names, keys)
        }
        FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => {
            keys.push(key);
            collect(key, names, keys);
            collect(value, names, keys);
        }
        FieldType::Other(name) => {
            names.insert(name.clone());
        }
        FieldType::String
        | FieldType::Char
        | FieldType::Bool
        | FieldType::Unit
        | FieldType::F32
        | FieldType::F64
        | FieldType::I8
        | FieldType::I16
        | FieldType::I32
        | FieldType::I64
        | FieldType::I128
        | FieldType::Isize
        | FieldType::U8
        | FieldType::U16
        | FieldType::U32
        | FieldType::U64
        | FieldType::U128
        | FieldType::Usize
        | FieldType::Duration => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AllConfigs, BTreeMap, FieldType, ForeignTypeRegistry, Pipeline, Result, StructField,
        TaggedUnion, check_types,
    };
    use crate::schemasync::TableConfig;
    use crate::types::StructConfig;

    fn named(name: &str) -> FieldType {
        FieldType::Other(name.to_string())
    }

    fn object(name: &str, fields: Vec<(&str, FieldType)>) -> (String, StructConfig) {
        let struct_config = StructConfig {
            struct_name: name.to_string(),
            fields: fields
                .into_iter()
                .map(|(field_name, field_type)| StructField {
                    field_name: field_name.to_string(),
                    field_type,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        (name.to_string(), struct_config)
    }

    fn check(
        objects: Vec<(String, StructConfig)>,
        enums: Vec<TaggedUnion>,
        registry: &ForeignTypeRegistry,
    ) -> Result<()> {
        let enums = enums
            .into_iter()
            .map(|tagged_union| (tagged_union.enum_name.clone(), tagged_union))
            .collect();
        check_types(
            &AllConfigs {
                enums,
                objects: objects.into_iter().collect(),
                ..AllConfigs::default()
            },
            registry,
        )
    }

    #[test]
    fn references_to_generated_and_foreign_types_resolve() {
        let foreign: crate::config::ForeignTypeConfig =
            toml::from_str("rust_type_names = [\"DateTime\"]").unwrap();
        let registry =
            ForeignTypeRegistry::from_config(&BTreeMap::from([("DateTime".to_string(), foreign)]));
        let objects = vec![
            object(
                "Deal",
                vec![
                    ("buyer", FieldType::Option(Box::new(named("party")))),
                    ("closes_at", named("DateTime")),
                    ("owner", FieldType::RecordLink(Box::new(named("User")))),
                ],
            ),
            object("Party", Vec::new()),
            object("User", Vec::new()),
        ];
        assert!(check(objects, Vec::new(), &registry).is_ok());
    }

    #[test]
    fn every_unresolvable_reference_is_reported_with_its_fix() {
        let (name, mut resolve_only) = object("Address", Vec::new());
        resolve_only.resolve_only = true;
        let (cache_name, mut schemasync_only) = object("CacheRow", Vec::new());
        schemasync_only.pipeline = Pipeline::Schemasync;
        let status: TaggedUnion = serde_json::from_value(serde_json::json!({
            "enum_name": "Status",
            "variants": [{ "name": "Blocked", "data": { "DataStructureRef": { "Other": "Reason" } } }],
        }))
        .unwrap();
        let objects = vec![
            object(
                "Deal",
                vec![
                    ("address", named("Address")),
                    ("cache", FieldType::Vec(Box::new(named("CacheRow")))),
                    ("party", named("Party")),
                ],
            ),
            (name, resolve_only),
            (cache_name, schemasync_only),
        ];

        let error = check(objects, vec![status], &ForeignTypeRegistry::default())
            .unwrap_err()
            .to_string();

        assert!(error.contains("4 problems stop typesync"), "{error}");
        assert!(
            error.contains(
                "`Deal.address` names `Address`, which comes from a `resolve_only` entry"
            ),
            "{error}"
        );
        assert!(
            error.contains("`Deal.cache` names `CacheRow`, which derives `Schemasync`"),
            "{error}"
        );
        assert!(
            error.contains("`Deal.party` names `Party`, which is neither a scanned type"),
            "{error}"
        );
        assert!(error.contains("`general.apply_aliases`"), "{error}");
        assert!(
            error.contains("`Status::Blocked` names `Reason`"),
            "{error}"
        );
    }

    #[test]
    fn types_that_are_not_emitted_are_not_checked() {
        let (name, mut resolve_only) = object("Address", vec![("geo", named("Missing"))]);
        resolve_only.resolve_only = true;
        assert!(
            check(
                vec![(name, resolve_only)],
                Vec::new(),
                &ForeignTypeRegistry::default()
            )
            .is_ok()
        );
    }

    #[test]
    fn a_map_key_json_cannot_write_is_reported_with_its_field() {
        let objects = vec![object(
            "Deal",
            vec![(
                "flags",
                FieldType::Vec(Box::new(FieldType::HashMap(
                    Box::new(FieldType::Unit),
                    Box::new(FieldType::String),
                ))),
            )],
        )];
        let error = check(objects, Vec::new(), &ForeignTypeRegistry::default())
            .unwrap_err()
            .to_string();
        assert!(error.contains("1 problem stops typesync"), "{error}");
        assert!(
            error.contains("`Deal.flags` holds a map keyed by `()`, so it has no JSON object key"),
            "{error}"
        );
    }

    fn tagged_union(value: serde_json::Value) -> TaggedUnion {
        serde_json::from_value(value).unwrap()
    }

    fn map(key: FieldType, value: FieldType) -> FieldType {
        FieldType::HashMap(Box::new(key), Box::new(value))
    }

    fn problems_of(
        objects: Vec<(String, StructConfig)>,
        enums: Vec<TaggedUnion>,
        registry: &ForeignTypeRegistry,
    ) -> String {
        check(objects, enums, registry).unwrap_err().to_string()
    }

    #[test]
    fn an_internally_tagged_newtype_variant_may_hold_a_projection() {
        let target = tagged_union(serde_json::json!({
            "enum_name": "Target",
            "variants": [
                { "name": "Invoice", "data": { "DataStructureRef": { "Other": "PartialInvoice" } } }
            ],
            "representation": { "InternallyTagged": { "tag": "variant" } }
        }));
        let (_, invoice) = object("Invoice", vec![("id", FieldType::String)]);
        let (key, mut projection) = object("PartialInvoice", vec![("id", FieldType::String)]);
        projection.output_override = Some(Box::new(invoice));
        assert!(
            check(
                vec![(key, projection)],
                vec![target],
                &ForeignTypeRegistry::default()
            )
            .is_ok()
        );
    }

    #[test]
    fn an_internally_tagged_newtype_variant_must_hold_a_struct() {
        let property = tagged_union(serde_json::json!({
            "enum_name": "Property",
            "variants": [
                { "name": "Text", "data": { "DataStructureRef": { "Other": "TextValue" } } },
                { "name": "Count", "data": { "DataStructureRef": "U32" } },
                { "name": "Tags", "data": { "DataStructureRef": { "Vec": "String" } } }
            ],
            "representation": { "InternallyTagged": { "tag": "variant" } }
        }));
        let objects = vec![object("TextValue", vec![("value", FieldType::String)])];
        let error = problems_of(objects, vec![property], &ForeignTypeRegistry::default());
        assert!(error.contains("2 problems stop typesync"), "{error}");
        assert!(
            error.contains(
                "`Property::Count` holds `u32` in an internally tagged enum, but serde writes the \
                 tag into the value the variant holds, which only a struct can take"
            ),
            "{error}"
        );
        assert!(
            error.contains("`Property::Tags` holds `Vec<String>`"),
            "{error}"
        );
        assert!(!error.contains("Property::Text"), "{error}");
    }

    #[test]
    fn references_are_found_at_any_depth() {
        let deep = FieldType::Option(Box::new(FieldType::Vec(Box::new(map(
            FieldType::String,
            FieldType::BTreeMap(
                Box::new(FieldType::I64),
                Box::new(FieldType::Option(Box::new(named("Deep")))),
            ),
        )))));
        let objects = vec![object(
            "Deal",
            vec![
                ("deep", deep),
                (
                    "pair",
                    FieldType::Tuple(vec![FieldType::String, named("InTuple")]),
                ),
                (
                    "inline",
                    FieldType::Struct(vec![("nested".to_string(), named("InStruct"))]),
                ),
                ("link", FieldType::RecordLink(Box::new(named("InLink")))),
            ],
        )];
        let error = problems_of(objects, Vec::new(), &ForeignTypeRegistry::default());
        assert!(error.contains("4 problems stop typesync"), "{error}");
        for expected in [
            "`Deal.deep` names `Deep`",
            "`Deal.pair` names `InTuple`",
            "`Deal.inline` names `InStruct`",
            "`Deal.link` names `InLink`",
        ] {
            assert!(error.contains(expected), "missing {expected}: {error}");
        }
    }

    #[test]
    fn a_name_used_twice_in_one_field_is_reported_once_per_field() {
        let objects = vec![object(
            "Deal",
            vec![
                (
                    "twice",
                    FieldType::Tuple(vec![named("Missing"), named("Missing")]),
                ),
                ("again", named("Missing")),
            ],
        )];
        let error = problems_of(objects, Vec::new(), &ForeignTypeRegistry::default());
        assert!(error.contains("2 problems stop typesync"), "{error}");
        assert!(error.contains("`Deal.twice` names `Missing`"), "{error}");
        assert!(error.contains("`Deal.again` names `Missing`"), "{error}");
    }

    #[test]
    fn enum_variants_are_checked_where_serde_writes_them() {
        let status = tagged_union(serde_json::json!({
            "enum_name": "Status",
            "variants": [
                { "name": "Open" },
                { "name": "Held", "data": { "InlineStruct": {
                    "struct_name": "Held",
                    "fields": [{ "field_name": "reason", "field_type": { "Other": "Reason" },
                                 "validators": [], "always_regenerate": false }],
                    "validators": []
                } } },
                { "name": "Moved", "data": { "DataStructureRef": { "Vec": { "Other": "Place" } } } },
            ],
        }));
        let error = problems_of(Vec::new(), vec![status], &ForeignTypeRegistry::default());
        assert!(
            error.contains("`Status::Held.reason` names `Reason`"),
            "{error}"
        );
        assert!(error.contains("`Status::Moved` names `Place`"), "{error}");
    }

    #[test]
    fn names_resolve_through_foreign_aliases_and_case() {
        let foreign: crate::config::ForeignTypeConfig =
            toml::from_str("rust_type_names = [\"DateTime\", \"chrono::DateTime\"]").unwrap();
        let registry =
            ForeignTypeRegistry::from_config(&BTreeMap::from([("DateTime".to_string(), foreign)]));
        let objects = vec![
            object(
                "Deal",
                vec![
                    ("at", named("chrono::DateTime")),
                    ("info", named("party_info")),
                ],
            ),
            object("PartyInfo", Vec::new()),
        ];
        assert!(check(objects, Vec::new(), &registry).is_ok());
    }

    #[test]
    fn a_name_defined_resolve_only_and_in_tree_resolves() {
        let (_, mut shared) = object("Address", Vec::new());
        shared.resolve_only = true;
        let objects = vec![
            object("Deal", vec![("address", named("Address"))]),
            ("shared::Address".to_string(), shared),
            object("Address", Vec::new()),
        ];
        assert!(check(objects, Vec::new(), &ForeignTypeRegistry::default()).is_ok());
    }

    #[test]
    fn output_overrides_are_what_is_checked() {
        let (name, mut deal) = object("Deal", vec![("party", FieldType::String)]);
        let (_, replacement) = object("Deal", vec![("party", named("Missing"))]);
        deal.output_override = Some(Box::new(replacement));
        let error = problems_of(
            vec![(name, deal)],
            Vec::new(),
            &ForeignTypeRegistry::default(),
        );
        assert!(error.contains("`Deal.party` names `Missing`"), "{error}");

        let (name, mut fixed) = object("Deal", vec![("party", named("Missing"))]);
        fixed.fields[0].output_override = Some(Box::new(StructField {
            field_name: "party".to_string(),
            field_type: FieldType::String,
            ..Default::default()
        }));
        assert!(
            check(
                vec![(name, fixed)],
                Vec::new(),
                &ForeignTypeRegistry::default()
            )
            .is_ok()
        );
    }

    #[test]
    fn table_fields_are_checked() {
        #[derive(serde::Deserialize)]
        struct Fixture {
            table_config: TableConfig,
        }
        let fixture: Fixture =
            serde_json::from_str(include_str!("../../tests/specs/surrealql/basic_table.json"))
                .unwrap();
        let mut table = fixture.table_config;
        table.struct_config.fields.push(StructField {
            field_name: "team".to_string(),
            field_type: named("Team"),
            ..Default::default()
        });
        let tables = BTreeMap::from([("user".to_string(), table)]);
        let error = check_types(
            &AllConfigs {
                tables,
                ..AllConfigs::default()
            },
            &ForeignTypeRegistry::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("`User.team` names `Team`"), "{error}");
    }

    #[test]
    fn schemasync_only_types_are_not_checked() {
        let (name, mut cache) = object("Cache", vec![("row", named("Missing"))]);
        cache.pipeline = Pipeline::Schemasync;
        assert!(
            check(
                vec![(name, cache)],
                Vec::new(),
                &ForeignTypeRegistry::default()
            )
            .is_ok()
        );
    }

    #[test]
    fn text_integer_and_char_keys_are_accepted() {
        let keys = [
            FieldType::String,
            FieldType::Char,
            FieldType::Bool,
            FieldType::I8,
            FieldType::I16,
            FieldType::I32,
            FieldType::I64,
            FieldType::I128,
            FieldType::Isize,
            FieldType::U8,
            FieldType::U16,
            FieldType::U32,
            FieldType::U64,
            FieldType::U128,
            FieldType::Usize,
        ];
        for key in keys {
            let objects = vec![object(
                "Deal",
                vec![("by", map(key.clone(), FieldType::String))],
            )];
            assert!(
                check(objects, Vec::new(), &ForeignTypeRegistry::default()).is_ok(),
                "{key:?} was rejected"
            );
        }
    }

    #[test]
    fn keys_with_no_json_object_key_are_rejected() {
        let keys = [
            FieldType::Unit,
            FieldType::Option(Box::new(FieldType::String)),
            FieldType::Vec(Box::new(FieldType::String)),
            FieldType::Tuple(vec![FieldType::String, FieldType::I32]),
            FieldType::Struct(vec![("inner".to_string(), FieldType::String)]),
            map(FieldType::String, FieldType::String),
            FieldType::RecordLink(Box::new(named("Deal"))),
        ];
        for key in keys {
            let objects = vec![object(
                "Deal",
                vec![("by", map(key.clone(), FieldType::String))],
            )];
            let error = problems_of(objects, Vec::new(), &ForeignTypeRegistry::default());
            assert!(
                error.contains(&format!(
                    "`Deal.by` holds a map keyed by `{}`, so it has no JSON object key",
                    key.canonical_name()
                )),
                "{key:?}: {error}"
            );
        }
    }

    #[test]
    fn float_keys_are_rejected_because_no_rust_map_holds_them() {
        for key in [FieldType::F32, FieldType::F64] {
            let objects = vec![object(
                "Deal",
                vec![("by", map(key.clone(), FieldType::String))],
            )];
            let error = problems_of(objects, Vec::new(), &ForeignTypeRegistry::default());
            assert!(
                error.contains(&format!(
                    "`Deal.by` holds a map keyed by `{}`, so it cannot exist: f32 and f64 \
                     implement neither Hash nor Ord",
                    key.canonical_name()
                )),
                "{key:?}: {error}"
            );
        }
    }

    #[test]
    fn a_map_key_nested_in_a_value_is_checked() {
        let objects = vec![object(
            "Deal",
            vec![(
                "by",
                map(
                    FieldType::String,
                    FieldType::Vec(Box::new(map(FieldType::F64, FieldType::String))),
                ),
            )],
        )];
        let error = problems_of(objects, Vec::new(), &ForeignTypeRegistry::default());
        assert!(
            error.contains("`Deal.by` holds a map keyed by `f64`"),
            "{error}"
        );
    }

    #[test]
    fn enum_keys_must_be_externally_tagged_unit_variants() {
        let unit = |name: &str, representation: serde_json::Value| {
            tagged_union(serde_json::json!({
                "enum_name": name,
                "variants": [{ "name": "Admin" }, { "name": "Member" }],
                "representation": representation,
            }))
        };
        let with_data = tagged_union(serde_json::json!({
            "enum_name": "Grade",
            "variants": [{ "name": "Pass" }, { "name": "Scored", "data": { "DataStructureRef": "U8" } }],
        }));
        let enums = vec![
            unit("Role", serde_json::json!("ExternallyTagged")),
            unit(
                "Inner",
                serde_json::json!({ "InternallyTagged": { "tag": "kind" } }),
            ),
            unit(
                "Adjacent",
                serde_json::json!({ "AdjacentlyTagged": { "tag": "t", "content": "c" } }),
            ),
            unit("Loose", serde_json::json!("Untagged")),
            with_data,
        ];
        let objects = vec![
            object(
                "Deal",
                vec![
                    ("by_role", map(named("Role"), FieldType::String)),
                    ("by_inner", map(named("Inner"), FieldType::String)),
                    ("by_adjacent", map(named("Adjacent"), FieldType::String)),
                    ("by_loose", map(named("Loose"), FieldType::String)),
                    ("by_grade", map(named("Grade"), FieldType::String)),
                    ("by_party", map(named("Party"), FieldType::String)),
                    ("by_missing", map(named("Missing"), FieldType::String)),
                ],
            ),
            object("Party", Vec::new()),
        ];
        let error = problems_of(objects, enums, &ForeignTypeRegistry::default());
        assert!(error.contains("6 problems stop typesync"), "{error}");
        assert!(!error.contains("by_role"), "{error}");
        for expected in [
            "`Deal.by_inner` holds a map keyed by `Inner`, an internally tagged enum",
            "`Deal.by_adjacent` holds a map keyed by `Adjacent`, an adjacently tagged enum",
            "`Deal.by_loose` holds a map keyed by `Loose`, an untagged enum",
            "`Deal.by_grade` holds a map keyed by `Grade`, whose variant `Scored` carries data",
            "`Deal.by_party` holds a map keyed by the struct `Party`",
            "`Deal.by_missing` names `Missing`, which is neither a scanned type",
        ] {
            assert!(error.contains(expected), "missing {expected}: {error}");
        }
    }

    #[test]
    fn a_foreign_key_is_accepted() {
        let foreign: crate::config::ForeignTypeConfig =
            toml::from_str("rust_type_names = [\"Uuid\"]\narktype = { type = \"'string'\" }")
                .unwrap();
        let registry =
            ForeignTypeRegistry::from_config(&BTreeMap::from([("Uuid".to_string(), foreign)]));
        let objects = vec![object(
            "Deal",
            vec![("by", map(named("Uuid"), FieldType::String))],
        )];
        assert!(check(objects, Vec::new(), &registry).is_ok());
    }
}

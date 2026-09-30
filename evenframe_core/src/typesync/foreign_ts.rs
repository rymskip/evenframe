//! Foreign types in the TypeScript outputs: their types, with generic
//! parameters filled in, and the imports they need.

use crate::config::{ForeignTypeConfig, TsImport};
use crate::types::{FieldType, ForeignTypeRegistry, StructConfig, TaggedUnion, VariantData};
use convert_case::{Case, Casing};
use std::collections::{BTreeMap, BTreeSet};

/// The foreign type config for evenframe's record link, which a project may
/// configure to own its TypeScript definition.
pub const RECORD_LINK: &str = "RecordLink";

/// `type_expr` with `{0}`, `{1}` and so on replaced by `params`. Config
/// loading rejects a placeholder no parameter can fill.
pub fn fill(type_expr: &str, params: &[String]) -> String {
    params
        .iter()
        .enumerate()
        .fold(type_expr.to_string(), |filled, (index, param)| {
            filled.replace(&format!("{{{index}}}"), param)
        })
}

/// The generic parameter indices `type_expr` has placeholders for.
pub fn placeholders(type_expr: &str) -> Vec<usize> {
    type_expr
        .split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}'))
        .filter_map(|(digits, _)| digits.parse().ok())
        .collect()
}

/// The foreign types a set of generated types uses, and whether they use a
/// record link.
#[derive(Default)]
pub struct ForeignUse<'a> {
    pub foreign: BTreeMap<String, &'a ForeignTypeConfig>,
    pub record_link: bool,
}

/// How an output reads the types it writes.
pub struct Reading {
    /// The struct an output writes for a struct config.
    pub struct_view: fn(&StructConfig) -> &StructConfig,
    /// The enum an output writes for an enum config.
    pub enum_view: fn(&TaggedUnion) -> &TaggedUnion,
    /// Whether the output expands a struct or enum a type holds into that
    /// type's file, so the held type's foreign types are used there too.
    pub expands_held_types: bool,
}

/// The foreign types the written types named in `type_names` use, as
/// `reading` reads them.
pub fn foreign_types_used<'a>(
    type_names: &[String],
    structs: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &'a ForeignTypeRegistry,
    reading: &Reading,
) -> ForeignUse<'a> {
    let wanted: BTreeSet<String> = type_names.iter().cloned().collect();
    let mut walker = Walker {
        structs,
        enums,
        registry,
        reading,
        visited: BTreeSet::new(),
        used: ForeignUse::default(),
    };
    for struct_config in structs.values() {
        if !struct_config.resolve_only
            && wanted.contains(&struct_config.struct_name.to_case(Case::Pascal))
        {
            walker
                .visited
                .insert(struct_config.struct_name.to_case(Case::Pascal));
            walker.fields(struct_config);
        }
    }
    for tagged_union in enums.values() {
        if !tagged_union.resolve_only
            && wanted.contains(&tagged_union.enum_name.to_case(Case::Pascal))
        {
            walker
                .visited
                .insert(tagged_union.enum_name.to_case(Case::Pascal));
            walker.variants(tagged_union);
        }
    }
    walker.used
}

struct Walker<'a, 'b> {
    structs: &'b BTreeMap<String, StructConfig>,
    enums: &'b BTreeMap<String, TaggedUnion>,
    registry: &'a ForeignTypeRegistry,
    reading: &'b Reading,
    visited: BTreeSet<String>,
    used: ForeignUse<'a>,
}

impl Walker<'_, '_> {
    fn fields(&mut self, struct_config: &StructConfig) {
        for field in &(self.reading.struct_view)(struct_config).fields {
            self.field_type(&field.effective().field_type);
        }
    }

    fn variants(&mut self, tagged_union: &TaggedUnion) {
        for variant in &(self.reading.enum_view)(tagged_union).variants {
            match &variant.effective().data {
                Some(VariantData::InlineStruct(inline)) => self.fields(inline),
                Some(VariantData::DataStructureRef(field_type)) => self.field_type(field_type),
                None => {}
            }
        }
    }

    fn field_type(&mut self, field_type: &FieldType) {
        match field_type {
            // The linked table is only named here, never expanded, so its own
            // foreign types belong to its file.
            FieldType::RecordLink(_) => self.used.record_link = true,
            FieldType::Option(inner) | FieldType::Vec(inner) => self.field_type(inner),
            FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => {
                self.field_type(key);
                self.field_type(value);
            }
            FieldType::Tuple(items) => items.iter().for_each(|item| self.field_type(item)),
            FieldType::Struct(fields) => fields
                .iter()
                .for_each(|(_, member)| self.field_type(member)),
            FieldType::Other(name) => {
                if let Some(foreign) = self.registry.lookup(name) {
                    self.used.foreign.insert(name.clone(), foreign);
                    return;
                }
                let pascal = name.to_case(Case::Pascal);
                if !self.reading.expands_held_types || !self.visited.insert(pascal.clone()) {
                    return;
                }
                // Tables are held under their table name, so a lookup by type
                // name finds only embedded types: a table held by value is a
                // link to it and is never expanded.
                if let Some(struct_config) =
                    self.structs.get(name).or_else(|| self.structs.get(&pascal))
                {
                    self.fields(struct_config);
                } else if let Some(tagged_union) =
                    self.enums.get(name).or_else(|| self.enums.get(&pascal))
                {
                    self.variants(tagged_union);
                }
            }
            _ => {}
        }
    }
}

/// Import lines for `imports`, one per imported name, sorted by name. A name
/// imported both as a type and as a value is imported as a value.
pub fn import_lines<'a>(imports: impl IntoIterator<Item = &'a TsImport>) -> Vec<String> {
    let mut by_name: BTreeMap<(&str, &str), bool> = BTreeMap::new();
    for import in imports {
        let type_only = by_name
            .entry((import.name.as_str(), import.from.as_str()))
            .or_insert(true);
        *type_only &= import.type_only;
    }
    by_name
        .into_iter()
        .map(|((name, module), type_only)| {
            let keyword = if type_only { "import type" } else { "import" };
            format!("{keyword} {{ {name} }} from '{module}';")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_take_generic_parameters() {
        assert_eq!(
            fill("RecordLink<{0}>", &["Order".to_string()]),
            "RecordLink<Order>"
        );
        assert_eq!(placeholders("Pair<{0}, {1}>"), vec![0, 1]);
        assert!(placeholders("{ on: boolean }").is_empty());
    }

    #[test]
    fn a_type_that_is_never_written_uses_no_foreign_types() {
        let date_time: ForeignTypeConfig =
            toml::from_str("rust_type_names = [\"DateTime\"]").unwrap();
        let registry = ForeignTypeRegistry::from_config(&BTreeMap::from([(
            "DateTime".to_string(),
            date_time,
        )]));
        let dated = |struct_name: &str, resolve_only: bool| StructConfig {
            struct_name: struct_name.to_string(),
            fields: vec![crate::types::StructField {
                field_name: "at".to_string(),
                field_type: FieldType::Other("DateTime".to_string()),
                ..Default::default()
            }],
            resolve_only,
            ..Default::default()
        };
        let reading = Reading {
            struct_view: StructConfig::effective,
            enum_view: TaggedUnion::effective,
            expands_held_types: false,
        };
        let used = |struct_config: StructConfig| {
            let name = struct_config.struct_name.clone();
            let structs = BTreeMap::from([(name.clone(), struct_config)]);
            foreign_types_used(&[name], &structs, &BTreeMap::new(), &registry, &reading)
                .foreign
                .into_keys()
                .collect::<Vec<_>>()
        };
        assert_eq!(used(dated("Event", false)), vec!["DateTime".to_string()]);
        assert!(used(dated("Borrowed", true)).is_empty());
    }

    #[test]
    fn imports_are_one_line_per_name() {
        let import = |from: &str, name: &str, type_only: bool| TsImport {
            from: from.to_string(),
            name: name.to_string(),
            type_only,
        };
        let imports = [
            import("effect", "DateTime", false),
            import("effect", "BigDecimal", false),
            import("./index", "RecordLink", true),
            import("effect", "DateTime", true),
        ];
        assert_eq!(
            import_lines(&imports),
            vec![
                "import { BigDecimal } from 'effect';".to_string(),
                "import { DateTime } from 'effect';".to_string(),
                "import type { RecordLink } from './index';".to_string(),
            ]
        );
    }
}

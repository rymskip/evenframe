use crate::{
    schemasync::TableConfig,
    types::{
        DeclaredTypes, FieldType, NewtypeConfig, StructConfig, StructField, TaggedUnion, Variant,
        VariantData, desugar_newtypes,
    },
    validator::{Validator, ValidatorOverrides},
};
use convert_case::{Case, Casing};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Every type a scan found, by kind.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct AllConfigs {
    pub enums: BTreeMap<String, TaggedUnion>,
    /// Structs with a record id, keyed by their snake_case table name.
    pub tables: BTreeMap<String, TableConfig>,
    /// Every struct by its name, tables included.
    pub objects: BTreeMap<String, StructConfig>,
    /// Structs serde writes as another type, by name.
    #[serde(default)]
    pub newtypes: BTreeMap<String, NewtypeConfig>,
}

/// The types schemasync works with: each in the form the database stores it,
/// with the types as declared wherever that form replaced a newtype.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SchemasyncTypes {
    pub enums: BTreeMap<String, TaggedUnion>,
    pub tables: BTreeMap<String, TableConfig>,
    pub objects: BTreeMap<String, StructConfig>,
    pub declared: DeclaredTypes,
}

impl AllConfigs {
    /// The types that take part in the typesync pipeline, copied out, with
    /// each flattened struct's fields in place of the field holding it and
    /// without the fields and variants serde skips, which its JSON never
    /// carries.
    pub fn for_typesync(&self) -> crate::Result<AllConfigs> {
        let flattening = Flattening {
            structs: &self.objects,
            flattened: |wire| wire.serde_flatten,
        };
        Ok(AllConfigs {
            enums: self
                .enums
                .iter()
                .filter(|(_, tagged_union)| tagged_union.pipeline.includes_typesync())
                .map(|(name, tagged_union)| {
                    let mut tagged_union = tagged_union.clone();
                    flattening.tagged_union(&mut tagged_union)?;
                    drop_serde_skipped_variants(&mut tagged_union);
                    Overrides::Typesync.tagged_union(&mut tagged_union);
                    Ok((name.clone(), tagged_union))
                })
                .collect::<crate::Result<_>>()?,
            tables: self
                .tables
                .iter()
                .filter(|(_, table)| table.struct_config.pipeline.includes_typesync())
                .map(|(name, table)| {
                    let mut table = table.clone();
                    flattening.table(&mut table)?;
                    drop_serde_skipped_table_fields(&mut table);
                    Overrides::Typesync.table(&mut table);
                    Ok((name.clone(), table))
                })
                .collect::<crate::Result<_>>()?,
            objects: self
                .objects
                .iter()
                .filter(|(_, object)| object.pipeline.includes_typesync())
                .map(|(name, object)| {
                    let mut object = object.clone();
                    flattening.struct_config(&mut object)?;
                    drop_serde_skipped_fields(&mut object);
                    Overrides::Typesync.struct_config(&mut object);
                    Ok((name.clone(), object))
                })
                .collect::<crate::Result<_>>()?,
            newtypes: self
                .newtypes
                .iter()
                .filter(|(_, newtype)| newtype.pipeline.includes_typesync())
                .map(|(name, newtype)| {
                    let mut newtype = newtype.clone();
                    Overrides::Typesync.newtype(&mut newtype);
                    (name.clone(), newtype)
                })
                .collect(),
        })
    }

    /// The types that take part in the schemasync pipeline, as the database
    /// stores them: each flattened struct's fields in place of the field
    /// holding it, without the fields it never holds, and with every newtype
    /// rewritten to its inner type.
    pub fn into_schemasync(self) -> crate::Result<SchemasyncTypes> {
        let AllConfigs {
            enums,
            tables,
            objects: every_object,
            mut newtypes,
        } = self;
        // A struct flattened into a schemasync type may itself be typesync only.
        let flattening = Flattening {
            structs: &every_object,
            flattened: |wire| wire.storage.flatten,
        };
        let mut enums: BTreeMap<String, TaggedUnion> = enums
            .into_iter()
            .filter(|(_, tagged_union)| tagged_union.pipeline.includes_schemasync())
            .collect();
        let mut tables: BTreeMap<String, TableConfig> = tables
            .into_iter()
            .filter(|(_, table)| table.struct_config.pipeline.includes_schemasync())
            .collect();
        let mut objects: BTreeMap<String, StructConfig> = every_object
            .iter()
            .filter(|(_, object)| object.pipeline.includes_schemasync())
            .map(|(name, object)| (name.clone(), object.clone()))
            .collect();
        for table in tables.values_mut() {
            flattening.table(table)?;
            drop_unstored_table_fields(table);
            Overrides::Schemasync.table(table);
        }
        for object in objects.values_mut() {
            flattening.struct_config(object)?;
            drop_unstored_fields(object);
            Overrides::Schemasync.struct_config(object);
        }
        for tagged_union in enums.values_mut() {
            flattening.tagged_union(tagged_union)?;
            drop_unstored_variant_fields(tagged_union);
            Overrides::Schemasync.tagged_union(tagged_union);
        }
        for newtype in newtypes.values_mut() {
            Overrides::Schemasync.newtype(newtype);
        }
        let declared = desugar_newtypes(&newtypes, &mut enums, &mut tables, &mut objects)?;
        Ok(SchemasyncTypes {
            enums,
            tables,
            objects,
            declared,
        })
    }
}

/// Puts each flattened struct's fields in place of the field holding it.
struct Flattening<'a> {
    /// Every struct, whatever its pipeline, by name.
    structs: &'a BTreeMap<String, StructConfig>,
    /// Whether a field is flattened: as serde writes it for the typesync
    /// outputs, as the database stores it for the schema.
    flattened: fn(&crate::types::Wire) -> bool,
}

impl Flattening<'_> {
    fn table(&self, table: &mut TableConfig) -> crate::Result<()> {
        self.struct_config(&mut table.struct_config)?;
        match table.output_override.as_deref_mut() {
            Some(replacement) => self.table(replacement),
            None => Ok(()),
        }
    }

    fn struct_config(&self, struct_config: &mut StructConfig) -> crate::Result<()> {
        let mut holding = vec![struct_config.struct_name.clone()];
        struct_config.fields = self.fields(&struct_config.fields, &mut holding)?;
        match struct_config.output_override.as_deref_mut() {
            Some(replacement) => self.struct_config(replacement),
            None => Ok(()),
        }
    }

    fn tagged_union(&self, tagged_union: &mut TaggedUnion) -> crate::Result<()> {
        for variant in &mut tagged_union.variants {
            self.variant(variant)?;
        }
        match tagged_union.output_override.as_deref_mut() {
            Some(replacement) => self.tagged_union(replacement),
            None => Ok(()),
        }
    }

    fn variant(&self, variant: &mut Variant) -> crate::Result<()> {
        if let Some(VariantData::InlineStruct(inline)) = &mut variant.data {
            self.struct_config(inline)?;
        }
        match variant.output_override.as_deref_mut() {
            Some(replacement) => self.variant(replacement),
            None => Ok(()),
        }
    }

    /// `fields` with each flattened struct, or `Option` of one, replaced by
    /// its own fields, each optional for an `Option`. A flattened map or enum
    /// stays, as its keys are known only from a value. `holding` names the
    /// structs being expanded, which a struct cannot flatten again.
    fn fields(
        &self,
        fields: &[StructField],
        holding: &mut Vec<String>,
    ) -> crate::Result<Vec<StructField>> {
        let mut expanded = Vec::with_capacity(fields.len());
        for field in fields {
            if !(self.flattened)(&field.effective().wire) {
                expanded.push(field.clone());
                continue;
            }
            let (held, optional) = match &field.effective().field_type {
                FieldType::Option(inner) => (inner.as_ref(), true),
                held => (held, false),
            };
            let Some((name, held_struct)) = self.struct_named(held) else {
                expanded.push(field.clone());
                continue;
            };
            if holding.contains(&name) {
                let mut path = holding.clone();
                path.push(name);
                return Err(crate::EvenframeError::Config(format!(
                    "`{}` flattens itself ({}), so serde would write it without end",
                    path[0],
                    path.join(" → ")
                )));
            }
            holding.push(name);
            let held_fields = self.fields(&held_struct.fields, holding)?;
            holding.pop();
            expanded.extend(held_fields.into_iter().map(|mut held_field| {
                if optional {
                    if !matches!(held_field.field_type, FieldType::Option(_)) {
                        held_field.field_type = FieldType::Option(Box::new(held_field.field_type));
                    }
                    held_field.wire.serde_optional = true;
                }
                held_field
            }));
        }
        Ok(expanded)
    }

    /// The struct `field_type` names, by its own name.
    fn struct_named(&self, field_type: &FieldType) -> Option<(String, &StructConfig)> {
        let FieldType::Other(name) = field_type else {
            return None;
        };
        self.structs
            .get(name)
            .or_else(|| self.structs.get(&name.to_case(Case::Pascal)))
            .map(|found| (found.struct_name.clone(), found.effective()))
    }
}

fn drop_serde_skipped_fields(struct_config: &mut StructConfig) {
    struct_config
        .fields
        .retain(|field| !field.effective().wire.serde_skipped);
    if let Some(replacement) = struct_config.output_override.as_deref_mut() {
        drop_serde_skipped_fields(replacement);
    }
}

fn drop_serde_skipped_table_fields(table: &mut TableConfig) {
    drop_serde_skipped_fields(&mut table.struct_config);
    if let Some(replacement) = table.output_override.as_deref_mut() {
        drop_serde_skipped_table_fields(replacement);
    }
}

fn drop_serde_skipped_variants(tagged_union: &mut TaggedUnion) {
    tagged_union
        .variants
        .retain(|variant| !variant.effective().wire.serde_skipped);
    for variant in &mut tagged_union.variants {
        if let Some(VariantData::InlineStruct(inline)) = &mut variant.data {
            drop_serde_skipped_fields(inline);
        }
    }
    if let Some(replacement) = tagged_union.output_override.as_deref_mut() {
        drop_serde_skipped_variants(replacement);
    }
}

/// Leaves out the fields the database never holds.
fn drop_unstored_fields(struct_config: &mut StructConfig) {
    struct_config
        .fields
        .retain(|field| !field.effective().wire.storage.skipped);
    if let Some(replacement) = struct_config.output_override.as_deref_mut() {
        drop_unstored_fields(replacement);
    }
}

fn drop_unstored_table_fields(table: &mut TableConfig) {
    drop_unstored_fields(&mut table.struct_config);
    if let Some(replacement) = table.output_override.as_deref_mut() {
        drop_unstored_table_fields(replacement);
    }
}

/// The pipeline whose `ValidatorOverrides` replace each value's validators in
/// its view, after flattening, so a flattened field brings its own.
#[derive(Clone, Copy)]
enum Overrides {
    Typesync,
    Schemasync,
}

impl Overrides {
    /// Puts the pipeline's list in `validators`. The schemasync view keeps the
    /// TypeScript list in `overrides.typesync` where it differs, so mock data
    /// can meet both.
    fn replace(self, validators: &mut Vec<Validator>, overrides: &mut ValidatorOverrides) {
        match self {
            Overrides::Typesync => {
                if let Some(replacement) = &overrides.typesync {
                    validators.clone_from(replacement);
                }
            }
            Overrides::Schemasync => {
                let typesync = overrides
                    .typesync
                    .take()
                    .unwrap_or_else(|| validators.clone());
                if let Some(replacement) = &overrides.schemasync {
                    validators.clone_from(replacement);
                }
                overrides.typesync = (typesync != *validators).then_some(typesync);
            }
        }
    }

    fn elements(self, validators: &mut [Vec<Validator>], overrides: &mut [ValidatorOverrides]) {
        for (element, element_overrides) in validators.iter_mut().zip(overrides) {
            self.replace(element, element_overrides);
        }
    }

    fn field(self, field: &mut StructField) {
        self.replace(&mut field.validators, &mut field.validator_overrides);
        if let Some(replacement) = field.output_override.as_deref_mut() {
            self.field(replacement);
        }
    }

    fn struct_config(self, struct_config: &mut StructConfig) {
        for field in &mut struct_config.fields {
            self.field(field);
        }
        if let Some(replacement) = struct_config.output_override.as_deref_mut() {
            self.struct_config(replacement);
        }
    }

    fn table(self, table: &mut TableConfig) {
        self.struct_config(&mut table.struct_config);
        if let Some(replacement) = table.output_override.as_deref_mut() {
            self.table(replacement);
        }
    }

    fn variant(self, variant: &mut Variant) {
        self.elements(
            &mut variant.element_validators,
            &mut variant.element_validator_overrides,
        );
        if let Some(VariantData::InlineStruct(inline)) = &mut variant.data {
            self.struct_config(inline);
        }
        if let Some(replacement) = variant.output_override.as_deref_mut() {
            self.variant(replacement);
        }
    }

    fn tagged_union(self, tagged_union: &mut TaggedUnion) {
        for variant in &mut tagged_union.variants {
            self.variant(variant);
        }
        if let Some(replacement) = tagged_union.output_override.as_deref_mut() {
            self.tagged_union(replacement);
        }
    }

    fn newtype(self, newtype: &mut NewtypeConfig) {
        self.replace(&mut newtype.validators, &mut newtype.validator_overrides);
        self.elements(
            &mut newtype.element_validators,
            &mut newtype.element_validator_overrides,
        );
    }
}

fn drop_unstored_variant_fields(tagged_union: &mut TaggedUnion) {
    for variant in &mut tagged_union.variants {
        drop_unstored_payload_fields(variant);
    }
    if let Some(replacement) = tagged_union.output_override.as_deref_mut() {
        drop_unstored_variant_fields(replacement);
    }
}

fn drop_unstored_payload_fields(variant: &mut Variant) {
    if let Some(VariantData::InlineStruct(inline)) = &mut variant.data {
        drop_unstored_fields(inline);
    }
    if let Some(replacement) = variant.output_override.as_deref_mut() {
        drop_unstored_payload_fields(replacement);
    }
}

#[cfg(test)]
mod tests {
    use super::AllConfigs;
    use crate::types::{FieldType, NewtypeConfig, StructConfig, StructField};
    use crate::validator::{StringValidator, Validator, ValidatorOverrides};
    use std::collections::BTreeMap;

    fn string(validator: StringValidator) -> Validator {
        Validator::StringValidator(validator)
    }

    fn field(name: &str, field_type: FieldType, overrides: ValidatorOverrides) -> StructField {
        StructField {
            field_name: name.to_owned(),
            field_type,
            validators: vec![string(StringValidator::NonEmpty)],
            validator_overrides: overrides,
            ..StructField::default()
        }
    }

    fn profile(fields: Vec<StructField>, newtypes: Vec<NewtypeConfig>) -> AllConfigs {
        AllConfigs {
            objects: BTreeMap::from([(
                "Profile".to_owned(),
                StructConfig {
                    struct_name: "Profile".to_owned(),
                    fields,
                    ..StructConfig::default()
                },
            )]),
            newtypes: newtypes
                .into_iter()
                .map(|newtype| (newtype.name.clone(), newtype))
                .collect(),
            ..AllConfigs::default()
        }
    }

    fn validators<'a>(objects: &'a BTreeMap<String, StructConfig>, name: &str) -> &'a StructField {
        objects["Profile"]
            .fields
            .iter()
            .find(|field| field.field_name == name)
            .expect("the field is in the view")
    }

    #[test]
    fn each_pipeline_replaces_the_shared_validators_with_its_own() {
        let typesync_list = vec![string(StringValidator::MinLength(3))];
        let schemasync_list = vec![string(StringValidator::MaxLength(20))];
        let configs = profile(
            vec![
                field(
                    "both",
                    FieldType::String,
                    ValidatorOverrides {
                        typesync: Some(typesync_list.clone()),
                        schemasync: Some(schemasync_list.clone()),
                    },
                ),
                field(
                    "typesync_only",
                    FieldType::String,
                    ValidatorOverrides {
                        typesync: Some(typesync_list.clone()),
                        schemasync: None,
                    },
                ),
                field("shared", FieldType::String, ValidatorOverrides::default()),
            ],
            Vec::new(),
        );

        let typesync = configs.for_typesync().expect("the typesync view builds");
        assert_eq!(
            validators(&typesync.objects, "both").validators,
            typesync_list
        );
        assert_eq!(
            validators(&typesync.objects, "typesync_only").validators,
            typesync_list
        );
        assert_eq!(
            validators(&typesync.objects, "shared").validators,
            vec![string(StringValidator::NonEmpty)]
        );

        let schemasync = configs
            .into_schemasync()
            .expect("the schemasync view builds");
        let both = validators(&schemasync.objects, "both");
        assert_eq!(both.validators, schemasync_list);
        assert_eq!(
            both.validator_overrides.typesync,
            Some(typesync_list.clone()),
            "mock data meets the TypeScript list too"
        );
        let typesync_only = validators(&schemasync.objects, "typesync_only");
        assert_eq!(
            typesync_only.validators,
            vec![string(StringValidator::NonEmpty)]
        );
        assert_eq!(
            typesync_only.validator_overrides.typesync,
            Some(typesync_list)
        );
        let shared = validators(&schemasync.objects, "shared");
        assert_eq!(
            shared.validator_overrides.typesync, None,
            "nothing to meet beyond the schema's own list"
        );
    }

    #[test]
    fn a_newtypes_overrides_reach_the_field_holding_it() {
        let configs = profile(
            vec![StructField {
                field_name: "slug".to_owned(),
                field_type: FieldType::Other("Slug".to_owned()),
                ..StructField::default()
            }],
            vec![NewtypeConfig {
                name: "Slug".to_owned(),
                inner: FieldType::String,
                validators: vec![string(StringValidator::NonEmpty)],
                validator_overrides: ValidatorOverrides {
                    typesync: Some(vec![string(StringValidator::Lowercased)]),
                    schemasync: Some(vec![string(StringValidator::MinLength(2))]),
                },
                ..NewtypeConfig::default()
            }],
        );
        let typesync = configs.for_typesync().expect("the typesync view builds");
        assert_eq!(
            typesync.newtypes["Slug"].validators,
            vec![string(StringValidator::Lowercased)]
        );

        let schemasync = configs
            .into_schemasync()
            .expect("the schemasync view builds");
        let slug = validators(&schemasync.objects, "slug");
        assert_eq!(slug.field_type, FieldType::String);
        assert_eq!(slug.validators, vec![string(StringValidator::MinLength(2))]);
        assert_eq!(
            slug.validator_overrides.typesync,
            Some(vec![string(StringValidator::Lowercased)])
        );
    }
}

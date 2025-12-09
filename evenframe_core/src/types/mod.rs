mod field_type;

pub use crate::types::field_type::FieldType;
use crate::{
    EvenframeError, Result, evenframe_log,
    format::Format,
    schemasync::{DefineConfig, EdgeConfig, TableConfig},
    traits::EvenframePersistableStruct,
    validator::Validator,
    wrappers::EvenframeRecordId,
};
use convert_case::{Case, Casing};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaggedUnion {
    pub enum_name: String,
    pub variants: Vec<Variant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum RecordLink<T: EvenframePersistableStruct> {
    Id(EvenframeRecordId),
    Object(T),
}

impl<'de, T: EvenframePersistableStruct> Deserialize<'de> for RecordLink<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;

        if value.is_string() {
            // If it's a string, it can only be an Id
            EvenframeRecordId::deserialize(value)
                .map(RecordLink::Id)
                .map_err(|e| {
                    serde::de::Error::custom(format!(
                        "Failed to deserialize RecordLink from string as Id: {}",
                        e
                    ))
                })
        } else if value.is_object() {
            // If it's an object, it could be an Id or an Object
            let id_attempt = EvenframeRecordId::deserialize(value.clone());
            let obj_attempt = T::deserialize(value.clone());

            match (id_attempt, obj_attempt) {
                (Ok(id), Err(_)) => Ok(RecordLink::Id(id)),
                (Err(_), Ok(obj)) => Ok(RecordLink::Object(obj)),
                (Ok(_), Ok(_)) => Err(serde::de::Error::custom(
                    "Ambiguous object: it can be deserialized as both RecordLink::Id and RecordLink::Object",
                )),
                (Err(err_id), Err(err_obj)) => Err(serde::de::Error::custom(format!(
                    "Failed to deserialize object as RecordLink: {:#?}. Tried Id variant: {}. Tried Object variant: {}.",
                    value, err_id, err_obj
                ))),
            }
        } else {
            Err(serde::de::Error::custom(
                "RecordLink must be a string or an object",
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Variant {
    pub name: String,
    pub data: Option<VariantData>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VariantData {
    InlineStruct(StructConfig),
    DataStructureRef(FieldType),
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StructField {
    pub field_name: String,
    pub field_type: FieldType,
    pub edge_config: Option<EdgeConfig>,
    pub define_config: Option<DefineConfig>,
    pub format: Option<Format>,
    pub validators: Vec<Validator>,
    pub always_regenerate: bool,
}

impl StructField {
    pub fn unit(field_name: String) -> Self {
        Self {
            field_name,
            field_type: FieldType::Unit,
            edge_config: None,
            define_config: None,
            format: None,
            validators: Vec::new(),
            always_regenerate: false,
        }
    }

    pub fn partial(field_name: &str) -> Self {
        Self {
            field_name: field_name.to_string(),
            field_type: FieldType::Struct(Vec::new()),
            edge_config: None,
            define_config: None,
            format: None,
            validators: Vec::new(),
            always_regenerate: false,
        }
    }
    pub fn generate_define_statement(
        &self,
        enums: HashMap<String, TaggedUnion>,
        app_structs: HashMap<String, StructConfig>,
        persistable_structs: HashMap<String, TableConfig>,
        table_name: &String,
    ) -> Result<String> {
        evenframe_log!(
            format!(
                "Generating define statements for:\nEnums: {:#?}\nApp structs: {:#?}\nTables: {:#?}",
                enums.keys(),
                app_structs.keys(),
                persistable_structs.keys()
            ),
            "define_generation.log"
        );

        /* --- Start of Iterative Type Conversion Logic --- */

        #[derive(Debug)]
        enum WorkItem<'a> {
            Process(&'a FieldType),
            PushString(String),
            AssembleOption,
            AssembleVec,
            AssembleMap,
            AssembleTuple { count: usize },
            AssembleStruct { count: usize, names: Vec<String> },
            AssembleEnum { count: usize },
            EnterStructScope { name: String },
            LeaveStructScope { name: String },
        }

        let convert_type_iteratively =
            |start_field_type: &FieldType| -> Result<(String, bool, Option<String>)> {
                let mut work_stack: Vec<WorkItem> = vec![WorkItem::Process(start_field_type)];
                let mut value_stack: Vec<(String, bool, Option<String>)> = Vec::new();
                let mut visited_types = HashSet::new();

                while let Some(item) = work_stack.pop() {
                    match item {
                        WorkItem::Process(field_type) => match field_type {
                            FieldType::String | FieldType::Char => {
                                value_stack.push(("string".to_string(), false, None))
                            }
                            FieldType::Bool => value_stack.push(("bool".to_string(), false, None)),
                            FieldType::DateTime => {
                                value_stack.push(("datetime".to_string(), false, None))
                            }
                            FieldType::EvenframeDuration => {
                                value_stack.push(("duration".to_string(), false, None))
                            }
                            FieldType::Timezone => {
                                value_stack.push(("string".to_string(), false, None))
                            }
                            FieldType::Decimal => {
                                value_stack.push(("decimal".to_string(), false, None))
                            }
                            FieldType::F32 | FieldType::F64 | FieldType::OrderedFloat(_) => {
                                value_stack.push(("float".to_string(), false, None))
                            }
                            FieldType::I8
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
                            | FieldType::Usize => {
                                value_stack.push(("int".to_string(), false, None))
                            }
                            FieldType::Unit => value_stack.push(("any".to_string(), false, None)),
                            FieldType::EvenframeRecordId => {
                                let type_str = if self.field_name == "id" {
                                    format!("record<{}>", table_name)
                                } else {
                                    "record<any>".to_string()
                                };
                                value_stack.push((type_str, false, None));
                            }
                            FieldType::Option(inner) => {
                                work_stack.push(WorkItem::AssembleOption);
                                work_stack.push(WorkItem::Process(inner));
                            }
                            FieldType::Vec(inner) => {
                                work_stack.push(WorkItem::AssembleVec);
                                work_stack.push(WorkItem::Process(inner));
                            }
                            FieldType::HashMap(_, value) | FieldType::BTreeMap(_, value) => {
                                work_stack.push(WorkItem::AssembleMap);
                                work_stack.push(WorkItem::Process(value));
                            }
                            FieldType::RecordLink(inner) => {
                                if let FieldType::Other(type_name) = inner.as_ref() {
                                    let type_str =
                                        format!("record<{}>", type_name.to_case(Case::Snake));
                                    value_stack.push((type_str, false, None));
                                } else {
                                    work_stack.push(WorkItem::Process(inner));
                                }
                            }
                            FieldType::Tuple(types) => {
                                work_stack.push(WorkItem::AssembleTuple { count: types.len() });
                                for t in types.iter().rev() {
                                    work_stack.push(WorkItem::Process(t));
                                }
                            }
                            FieldType::Struct(fields) => {
                                let names = fields.iter().map(|(name, _)| name.clone()).collect();
                                work_stack.push(WorkItem::AssembleStruct {
                                    count: fields.len(),
                                    names,
                                });
                                for (_, ftype) in fields.iter().rev() {
                                    work_stack.push(WorkItem::Process(ftype));
                                }
                            }
                            FieldType::Other(name) => {
                                if let Some(enum_def) = enums.get(name) {
                                    let total_variants = enum_def.variants.len();
                                    work_stack.push(WorkItem::AssembleEnum {
                                        count: total_variants,
                                    });

                                    for variant in enum_def.variants.iter().rev() {
                                        if let Some(data) = &variant.data {
                                            match data {
                                                VariantData::InlineStruct(s) => {
                                                    let struct_config = app_structs.get(&s.struct_name)
                                                        .ok_or_else(|| EvenframeError::FieldDefinition {
                                                            message: format!("Inline enum struct '{}' should have corresponding object definition", s.struct_name),
                                                            work_stack: format!("{:#?}", work_stack),
                                                            value_stack: format!("{:#?}", value_stack),
                                                            item: format!("{:#?}", item),
                                                            visited_types: format!("{:#?}", visited_types),
                                                        })?;
                                                    let names = struct_config
                                                        .fields
                                                        .iter()
                                                        .map(|f| f.field_name.clone())
                                                        .collect();
                                                    work_stack.push(WorkItem::AssembleStruct {
                                                        count: struct_config.fields.len(),
                                                        names,
                                                    });
                                                    for field in struct_config.fields.iter().rev() {
                                                        work_stack.push(WorkItem::Process(
                                                            &field.field_type,
                                                        ));
                                                    }
                                                }
                                                VariantData::DataStructureRef(ft) => {
                                                    work_stack.push(WorkItem::Process(ft));
                                                }
                                            }
                                        } else {
                                            work_stack.push(WorkItem::PushString(format!(
                                                "\"{}\"",
                                                variant.name
                                            )));
                                        }
                                    }
                                } else if let Some(app_struct) = app_structs.get(name) {
                                    if visited_types.contains(name) {
                                        value_stack.push(("object".to_string(), false, None));
                                        continue;
                                    }
                                    work_stack
                                        .push(WorkItem::LeaveStructScope { name: name.clone() });
                                    let names = app_struct
                                        .fields
                                        .iter()
                                        .map(|f| f.field_name.clone())
                                        .collect();
                                    work_stack.push(WorkItem::AssembleStruct {
                                        count: app_struct.fields.len(),
                                        names,
                                    });
                                    for field in app_struct.fields.iter().rev() {
                                        work_stack.push(WorkItem::Process(&field.field_type));
                                    }
                                    work_stack
                                        .push(WorkItem::EnterStructScope { name: name.clone() });
                                } else if persistable_structs
                                    .contains_key(&name.to_case(Case::Snake))
                                {
                                    value_stack.push((
                                        format!("record<{}>", name.to_case(Case::Snake)),
                                        false,
                                        None,
                                    ));
                                } else {
                                    value_stack.push((name.clone(), false, None));
                                }
                            }
                        },
                        WorkItem::PushString(s) => {
                            value_stack.push((s, false, None));
                        }
                        WorkItem::AssembleOption => {
                            let (inner_type, needs_wildcard, wildcard_type) = value_stack
                                .pop()
                                .ok_or_else(|| EvenframeError::FieldDefinition {
                                    message: "Stack underflow in AssembleOption".to_string(),
                                    work_stack: format!("{:#?}", work_stack),
                                    value_stack: format!("{:#?}", value_stack),
                                    item: "AssembleOption".to_string(),
                                    visited_types: format!("{:#?}", visited_types),
                                })?;
                            value_stack.push((
                                format!("null | {}", inner_type),
                                needs_wildcard,
                                wildcard_type,
                            ));
                        }
                        WorkItem::AssembleVec => {
                            let (inner_type, _, _) = value_stack.pop().ok_or_else(|| {
                                EvenframeError::FieldDefinition {
                                    message: "Stack underflow in AssembleVec".to_string(),
                                    work_stack: format!("{:#?}", work_stack),
                                    value_stack: format!("{:#?}", value_stack),
                                    item: "AssembleVec".to_string(),
                                    visited_types: format!("{:#?}", visited_types),
                                }
                            })?;
                            value_stack.push((format!("array<{}>", inner_type), false, None));
                        }
                        WorkItem::AssembleMap => {
                            let (value_type, _, _) = value_stack.pop().ok_or_else(|| {
                                EvenframeError::FieldDefinition {
                                    message: "Stack underflow in AssembleMap".to_string(),
                                    work_stack: format!("{:#?}", work_stack),
                                    value_stack: format!("{:#?}", value_stack),
                                    item: "AssembleMap".to_string(),
                                    visited_types: format!("{:#?}", visited_types),
                                }
                            })?;
                            value_stack.push(("object".to_string(), true, Some(value_type)));
                        }
                        WorkItem::AssembleTuple { count } => {
                            let mut items = Vec::with_capacity(count);
                            for _ in 0..count {
                                items.push(
                                    value_stack
                                        .pop()
                                        .ok_or_else(|| EvenframeError::FieldDefinition {
                                            message: "Stack underflow in AssembleTuple".to_string(),
                                            work_stack: format!("{:#?}", work_stack),
                                            value_stack: format!("{:#?}", value_stack),
                                            item: "AssembleTuple".to_string(),
                                            visited_types: format!("{:#?}", visited_types),
                                        })?
                                        .0,
                                );
                            }
                            items.reverse();
                            value_stack.push((format!("[{}]", items.join(", ")), false, None));
                        }
                        WorkItem::AssembleStruct { count, names } => {
                            let mut items = Vec::with_capacity(count);
                            for i in 0..count {
                                let (field_type, _, _) = value_stack.pop().ok_or_else(|| {
                                    EvenframeError::FieldDefinition {
                                        message: "Stack underflow in AssembleStruct".to_string(),
                                        work_stack: format!("{:#?}", work_stack),
                                        value_stack: format!("{:#?}", value_stack),
                                        item: "AssembleStruct".to_string(),
                                        visited_types: format!("{:#?}", visited_types),
                                    }
                                })?;
                                items.push(format!("{}: {}", names[count - 1 - i], field_type));
                            }
                            items.reverse();
                            value_stack.push((format!("{{ {} }}", items.join(", ")), false, None));
                        }
                        WorkItem::AssembleEnum { count } => {
                            let mut variants = Vec::with_capacity(count);
                            for _ in 0..count {
                                variants.push(
                                    value_stack
                                        .pop()
                                        .ok_or_else(|| EvenframeError::FieldDefinition {
                                            message: "Stack underflow in AssembleEnum".to_string(),
                                            work_stack: format!("{:#?}", work_stack),
                                            value_stack: format!("{:#?}", value_stack),
                                            item: "AssembleEnum".to_string(),
                                            visited_types: format!("{:#?}", visited_types),
                                        })?
                                        .0,
                                );
                            }
                            variants.reverse();
                            value_stack.push((variants.join(" | "), false, None));
                        }
                        WorkItem::EnterStructScope { name } => {
                            visited_types.insert(name);
                        }
                        WorkItem::LeaveStructScope { name } => {
                            visited_types.remove(&name);
                        }
                    }
                }
                value_stack
                    .pop()
                    .ok_or_else(|| EvenframeError::FieldDefinition {
                        message: "Final stack underflow".to_string(),
                        work_stack: format!("{:#?}", work_stack),
                        value_stack: format!("{:#?}", value_stack),
                        item: "(item out of scope)".to_string(),
                        visited_types: format!("{:#?}", visited_types),
                    })
            };

        let mut stmt = format!(
            "DEFINE FIELD OVERWRITE {} ON TABLE {}",
            self.field_name, table_name
        );

        let (type_str, needs_wildcard, wildcard_type) = if let Some(ref def) = self.define_config {
            if def.should_skip {
                ("".to_string(), false, None)
            } else if let Some(ref data_type) = def.data_type {
                (data_type.clone(), false, None)
            } else {
                convert_type_iteratively(&self.field_type)?
            }
        } else {
            convert_type_iteratively(&self.field_type)?
        };

        if let Some(ref def) = self.define_config
            && def.flexible.unwrap_or(false)
        {
            stmt.push_str(" FLEXIBLE");
        }

        if !type_str.is_empty() {
            stmt.push_str(&format!(" TYPE {}", type_str));
        }

        if let Some(ref def) = self.define_config {
            if let Some(ref def_val) = def.default {
                let always = if def.default_always.is_some() {
                    " ALWAYS"
                } else {
                    ""
                };
                stmt.push_str(&format!(" DEFAULT{} {}", always, def_val));
            } else {
                use crate::default::field_type_to_surql_default;
                stmt.push_str(&format!(
                    " DEFAULT {}",
                    field_type_to_surql_default(
                        &self.field_name,
                        table_name,
                        &self.field_type,
                        &enums,
                        &app_structs,
                        &persistable_structs,
                    )
                ));
            }

            if def.readonly.unwrap_or(false) {
                stmt.push_str(" READONLY");
            }

            if let Some(ref val) = def.value {
                stmt.push_str(&format!(" VALUE {}", val));
            }

            if let Some(ref assert_val) = def.assert {
                stmt.push_str(&format!(" ASSERT {}", assert_val));
            }
        }

        if let Some(ref def) = self.define_config {
            let mut permissions = Vec::new();

            if let Some(ref perm) = def.select_permissions {
                permissions.push(format!("FOR select {}", perm));
            }
            if let Some(ref perm) = def.create_permissions {
                permissions.push(format!("FOR create {}", perm));
            }
            if let Some(ref perm) = def.update_permissions {
                permissions.push(format!("FOR update {}", perm));
            }

            if !permissions.is_empty() {
                stmt.push_str(&format!(" PERMISSIONS {}", permissions.join(" ")));
            }
        }

        stmt.push_str(";\n");

        if let Some(wildcard_value_type) = wildcard_type
            && needs_wildcard
        {
            stmt.push_str(&format!(
                "DEFINE FIELD {}.* ON TABLE {} TYPE {};\n",
                self.field_name, table_name, wildcard_value_type
            ));
        }

        Ok(stmt)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StructConfig {
    pub struct_name: String,
    pub fields: Vec<StructField>,
    pub validators: Vec<Validator>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== TaggedUnion Tests ====================

    #[test]
    fn test_tagged_union_equality() {
        let tu1 = TaggedUnion {
            enum_name: "Status".to_string(),
            variants: vec![],
        };
        let tu2 = TaggedUnion {
            enum_name: "Status".to_string(),
            variants: vec![],
        };
        assert_eq!(tu1, tu2);
    }

    #[test]
    fn test_tagged_union_with_variants() {
        let tu = TaggedUnion {
            enum_name: "Status".to_string(),
            variants: vec![
                Variant {
                    name: "Active".to_string(),
                    data: None,
                },
                Variant {
                    name: "Inactive".to_string(),
                    data: None,
                },
            ],
        };
        assert_eq!(tu.variants.len(), 2);
        assert_eq!(tu.variants[0].name, "Active");
    }

    #[test]
    fn test_tagged_union_serialize_deserialize() {
        let tu = TaggedUnion {
            enum_name: "Color".to_string(),
            variants: vec![Variant {
                name: "Red".to_string(),
                data: None,
            }],
        };
        let json = serde_json::to_string(&tu).unwrap();
        let deserialized: TaggedUnion = serde_json::from_str(&json).unwrap();
        assert_eq!(tu, deserialized);
    }

    #[test]
    fn test_tagged_union_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        let tu1 = TaggedUnion {
            enum_name: "A".to_string(),
            variants: vec![],
        };
        let tu2 = TaggedUnion {
            enum_name: "B".to_string(),
            variants: vec![],
        };
        set.insert(tu1);
        set.insert(tu2);
        assert_eq!(set.len(), 2);
    }

    // ==================== Variant Tests ====================

    #[test]
    fn test_variant_unit() {
        let v = Variant {
            name: "None".to_string(),
            data: None,
        };
        assert!(v.data.is_none());
    }

    #[test]
    fn test_variant_with_data_structure_ref() {
        let v = Variant {
            name: "Some".to_string(),
            data: Some(VariantData::DataStructureRef(FieldType::String)),
        };
        assert!(matches!(v.data, Some(VariantData::DataStructureRef(FieldType::String))));
    }

    #[test]
    fn test_variant_with_inline_struct() {
        let struct_config = StructConfig {
            struct_name: "InnerData".to_string(),
            fields: vec![],
            validators: vec![],
        };
        let v = Variant {
            name: "Complex".to_string(),
            data: Some(VariantData::InlineStruct(struct_config)),
        };
        assert!(matches!(v.data, Some(VariantData::InlineStruct(_))));
    }

    // ==================== VariantData Tests ====================

    #[test]
    fn test_variant_data_equality() {
        let vd1 = VariantData::DataStructureRef(FieldType::I32);
        let vd2 = VariantData::DataStructureRef(FieldType::I32);
        assert_eq!(vd1, vd2);
    }

    #[test]
    fn test_variant_data_inline_struct_vs_ref() {
        let vd1 = VariantData::DataStructureRef(FieldType::String);
        let vd2 = VariantData::InlineStruct(StructConfig {
            struct_name: "Test".to_string(),
            fields: vec![],
            validators: vec![],
        });
        assert_ne!(vd1, vd2);
    }

    // ==================== StructField Tests ====================

    #[test]
    fn test_struct_field_unit() {
        let field = StructField::unit("name".to_string());
        assert_eq!(field.field_name, "name");
        assert!(matches!(field.field_type, FieldType::Unit));
    }

    #[test]
    fn test_struct_field_partial() {
        let field = StructField::partial("data");
        assert_eq!(field.field_name, "data");
        assert!(matches!(field.field_type, FieldType::Struct(_)));
    }

    #[test]
    fn test_struct_field_default() {
        let field = StructField::default();
        assert!(field.field_name.is_empty());
        assert!(field.validators.is_empty());
    }

    #[test]
    fn test_struct_field_equality() {
        let f1 = StructField {
            field_name: "id".to_string(),
            field_type: FieldType::String,
            edge_config: None,
            define_config: None,
            format: None,
            validators: vec![],
            always_regenerate: false,
        };
        let f2 = f1.clone();
        assert_eq!(f1, f2);
    }

    // ==================== StructConfig Tests ====================

    #[test]
    fn test_struct_config_empty() {
        let sc = StructConfig {
            struct_name: "Empty".to_string(),
            fields: vec![],
            validators: vec![],
        };
        assert!(sc.fields.is_empty());
    }

    #[test]
    fn test_struct_config_with_fields() {
        let sc = StructConfig {
            struct_name: "User".to_string(),
            fields: vec![
                StructField {
                    field_name: "id".to_string(),
                    field_type: FieldType::String,
                    edge_config: None,
                    define_config: None,
                    format: None,
                    validators: vec![],
                    always_regenerate: false,
                },
                StructField {
                    field_name: "age".to_string(),
                    field_type: FieldType::I32,
                    edge_config: None,
                    define_config: None,
                    format: None,
                    validators: vec![],
                    always_regenerate: false,
                },
            ],
            validators: vec![],
        };
        assert_eq!(sc.fields.len(), 2);
    }

    #[test]
    fn test_struct_config_serialize_deserialize() {
        let sc = StructConfig {
            struct_name: "Test".to_string(),
            fields: vec![],
            validators: vec![],
        };
        let json = serde_json::to_string(&sc).unwrap();
        let deserialized: StructConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(sc, deserialized);
    }

    // ==================== FieldType Basic Tests ====================

    #[test]
    fn test_field_type_primitives() {
        assert!(matches!(FieldType::String, FieldType::String));
        assert!(matches!(FieldType::I32, FieldType::I32));
        assert!(matches!(FieldType::Bool, FieldType::Bool));
        assert!(matches!(FieldType::F64, FieldType::F64));
    }

    #[test]
    fn test_field_type_option() {
        let ft = FieldType::Option(Box::new(FieldType::String));
        assert!(matches!(ft, FieldType::Option(_)));
    }

    #[test]
    fn test_field_type_vec() {
        let ft = FieldType::Vec(Box::new(FieldType::I32));
        assert!(matches!(ft, FieldType::Vec(_)));
    }

    #[test]
    fn test_field_type_tuple() {
        let ft = FieldType::Tuple(vec![FieldType::String, FieldType::I32]);
        assert!(matches!(ft, FieldType::Tuple(_)));
    }

    #[test]
    fn test_field_type_other() {
        let ft = FieldType::Other("CustomType".to_string());
        assert!(matches!(ft, FieldType::Other(ref s) if s == "CustomType"));
    }

    #[test]
    fn test_field_type_hashmap() {
        let ft = FieldType::HashMap(
            Box::new(FieldType::String),
            Box::new(FieldType::I32),
        );
        assert!(matches!(ft, FieldType::HashMap(_, _)));
    }

    #[test]
    fn test_field_type_btreemap() {
        let ft = FieldType::BTreeMap(
            Box::new(FieldType::String),
            Box::new(FieldType::Bool),
        );
        assert!(matches!(ft, FieldType::BTreeMap(_, _)));
    }

    #[test]
    fn test_field_type_record_link() {
        let ft = FieldType::RecordLink(Box::new(FieldType::Other("User".to_string())));
        assert!(matches!(ft, FieldType::RecordLink(_)));
    }

    #[test]
    fn test_field_type_struct_inline() {
        let ft = FieldType::Struct(vec![
            ("name".to_string(), FieldType::String),
            ("value".to_string(), FieldType::I32),
        ]);
        assert!(matches!(ft, FieldType::Struct(_)));
    }

    // ==================== FieldType Equality Tests ====================

    #[test]
    fn test_field_type_equality_primitives() {
        assert_eq!(FieldType::String, FieldType::String);
        assert_ne!(FieldType::String, FieldType::I32);
    }

    #[test]
    fn test_field_type_equality_nested() {
        let ft1 = FieldType::Option(Box::new(FieldType::String));
        let ft2 = FieldType::Option(Box::new(FieldType::String));
        let ft3 = FieldType::Option(Box::new(FieldType::I32));
        assert_eq!(ft1, ft2);
        assert_ne!(ft1, ft3);
    }

    // ==================== FieldType Hash Tests ====================

    #[test]
    fn test_field_type_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(FieldType::String);
        set.insert(FieldType::I32);
        set.insert(FieldType::String); // duplicate
        assert_eq!(set.len(), 2);
    }

    // ==================== FieldType Clone Tests ====================

    #[test]
    fn test_field_type_clone() {
        let ft = FieldType::Option(Box::new(FieldType::Vec(Box::new(FieldType::I32))));
        let cloned = ft.clone();
        assert_eq!(ft, cloned);
    }

    // ==================== FieldType Debug Tests ====================

    #[test]
    fn test_field_type_debug() {
        let ft = FieldType::String;
        let debug = format!("{:?}", ft);
        assert!(debug.contains("String"));
    }

    // ==================== Edge Cases ====================

    #[test]
    fn test_empty_struct_config() {
        let sc = StructConfig {
            struct_name: "".to_string(),
            fields: vec![],
            validators: vec![],
        };
        assert!(sc.struct_name.is_empty());
    }

    #[test]
    fn test_deeply_nested_field_type() {
        let ft = FieldType::Option(Box::new(
            FieldType::Vec(Box::new(
                FieldType::HashMap(
                    Box::new(FieldType::String),
                    Box::new(FieldType::Option(Box::new(FieldType::I32))),
                ),
            )),
        ));
        assert!(matches!(ft, FieldType::Option(_)));
    }

    #[test]
    fn test_struct_field_with_validators() {
        use crate::validator::{StringValidator, Validator};
        let field = StructField {
            field_name: "email".to_string(),
            field_type: FieldType::String,
            edge_config: None,
            define_config: None,
            format: None,
            validators: vec![Validator::StringValidator(StringValidator::Email)],
            always_regenerate: false,
        };
        assert_eq!(field.validators.len(), 1);
    }
}

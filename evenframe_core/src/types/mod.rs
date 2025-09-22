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

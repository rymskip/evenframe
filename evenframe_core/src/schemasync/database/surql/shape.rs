//! The stored shape of a field: the SurrealQL `TYPE` of its value and the
//! `ASSERT` that checks the validators declared at every position within it.
//! One walk produces both, so the assertion reaches exactly the positions the
//! type stores: a field's own value, an embedded object's members, an
//! array's elements, a map's keys and values, a tuple's items, an option's
//! value and a tagged enum's payload under its tag.

use crate::schemasync::TableConfig;
use crate::schemasync::config::SurqlOptions;
use crate::schemasync::database::surql::assert::generate_assert_from_validators;
use crate::schemasync::table::{surql_ident, surql_string_literal};
use crate::types::{
    ContentStorage, DeclaredTypes, EnumRepresentation, FieldOwner, FieldType, ForeignTypeRegistry,
    Storage, StructConfig, StructField, TaggedUnion, Variant, VariantData,
    record_link_target_surql,
};
use crate::validator::{MockValue, Validator};
use crate::{EvenframeError, Result};
use convert_case::{Case, Casing};
use std::collections::{BTreeMap, HashSet};

/// The types a field's shape is read against.
#[derive(Clone, Copy)]
pub struct DefineContext<'a> {
    pub tables: &'a BTreeMap<String, TableConfig>,
    pub objects: &'a BTreeMap<String, StructConfig>,
    pub enums: &'a BTreeMap<String, TaggedUnion>,
    /// The declared types where desugaring replaced a newtype, whose
    /// validators hold at the positions it sat at.
    pub declared: &'a DeclaredTypes,
    pub registry: &'a ForeignTypeRegistry,
    pub options: SurqlOptions,
}

/// Validators at a position the schema cannot assert, so the database
/// accepts a value failing them, which reading the record back rejects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unasserted {
    /// The field, as `table.field`, and the path below it.
    pub location: String,
    /// The validators, in words.
    pub validators: String,
    pub reason: UnassertedReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnassertedReason {
    /// Inside an untagged enum, whose variants SurrealQL cannot tell apart.
    Untagged,
    /// Inside a struct that holds itself, which an assertion cannot follow
    /// without end.
    Recursive,
    /// Inside a struct with keys known only from a value, stored as any
    /// object.
    Open,
}

impl UnassertedReason {
    pub fn describe(self) -> &'static str {
        match self {
            UnassertedReason::Untagged => {
                "inside an untagged enum, whose variants SurrealQL cannot tell apart"
            }
            UnassertedReason::Recursive => "inside a struct that holds itself",
            UnassertedReason::Open => {
                "inside a struct holding keys known only from a value, stored as any object"
            }
        }
    }
}

/// A field's stored shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shape {
    pub surql: String,
    /// The value type of a map, which the field's `.*` definition takes.
    pub map_values: Option<String>,
    pub assertion: Option<String>,
    /// Whether the zero value the schema falls back to as the field's
    /// `DEFAULT` (`''`, `0`, `[]`, a struct of zeros, an enum's default
    /// variant) meets every validator the assertion checks.
    pub zero_ok: bool,
}

/// The shape of `field` on `table_name`, recording each position whose
/// validators the schema cannot assert in `unasserted`.
pub fn field_shape(
    field: &StructField,
    owner: &FieldOwner,
    table_name: &str,
    context: &DefineContext<'_>,
    unasserted: &mut Vec<Unasserted>,
) -> Result<Shape> {
    let walk = Walk {
        context,
        table_name,
        id_field: field.db_name() == "id",
    };
    walk.run(
        WorkItem::ProcessField {
            field,
            owner: Some(owner.clone()),
            place: Place::root(format!("{owner}.{}", field.field_name)),
        },
        unasserted,
    )
}

/// The `TYPE` of a value of `field_type` in the field `field_name`.
pub fn type_surql(
    field_type: &FieldType,
    field_name: &str,
    table_name: &str,
    context: &DefineContext<'_>,
) -> Result<String> {
    let walk = Walk {
        context,
        table_name,
        id_field: field_name == "id",
    };
    walk.run(
        WorkItem::Process {
            field_type,
            place: Place::root(field_name.to_owned()),
        },
        &mut Vec::new(),
    )
    .map(|shape| shape.surql)
}

/// The assertion of `field`'s own validators alone, for a field whose stored
/// type the schema does not take from its Rust type.
pub fn own_assertion(field: &StructField, options: SurqlOptions) -> Option<String> {
    checked(
        &field.validators,
        "$value",
        matches!(field.field_type, FieldType::Option(_)),
        options,
    )
}

/// Whether `field`'s zero value meets its own validators, for a field whose
/// stored type the schema does not take from its Rust type.
pub fn own_zero_ok(field: &StructField) -> bool {
    zero_meets(&field.validators, zero_of(&field.field_type))
}

/// The zero value the schema's fallback `DEFAULT` gives a value of
/// `field_type`, for the types whose zero a validator can reject. An
/// `Option`'s is absence, which a guarded assertion accepts.
fn zero_of(field_type: &FieldType) -> Option<MockValue<'static>> {
    match field_type {
        FieldType::String | FieldType::Char => Some(MockValue::Str("")),
        FieldType::F32
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
        | FieldType::Usize => Some(MockValue::Num(0.0)),
        FieldType::Duration => Some(MockValue::DurationNanos(0)),
        FieldType::Vec(_) => Some(MockValue::ArrayLen(0)),
        FieldType::FromText(kind) => zero_of(kind.value_type()),
        FieldType::JsonText(inner) => zero_of(inner),
        FieldType::Option(_)
        | FieldType::Unit
        | FieldType::Bool
        | FieldType::HashMap(_, _)
        | FieldType::BTreeMap(_, _)
        | FieldType::Tuple(_)
        | FieldType::Struct(_)
        | FieldType::RecordLink(_)
        | FieldType::IsoDate
        | FieldType::EpochMillis
        | FieldType::Other(_) => None,
    }
}

fn zero_meets(validators: &[Validator], zero: Option<MockValue<'_>>) -> bool {
    zero.is_none_or(|zero| validators.iter().all(|validator| validator.matches(&zero)))
}

/// `validators` asserted on `expr`, guarded for an absent value when
/// `optional`.
fn checked(
    validators: &[Validator],
    expr: &str,
    optional: bool,
    options: SurqlOptions,
) -> Option<String> {
    let generated = generate_assert_from_validators(validators, expr, options.allow_scripting);
    if generated.is_empty() {
        None
    } else if optional {
        // Inner assertions such as `string::len($value)` reject absence.
        Some(format!(
            "{expr} = {} OR ({generated})",
            options.option_none.literal()
        ))
    } else {
        Some(generated)
    }
}

/// Every one of `parts` that is present, joined by AND.
fn all_of(parts: impl IntoIterator<Item = Option<String>>) -> Option<String> {
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    match parts.as_slice() {
        [] => None,
        [only] => Some(only.clone()),
        _ => Some(
            parts
                .iter()
                .map(|part| format!("({part})"))
                .collect::<Vec<_>>()
                .join(" AND "),
        ),
    }
}

/// Whether a field's own validators already hold the newtype at a position:
/// desugaring moves the validators of a newtype a field holds directly, or
/// in an `Option`, onto the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Own {
    Field,
    OptionOfField,
    Nested,
}

/// A position in a field's value: the expression reaching it, where it is
/// in words, and the closures already open around it.
#[derive(Debug, Clone)]
struct Place {
    expr: String,
    location: String,
    depth: usize,
    own: Own,
    /// Why validators here cannot be asserted, when they cannot.
    unasserted: Option<UnassertedReason>,
}

impl Place {
    fn root(location: String) -> Self {
        Place {
            expr: "$value".to_owned(),
            location,
            depth: 0,
            own: Own::Field,
            unasserted: None,
        }
    }

    /// The value `expr` reaches below this one, `suffix` further on.
    fn below(&self, expr: String, suffix: &str) -> Self {
        Place {
            expr,
            location: format!("{}{suffix}", self.location),
            depth: self.depth,
            own: Own::Nested,
            unasserted: self.unasserted,
        }
    }

    /// The key `name` of the object here.
    fn key(&self, name: &str) -> Self {
        self.below(
            format!("{}.{}", self.expr, surql_ident(name)),
            &format!(".{name}"),
        )
    }

    /// The value under the key `name` of the object here, which a variant
    /// keeps its payload in, at the variant's own location.
    fn content(&self, name: &str) -> Self {
        self.below(format!("{}.{}", self.expr, surql_ident(name)), "")
    }

    /// The `position`th item of the array here.
    fn item(&self, position: usize) -> Self {
        self.below(
            format!("{}[{position}]", self.expr),
            &format!("[{position}]"),
        )
    }

    /// The binding of a closure over the values collected here, and the
    /// position it names.
    fn each(&self, suffix: &str) -> (String, Self) {
        let binding = format!("$item{}", self.depth);
        let place = Place {
            expr: binding.clone(),
            location: format!("{}{suffix}", self.location),
            depth: self.depth + 1,
            own: Own::Nested,
            unasserted: self.unasserted,
        };
        (binding, place)
    }

    /// The value an `Option` here holds.
    fn present(&self) -> Self {
        Place {
            own: match self.own {
                Own::Field => Own::OptionOfField,
                Own::OptionOfField | Own::Nested => Own::Nested,
            },
            ..self.clone()
        }
    }

    /// The value a newtype here holds.
    fn within_newtype(&self) -> Self {
        Place {
            own: Own::Nested,
            ..self.clone()
        }
    }

    /// A field's own value here, whose validators the field carries.
    fn field(&self) -> Self {
        Place {
            own: Own::Field,
            ..self.clone()
        }
    }

    /// This position inside a variant, where `reason` keeps validators from
    /// being asserted, if any does.
    fn variant(&self, name: &str, reason: Option<UnassertedReason>) -> Self {
        Place {
            location: format!("{}::{name}", self.location),
            own: Own::Nested,
            unasserted: self.unasserted.or(reason),
            ..self.clone()
        }
    }
}

#[derive(Debug)]
enum WorkItem<'a> {
    Process {
        field_type: &'a FieldType,
        place: Place,
    },
    /// A struct field's value, `owner` naming where its declared type is
    /// recorded.
    ProcessField {
        field: &'a StructField,
        owner: Option<FieldOwner>,
        place: Place,
    },
    PushString(String),
    /// Adds `validators` at `place` to the value built last, whose zero
    /// value, when the schema gives it one, they must also accept.
    Check {
        validators: &'a [Validator],
        place: Place,
        optional: bool,
        zero: Option<MockValue<'static>>,
    },
    /// Makes the value built last's assertion hold only where `condition`
    /// does not: a variant's checks apply to that variant alone.
    Unless {
        condition: String,
    },
    AssembleOption {
        expr: String,
    },
    AssembleVec {
        expr: String,
        binding: String,
    },
    /// A map's value, built first, and its key.
    AssembleMap {
        expr: String,
        binding: String,
    },
    AssembleTuple {
        count: usize,
    },
    AssembleStruct {
        count: usize,
        names: Vec<String>,
    },
    /// The variants, and which one the schema's fallback `DEFAULT` is.
    AssembleEnum {
        count: usize,
        default: usize,
    },
    WrapInVariantKey {
        variant_name: String,
    },
    EnterStructScope {
        name: String,
    },
    LeaveStructScope {
        name: String,
    },
}

/// The walk's state: the work still to do, the values built so far, and
/// the structs being walked, which a struct holding itself meets again.
struct Stacks<'a> {
    work: Vec<WorkItem<'a>>,
    built: Vec<Built>,
    visited: HashSet<String>,
}

impl Stacks<'_> {
    fn pop(&mut self, item: &str) -> Result<Built> {
        self.built
            .pop()
            .ok_or_else(|| EvenframeError::FieldDefinition {
                message: format!("Stack underflow in {item}"),
                work_stack: format!("{:#?}", self.work),
                value_stack: "[]".to_owned(),
                item: item.to_owned(),
                visited_types: format!("{:#?}", self.visited),
            })
    }

    /// The last `count` values built, in the order they were built.
    fn pop_many(&mut self, count: usize, item: &str) -> Result<Vec<Built>> {
        let mut values = (0..count)
            .map(|_| self.pop(item))
            .collect::<Result<Vec<_>>>()?;
        values.reverse();
        Ok(values)
    }
}

#[derive(Debug)]
struct Built {
    surql: String,
    map_values: Option<String>,
    assertion: Option<String>,
    zero_ok: bool,
}

impl Built {
    fn of(surql: impl Into<String>) -> Self {
        Built {
            surql: surql.into(),
            map_values: None,
            assertion: None,
            zero_ok: true,
        }
    }
}

struct Walk<'c, 'a> {
    context: &'c DefineContext<'a>,
    table_name: &'c str,
    /// Whether the field is a record's `id`, which a foreign type may store
    /// in its own form.
    id_field: bool,
}

impl<'a> Walk<'_, 'a> {
    fn run(&self, start: WorkItem<'a>, unasserted: &mut Vec<Unasserted>) -> Result<Shape> {
        let mut stacks = Stacks {
            work: vec![start],
            built: Vec::new(),
            visited: HashSet::new(),
        };
        let options = self.context.options;

        while let Some(item) = stacks.work.pop() {
            match item {
                WorkItem::Process { field_type, place } => {
                    self.process(field_type, place, &mut stacks, unasserted)?
                }
                WorkItem::ProcessField {
                    field,
                    owner,
                    place,
                } => {
                    // A field with a default of its own is not given the zero.
                    let explicit_default = field
                        .define_config
                        .as_ref()
                        .is_some_and(|define| define.default.is_some());
                    stacks.work.push(WorkItem::Check {
                        validators: &field.validators,
                        place: place.clone(),
                        optional: matches!(field.field_type, FieldType::Option(_)),
                        zero: (!explicit_default)
                            .then(|| zero_of(&field.field_type))
                            .flatten(),
                    });
                    if field.wire.storage.opaque {
                        stacks.built.push(Built::of("any"));
                    } else {
                        let declared = owner
                            .as_ref()
                            .and_then(|owner| self.context.declared.field(owner, &field.field_name))
                            .unwrap_or(&field.field_type);
                        stacks.work.push(WorkItem::Process {
                            field_type: declared,
                            place: place.field(),
                        });
                    }
                }
                WorkItem::PushString(surql) => stacks.built.push(Built::of(surql)),
                WorkItem::Check {
                    validators,
                    place,
                    optional,
                    zero,
                } => {
                    let mut value = stacks.pop("Check")?;
                    value.zero_ok &= zero_meets(validators, zero);
                    if let Some(assertion) = checked(validators, &place.expr, optional, options) {
                        match place.unasserted {
                            Some(reason) => unasserted.push(Unasserted {
                                location: place.location,
                                validators: describe(validators),
                                reason,
                            }),
                            None => {
                                value.assertion = all_of([Some(assertion), value.assertion]);
                            }
                        }
                    }
                    stacks.built.push(value);
                }
                WorkItem::Unless { condition } => {
                    let mut value = stacks.pop("Unless")?;
                    value.assertion = value
                        .assertion
                        .map(|assertion| format!("{condition} OR ({assertion})"));
                    stacks.built.push(value);
                }
                WorkItem::AssembleOption { expr } => {
                    let inner = stacks.pop("AssembleOption")?;
                    stacks.built.push(Built {
                        surql: options.option_none.surql_type(&inner.surql),
                        map_values: inner.map_values,
                        assertion: inner.assertion.map(|assertion| {
                            format!(
                                "{expr} = {} OR ({assertion})",
                                options.option_none.literal()
                            )
                        }),
                        // An absent value is an option's zero.
                        zero_ok: true,
                    });
                }
                WorkItem::AssembleVec { expr, binding } => {
                    let element = stacks.pop("AssembleVec")?;
                    stacks.built.push(Built {
                        surql: format!("array<{}>", element.surql),
                        map_values: None,
                        assertion: element.assertion.map(|assertion| {
                            format!("array::all({expr}, |{binding}| {assertion})")
                        }),
                        // An empty array holds no element to check.
                        zero_ok: true,
                    });
                }
                WorkItem::AssembleMap { expr, binding } => {
                    let key = stacks.pop("AssembleMap")?;
                    let value = stacks.pop("AssembleMap")?;
                    let over = |collected: &str, assertion: Option<String>| {
                        assertion.map(|assertion| {
                            format!("array::all({collected}({expr}), |{binding}| {assertion})")
                        })
                    };
                    stacks.built.push(Built {
                        surql: "object".to_owned(),
                        assertion: all_of([
                            over("object::keys", key.assertion),
                            over("object::values", value.assertion),
                        ]),
                        map_values: Some(value.surql),
                        zero_ok: true,
                    });
                }
                WorkItem::AssembleTuple { count } => {
                    let items = stacks.pop_many(count, "AssembleTuple")?;
                    stacks.built.push(Built {
                        surql: format!(
                            "[{}]",
                            items
                                .iter()
                                .map(|item| item.surql.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        map_values: None,
                        zero_ok: items.iter().all(|item| item.zero_ok),
                        assertion: all_of(items.into_iter().map(|item| item.assertion)),
                    });
                }
                WorkItem::AssembleStruct { count, names } => {
                    let members = stacks.pop_many(count, "AssembleStruct")?;
                    let surql = names
                        .iter()
                        .zip(&members)
                        .map(|(name, member)| format!("{}: {}", surql_ident(name), member.surql))
                        .collect::<Vec<_>>()
                        .join(", ");
                    stacks.built.push(Built {
                        surql: format!("{{ {surql} }}"),
                        map_values: None,
                        zero_ok: members.iter().all(|member| member.zero_ok),
                        assertion: all_of(members.into_iter().map(|member| member.assertion)),
                    });
                }
                WorkItem::AssembleEnum { count, default } => {
                    let variants = stacks.pop_many(count, "AssembleEnum")?;
                    stacks.built.push(Built {
                        zero_ok: variants.get(default).is_none_or(|variant| variant.zero_ok),
                        surql: variants
                            .iter()
                            .map(|variant| variant.surql.as_str())
                            .collect::<Vec<_>>()
                            .join(" | "),
                        map_values: None,
                        assertion: all_of(variants.into_iter().map(|variant| variant.assertion)),
                    });
                }
                WorkItem::WrapInVariantKey { variant_name } => {
                    let inner = stacks.pop("WrapInVariantKey")?;
                    stacks.built.push(Built {
                        surql: format!("{{ {}: {} }}", surql_ident(&variant_name), inner.surql),
                        map_values: None,
                        assertion: inner.assertion,
                        zero_ok: inner.zero_ok,
                    });
                }
                WorkItem::EnterStructScope { name } => {
                    stacks.visited.insert(name);
                }
                WorkItem::LeaveStructScope { name } => {
                    stacks.visited.remove(&name);
                }
            }
        }
        let shape = stacks.pop("the field")?;
        Ok(Shape {
            surql: shape.surql,
            map_values: shape.map_values,
            assertion: shape.assertion,
            zero_ok: shape.zero_ok,
        })
    }

    fn process(
        &self,
        field_type: &'a FieldType,
        place: Place,
        stacks: &mut Stacks<'a>,
        unasserted: &mut Vec<Unasserted>,
    ) -> Result<()> {
        let context = self.context;
        match field_type {
            FieldType::String | FieldType::Char => stacks.built.push(Built::of("string")),
            FieldType::Bool => stacks.built.push(Built::of("bool")),
            FieldType::F32 | FieldType::F64 => stacks.built.push(Built::of("float")),
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
            | FieldType::Usize => stacks.built.push(Built::of("int")),
            FieldType::Unit => stacks.built.push(Built::of("any")),
            FieldType::Duration => stacks.built.push(Built::of("duration")),
            // The database holds the value the text writes.
            FieldType::FromText(kind) => stacks.built.push(Built::of(kind.surql_type())),
            FieldType::IsoDate | FieldType::EpochMillis => stacks.built.push(Built::of("datetime")),
            FieldType::JsonText(inner) => stacks.work.push(WorkItem::Process {
                field_type: inner,
                place,
            }),
            FieldType::Option(inner) => {
                stacks.work.push(WorkItem::AssembleOption {
                    expr: place.expr.clone(),
                });
                stacks.work.push(WorkItem::Process {
                    field_type: inner,
                    place: place.present(),
                });
            }
            FieldType::Vec(inner) => {
                let (binding, element) = place.each("[]");
                stacks.work.push(WorkItem::AssembleVec {
                    expr: place.expr,
                    binding,
                });
                stacks.work.push(WorkItem::Process {
                    field_type: inner,
                    place: element,
                });
            }
            FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => {
                let (binding, key_place) = place.each("{key}");
                let (_, value_place) = place.each("{}");
                stacks.work.push(WorkItem::AssembleMap {
                    expr: place.expr,
                    binding,
                });
                stacks.work.push(WorkItem::Process {
                    field_type: key,
                    place: key_place,
                });
                stacks.work.push(WorkItem::Process {
                    field_type: value,
                    place: value_place,
                });
            }
            FieldType::RecordLink(inner) => match inner.as_ref() {
                FieldType::Other(type_name) => {
                    let resolved = record_link_target_surql(
                        type_name,
                        context.tables,
                        context.objects,
                        context.enums,
                    )
                    .unwrap_or_else(|| type_name.to_case(Case::Snake));
                    stacks.built.push(Built::of(format!("record<{resolved}>")));
                }
                _ => stacks.work.push(WorkItem::Process {
                    field_type: inner,
                    place,
                }),
            },
            FieldType::Tuple(items) => {
                stacks
                    .work
                    .push(WorkItem::AssembleTuple { count: items.len() });
                for (position, item) in items.iter().enumerate().rev() {
                    stacks.work.push(WorkItem::Process {
                        field_type: item,
                        place: place.item(position),
                    });
                }
            }
            FieldType::Struct(members) => {
                stacks.work.push(WorkItem::AssembleStruct {
                    count: members.len(),
                    names: members.iter().map(|(name, _)| name.clone()).collect(),
                });
                for (name, member) in members.iter().rev() {
                    stacks.work.push(WorkItem::Process {
                        field_type: member,
                        place: place.key(name),
                    });
                }
            }
            FieldType::Other(name) => {
                self.process_named(name, field_type, place, stacks, unasserted)?
            }
        }
        Ok(())
    }

    fn process_named(
        &self,
        name: &'a String,
        field_type: &'a FieldType,
        place: Place,
        stacks: &mut Stacks<'a>,
        unasserted: &mut Vec<Unasserted>,
    ) -> Result<()> {
        let context = self.context;
        if let Some((inner, validators)) = context.declared.newtype(field_type) {
            if place.own == Own::Nested {
                stacks.work.push(WorkItem::Check {
                    validators,
                    place: place.clone(),
                    optional: false,
                    zero: zero_of(inner),
                });
            }
            stacks.work.push(WorkItem::Process {
                field_type: inner,
                place: place.within_newtype(),
            });
        } else if let Some(foreign) = context.registry.lookup(name) {
            let format = if self.id_field {
                foreign
                    .surrealdb_id_format
                    .as_ref()
                    .map(|format| format.replace("{table_name}", self.table_name))
            } else {
                foreign.surrealdb_non_id_format.clone()
            };
            stacks.built.push(Built::of(
                format.unwrap_or_else(|| foreign.surrealdb.clone()),
            ));
        } else if let Some(tagged_union) = context.enums.get(name) {
            self.process_enum(name, tagged_union.effective(), &place, &mut stacks.work)?;
        } else if let Some(object) = context.objects.get(name) {
            let object = object.effective();
            // A struct that is itself a table, such as a projection that
            // overrides to the underlying one, is stored as a link.
            let snake = object.struct_name.to_case(Case::Snake);
            if let Some(table) = context.tables.get(&snake) {
                stacks.built.push(Built::of(format!(
                    "record<{}>",
                    table.effective().table_name
                )));
            } else if stacks.visited.contains(name) || object.is_open() {
                // A struct holding itself, or keys known only from a value,
                // is any object.
                let reason = if stacks.visited.contains(name) {
                    UnassertedReason::Recursive
                } else {
                    UnassertedReason::Open
                };
                let owner = FieldOwner::Object(name.clone());
                for member in object.fields.iter().map(StructField::effective) {
                    let held = context.declared.field(&owner, &member.field_name).is_some();
                    if !member.validators.is_empty() || held {
                        unasserted.push(Unasserted {
                            location: place.key(member.db_name()).location,
                            validators: if member.validators.is_empty() {
                                "a newtype's validators".to_owned()
                            } else {
                                describe(&member.validators)
                            },
                            reason,
                        });
                    }
                }
                stacks.built.push(Built::of("object"));
            } else {
                stacks
                    .work
                    .push(WorkItem::LeaveStructScope { name: name.clone() });
                self.push_struct(
                    object,
                    FieldOwner::Object(name.clone()),
                    &place,
                    Vec::new(),
                    &mut stacks.work,
                );
                stacks
                    .work
                    .push(WorkItem::EnterStructScope { name: name.clone() });
            }
        } else if let Some(table) = context.tables.get(&name.to_case(Case::Snake)) {
            stacks.built.push(Built::of(format!(
                "record<{}>",
                table.effective().table_name
            )));
        } else {
            stacks.built.push(Built::of(name.clone()));
        }
        Ok(())
    }

    /// Pushes the items that build `object`'s members at `place`, after
    /// `leading` (the keys pushed before them, such as a tag).
    fn push_struct(
        &self,
        object: &'a StructConfig,
        owner: FieldOwner,
        place: &Place,
        leading: Vec<(String, WorkItem<'a>)>,
        work: &mut Vec<WorkItem<'a>>,
    ) {
        let mut names: Vec<String> = leading.iter().map(|(name, _)| name.clone()).collect();
        names.extend(
            object
                .fields
                .iter()
                .map(|field| field.effective().db_name().to_owned()),
        );
        work.push(WorkItem::AssembleStruct {
            count: names.len(),
            names,
        });
        for field in object.fields.iter().rev() {
            let field = field.effective();
            work.push(WorkItem::ProcessField {
                field,
                owner: Some(owner.clone()),
                place: place.key(field.db_name()).field(),
            });
        }
        for (_, item) in leading.into_iter().rev() {
            work.push(item);
        }
    }

    /// For each variant, in order, how its checks are guarded: a tagged one
    /// by its tag, an untagged one by serde's first match, or not at all once
    /// a variant's shape cannot be stated, since every later one's match
    /// depends on it.
    fn untagged_guards(&self, tagged_union: &TaggedUnion, expr: &str) -> Vec<Guard> {
        let mut earlier: Vec<String> = Vec::new();
        let mut blocked = false;
        tagged_union
            .variants
            .iter()
            .map(|variant| {
                let variant = variant.effective();
                let representation = variant.stored_representation(&tagged_union.representation);
                if !matches!(representation, EnumRepresentation::Untagged) {
                    return Guard::Tagged;
                }
                match self.untagged_match(variant, expr).filter(|_| !blocked) {
                    Some(matcher) => {
                        let condition = std::iter::once(format!("!({matcher})"))
                            .chain(earlier.iter().map(|earlier| format!("({earlier})")))
                            .collect::<Vec<_>>()
                            .join(" OR ");
                        earlier.push(matcher);
                        Guard::Untagged(condition)
                    }
                    None => {
                        blocked = true;
                        Guard::Unknown
                    }
                }
            })
            .collect()
    }

    /// A condition on the value at `expr` that holds where serde reads it as
    /// the untagged `variant`, when SurrealQL can state one.
    fn untagged_match(&self, variant: &Variant, expr: &str) -> Option<String> {
        let storage = &variant.wire.storage;
        if storage.skipped {
            return Some(format!("{expr} = NONE"));
        }
        if let Some(literal) = &storage.value {
            return Some(format!("{expr} = {literal}"));
        }
        match &variant.data {
            None => Some(format!(
                "{expr} = {}",
                surql_string_literal(variant.db_name())
            )),
            Some(VariantData::InlineStruct(inline)) => Some(object_match(inline.effective(), expr)),
            // `#[surreal(tuple)]` stores the payload as an array.
            Some(VariantData::DataStructureRef(_)) if storage.tuple => {
                Some(format!("type::is_array({expr})"))
            }
            Some(VariantData::DataStructureRef(payload)) => self.kind_match(payload, expr),
        }
    }

    /// A condition on the value at `expr` that holds where it is a value of
    /// `field_type`'s kind, when SurrealQL can state one.
    fn kind_match(&self, field_type: &FieldType, expr: &str) -> Option<String> {
        let is = |kind: &str| Some(format!("type::is_{kind}({expr})"));
        match field_type {
            FieldType::String | FieldType::Char => is("string"),
            FieldType::Bool => is("bool"),
            FieldType::F32
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
            | FieldType::Usize => is("number"),
            FieldType::FromText(kind) => self.kind_match(kind.value_type(), expr),
            FieldType::Duration => is("duration"),
            FieldType::IsoDate | FieldType::EpochMillis => is("datetime"),
            FieldType::RecordLink(_) => is("record"),
            FieldType::Vec(_) | FieldType::Tuple(_) => is("array"),
            FieldType::HashMap(_, _) | FieldType::BTreeMap(_, _) | FieldType::Struct(_) => {
                is("object")
            }
            FieldType::JsonText(inner) => self.kind_match(inner, expr),
            FieldType::Other(name) => {
                let context = self.context;
                if let Some((inner, _)) = context.declared.newtype(field_type) {
                    self.kind_match(inner, expr)
                } else if let Some(foreign) = context.registry.lookup(name) {
                    match foreign.surrealdb.as_str() {
                        "string" => is("string"),
                        "uuid" => is("uuid"),
                        "datetime" => is("datetime"),
                        "decimal" | "float" | "int" | "number" => is("number"),
                        "bool" => is("bool"),
                        "duration" => is("duration"),
                        other if other.starts_with("record") => is("record"),
                        _ => None,
                    }
                } else if let Some(object) = context.objects.get(name) {
                    let object = object.effective();
                    if context
                        .tables
                        .contains_key(&object.struct_name.to_case(Case::Snake))
                    {
                        is("record")
                    } else {
                        Some(object_match(object, expr))
                    }
                } else if context.tables.contains_key(&name.to_case(Case::Snake)) {
                    is("record")
                } else {
                    None
                }
            }
            // An option or a unit reads from absence, which another variant's
            // value may also hold.
            FieldType::Option(_) | FieldType::Unit => None,
        }
    }

    fn process_enum(
        &self,
        enum_key: &'a str,
        tagged_union: &'a TaggedUnion,
        place: &Place,
        work: &mut Vec<WorkItem<'a>>,
    ) -> Result<()> {
        work.push(WorkItem::AssembleEnum {
            count: tagged_union.variants.len(),
            // The fallback `DEFAULT` takes the `#[default]` variant, else the
            // first.
            default: tagged_union
                .variants
                .iter()
                .position(|variant| variant.is_default)
                .unwrap_or(0),
        });
        let guards = self.untagged_guards(tagged_union, &place.expr);
        for (variant, guard) in tagged_union.variants.iter().zip(guards).rev() {
            let variant = variant.effective();
            let storage = &variant.wire.storage;
            // A variant serde skips is stored as NONE.
            if storage.skipped {
                work.push(WorkItem::PushString("none".to_owned()));
                continue;
            }
            // An untagged unit variant is stored as its literal.
            if let Some(literal) = &storage.value {
                work.push(WorkItem::PushString(match literal.as_str() {
                    "NULL" => "null".to_owned(),
                    "NONE" => "none".to_owned(),
                    literal => literal.to_owned(),
                }));
                continue;
            }
            let representation = variant.stored_representation(&tagged_union.representation);
            let tag_literal = format!("\"{}\"", variant.db_name());
            let tag_is = |tag: &str| {
                format!(
                    "{}.{} != {}",
                    place.expr,
                    surql_ident(tag),
                    surql_string_literal(variant.db_name())
                )
            };
            let keyed = || format!("{}.{} = NONE", place.expr, surql_ident(variant.db_name()));
            let reason = match &guard {
                Guard::Unknown => Some(UnassertedReason::Untagged),
                Guard::Tagged | Guard::Untagged(_) => None,
            };
            let here = place.variant(&variant.name, reason);
            // An untagged variant's checks hold where serde reads the value
            // as that variant: it matches, and no earlier one does.
            if let Guard::Untagged(condition) = guard
                && variant.data.is_some()
            {
                work.push(WorkItem::Unless { condition });
            }
            match &variant.data {
                Some(VariantData::InlineStruct(inline)) => {
                    let inline = inline.effective();
                    let owner = FieldOwner::Variant {
                        enum_name: enum_key.to_owned(),
                        variant: variant.name.clone(),
                    };
                    match representation {
                        EnumRepresentation::ExternallyTagged => {
                            // { VariantName: { fields } }
                            work.push(WorkItem::Unless { condition: keyed() });
                            work.push(WorkItem::WrapInVariantKey {
                                variant_name: variant.db_name().to_owned(),
                            });
                            self.push_struct(
                                inline,
                                owner,
                                &here.content(variant.db_name()),
                                Vec::new(),
                                work,
                            );
                        }
                        EnumRepresentation::InternallyTagged { tag } => {
                            // { tag: "VariantName", field1: type1, ... }
                            work.push(WorkItem::Unless {
                                condition: tag_is(tag),
                            });
                            self.push_struct(
                                inline,
                                owner,
                                &here,
                                vec![(tag.clone(), WorkItem::PushString(tag_literal))],
                                work,
                            );
                        }
                        EnumRepresentation::AdjacentlyTagged { tag, content } => {
                            // { tag: "VariantName", content: { fields } }
                            work.push(WorkItem::Unless {
                                condition: tag_is(tag),
                            });
                            let content_place = here.content(content);
                            let mut content_items = Vec::new();
                            self.push_struct(
                                inline,
                                owner,
                                &content_place,
                                Vec::new(),
                                &mut content_items,
                            );
                            work.extend(adjacent_items(
                                tag,
                                content,
                                tag_literal,
                                storage,
                                &content_place,
                                content_items,
                            ));
                        }
                        EnumRepresentation::Untagged => {
                            // { fields } (no wrapping)
                            self.push_struct(inline, owner, &here, Vec::new(), work);
                        }
                    }
                }
                Some(VariantData::DataStructureRef(stored)) => {
                    let payload = self
                        .context
                        .declared
                        .payload(enum_key, &variant.name)
                        .unwrap_or(stored);
                    match representation {
                        EnumRepresentation::ExternallyTagged => {
                            // { VariantName: value }
                            work.push(WorkItem::Unless { condition: keyed() });
                            work.push(WorkItem::WrapInVariantKey {
                                variant_name: variant.db_name().to_owned(),
                            });
                            work.extend(payload_items(
                                payload,
                                storage,
                                &here.content(variant.db_name()),
                            ));
                        }
                        EnumRepresentation::AdjacentlyTagged { tag, content } => {
                            // { tag: "VariantName", content: value }
                            work.push(WorkItem::Unless {
                                condition: tag_is(tag),
                            });
                            let content_place = here.content(content);
                            let content_items = payload_items(payload, storage, &content_place);
                            work.extend(adjacent_items(
                                tag,
                                content,
                                tag_literal,
                                storage,
                                &content_place,
                                content_items,
                            ));
                        }
                        EnumRepresentation::Untagged => {
                            // value (no wrapping)
                            work.extend(payload_items(payload, storage, &here));
                        }
                        EnumRepresentation::InternallyTagged { tag } => {
                            let (struct_key, struct_config) = match stored {
                                FieldType::Other(name) => self
                                    .context
                                    .objects
                                    .get_key_value(name)
                                    .map(|(key, object)| (key, object.effective())),
                                _ => None,
                            }
                            .ok_or_else(|| {
                                EvenframeError::SchemaSync(format!(
                                    "Internally tagged enum `{}` variant `{}` must reference a \
                                     known struct payload; found `{stored:?}`",
                                    tagged_union.enum_name,
                                    variant.db_name(),
                                ))
                            })?;
                            work.push(WorkItem::Unless {
                                condition: tag_is(tag),
                            });
                            self.push_struct(
                                struct_config,
                                FieldOwner::Object(struct_key.clone()),
                                &here,
                                vec![(tag.clone(), WorkItem::PushString(tag_literal))],
                                work,
                            );
                        }
                    }
                }
                None => match representation {
                    // { tag: "VariantName" }
                    EnumRepresentation::InternallyTagged { tag }
                    | EnumRepresentation::AdjacentlyTagged { tag, .. } => {
                        work.push(WorkItem::AssembleStruct {
                            count: 1,
                            names: vec![tag.clone()],
                        });
                        work.push(WorkItem::PushString(tag_literal));
                    }
                    // "VariantName"
                    EnumRepresentation::ExternallyTagged | EnumRepresentation::Untagged => {
                        work.push(WorkItem::PushString(tag_literal));
                    }
                },
            }
        }
        Ok(())
    }
}

/// How a variant's checks are guarded in its enum's assertion.
enum Guard {
    /// By its tag, which the variant's own items state.
    Tagged,
    /// By serde's first match among the untagged variants.
    Untagged(String),
    /// Not at all: its shape, or an earlier untagged variant's, cannot be
    /// stated, so its checks are reported instead.
    Unknown,
}

/// A condition on the value at `expr` that holds where it is an object
/// holding every key `object` always stores.
fn object_match(object: &StructConfig, expr: &str) -> String {
    std::iter::once(format!("type::is_object({expr})"))
        .chain(
            object
                .fields
                .iter()
                .map(StructField::effective)
                .filter(|field| {
                    !matches!(field.field_type, FieldType::Option(_)) && !field.wire.storage.skipped
                })
                .map(|field| format!("{expr}.{} != NONE", surql_ident(field.db_name()))),
        )
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// The work items a tuple variant's payload at `place` pushes: its element,
/// or each of several, as `#[surreal(tuple)]` and `#[surreal(wrap)]` store
/// them, in the order they are pushed.
fn payload_items<'a>(
    payload: &'a FieldType,
    storage: &Storage,
    place: &Place,
) -> Vec<WorkItem<'a>> {
    let element = |item: &'a FieldType, position: usize, at: Place| match storage
        .opaque_elements
        .get(position)
    {
        Some(true) => WorkItem::PushString("any".to_owned()),
        _ => WorkItem::Process {
            field_type: item,
            place: at,
        },
    };
    match payload {
        FieldType::Tuple(items) if storage.opaque_elements.contains(&true) => {
            let mut pushed = vec![WorkItem::AssembleTuple { count: items.len() }];
            pushed.extend(
                items
                    .iter()
                    .enumerate()
                    .rev()
                    .map(|(position, item)| element(item, position, place.item(position))),
            );
            pushed
        }
        single if storage.tuple => vec![
            WorkItem::AssembleTuple { count: 1 },
            element(single, 0, place.item(0)),
        ],
        single => vec![element(single, 0, place.clone())],
    }
}

/// The work items an adjacently tagged variant pushes: its tag, and its
/// content at `content_place` as `#[surreal(skip_content)]` stores it, from
/// the payload's own `content_items`, in the order they are pushed.
fn adjacent_items<'a>(
    tag: &str,
    content: &str,
    tag_literal: String,
    storage: &Storage,
    content_place: &Place,
    content_items: Vec<WorkItem<'a>>,
) -> Vec<WorkItem<'a>> {
    let tag_value = WorkItem::PushString(tag_literal);
    match storage.content {
        ContentStorage::Never => vec![
            WorkItem::AssembleStruct {
                count: 1,
                names: vec![tag.to_owned()],
            },
            tag_value,
        ],
        ContentStorage::Sometimes | ContentStorage::Always => {
            let mut pushed = vec![WorkItem::AssembleStruct {
                count: 2,
                names: vec![tag.to_owned(), content.to_owned()],
            }];
            if storage.content == ContentStorage::Sometimes {
                pushed.push(WorkItem::AssembleOption {
                    expr: content_place.expr.clone(),
                });
            }
            pushed.extend(content_items);
            pushed.push(tag_value);
            pushed
        }
    }
}

fn describe(validators: &[Validator]) -> String {
    validators
        .iter()
        .map(Validator::describe)
        .collect::<Vec<_>>()
        .join(", ")
}

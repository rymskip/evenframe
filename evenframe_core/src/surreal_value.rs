//! What the derive's `SurrealValue` impls reach for. A derived type converts
//! to and from the database's value in the shape evenframe's schema defines
//! for it: each field under its database name, each enum in serde's
//! representation.

#[doc(hidden)]
pub mod __private {
    use crate::validator::validate::Validate;
    use serde::{Serialize, de::DeserializeOwned};
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::marker::PhantomData;

    pub use surrealdb_types::{
        Array, Error, Kind, KindLiteral, Number, Object, SerdeWrapper, SerializationError,
        SurrealValue, Table, Value,
    };

    /// Converts a field's value with its type's own `SurrealValue`, or
    /// through serde where the type has none. `(&Field::<T>::new()).write(..)`
    /// finds the impl on `Field` before the one on `&Field` that needs another
    /// borrow, and that first impl applies only to a `SurrealValue` type.
    pub struct Field<T>(PhantomData<T>);

    impl<T> Field<T> {
        pub fn new() -> Self {
            Self(PhantomData)
        }
    }

    impl<T> Default for Field<T> {
        fn default() -> Self {
            Self::new()
        }
    }

    pub trait Convert<T> {
        fn kind(&self) -> Kind;
        fn write(&self, value: T) -> Value;
        fn read(&self, value: Value) -> Result<T, Error>;
    }

    impl<T: SurrealValue> Convert<T> for Field<T> {
        fn kind(&self) -> Kind {
            T::kind_of()
        }

        fn write(&self, value: T) -> Value {
            value.into_value()
        }

        fn read(&self, value: Value) -> Result<T, Error> {
            T::from_value(value)
        }
    }

    impl<T: Serialize + DeserializeOwned + 'static> Convert<T> for &Field<T> {
        fn kind(&self) -> Kind {
            SerdeWrapper::<T>::kind_of()
        }

        fn write(&self, value: T) -> Value {
            SerdeWrapper(value).into_value()
        }

        fn read(&self, value: Value) -> Result<T, Error> {
            SerdeWrapper::<T>::from_value(value).map(|wrapper| wrapper.0)
        }
    }

    thread_local! {
        /// The types whose kind is being described, outermost first.
        static DESCRIBING: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
    }

    /// Pops the type `kind_of` pushed, also when describing it panicked.
    struct Described;

    impl Drop for Described {
        fn drop(&mut self) {
            DESCRIBING.with(|describing| describing.borrow_mut().pop());
        }
    }

    /// The kind `describe` gives `T`, or `Kind::Any` where `T` sits inside
    /// its own description: a recursive type nests without end, which no
    /// finite kind describes.
    pub fn kind_of<T: ?Sized>(describe: impl FnOnce() -> Kind) -> Kind {
        let name = std::any::type_name::<T>();
        let entered = DESCRIBING.with(|describing| {
            let mut describing = describing.borrow_mut();
            let entered = !describing.contains(&name);
            if entered {
                describing.push(name);
            }
            entered
        });
        if !entered {
            return Kind::Any;
        }
        let _described = Described;
        describe()
    }

    /// The keys a struct reads, which a flattened read of it takes from the
    /// keys its siblings left, as serde's does.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` is not stored as an object of named keys",
        label = "an internally tagged variant stores its tag beside this payload's keys",
        note = "serde fails at runtime to write a tag beside anything but an object: hold a \
                struct of named fields deriving Evenframe, or tag the enum adjacently"
    )]
    pub trait StoredKeys {
        fn stored_keys() -> &'static [&'static str];
    }

    /// Holds only where `T` is stored as an object, which an internally tagged
    /// variant's payload must be.
    pub const fn stored_object<T: StoredKeys>() {}

    /// The keys a flattened field of type `T` takes: a struct's own, through
    /// the impl on `Field`; none for any other type, such as a map, which
    /// reads the keys left without taking them.
    pub trait Taken {
        fn taken(&self) -> &'static [&'static str];
    }

    impl<T: StoredKeys> Taken for Field<T> {
        fn taken(&self) -> &'static [&'static str] {
            T::stored_keys()
        }
    }

    impl<T> Taken for &Field<T> {
        fn taken(&self) -> &'static [&'static str] {
            &[]
        }
    }

    /// Removes the keys a flattened read took.
    pub fn take(fields: &mut BTreeMap<String, Value>, keys: &[&str]) {
        for key in keys {
            fields.remove(*key);
        }
    }

    pub fn error(message: String) -> Error {
        Error::serialization(message, SerializationError::Deserialization)
    }

    /// The fields of an object read as `type_name`.
    pub fn object(value: Value, type_name: &str) -> Result<BTreeMap<String, Value>, Error> {
        match value {
            Value::Object(object) => Ok(object.into_inner()),
            other => Err(error(format!(
                "{type_name} must be an object, got {}",
                other.kind()
            ))),
        }
    }

    /// The items of an array read as `type_name` with `length` items.
    pub fn array(value: Value, type_name: &str, length: usize) -> Result<Vec<Value>, Error> {
        match value {
            Value::Array(array) if array.len() == length => Ok(array.into_inner()),
            other => Err(error(format!(
                "{type_name} must be an array of {length} items, got {}",
                other.kind()
            ))),
        }
    }

    /// The value under the first of `keys` present and not NONE, nor NULL for
    /// an `Option`, which reads NULL as absent as serde does. Every one of
    /// them is taken, so none reads as an unknown key.
    pub fn present(
        fields: &mut BTreeMap<String, Value>,
        keys: &[&str],
        optional: bool,
    ) -> Option<Value> {
        keys.iter()
            .filter_map(|key| fields.remove(*key))
            .filter(|value| match value {
                Value::None => false,
                Value::Null => !optional,
                _ => true,
            })
            .reduce(|first, _| first)
    }

    /// A field's value converted, its failure naming the field.
    pub fn field<T>(converted: Result<T, Error>, type_name: &str, key: &str) -> Result<T, Error> {
        converted.map_err(|failure| error(format!("{type_name}.{key}: {failure}")))
    }

    pub fn missing(type_name: &str, key: &str) -> Error {
        error(format!("{type_name} is missing the field `{key}`"))
    }

    /// Rejects keys no field reads, for `#[serde(deny_unknown_fields)]`.
    pub fn deny_unknown(fields: &BTreeMap<String, Value>, type_name: &str) -> Result<(), Error> {
        match fields.keys().next() {
            Some(key) => Err(error(format!("{type_name} has no field `{key}`"))),
            None => Ok(()),
        }
    }

    /// The tag an internally or adjacently tagged enum stores.
    pub fn tag(
        fields: &mut BTreeMap<String, Value>,
        type_name: &str,
        tag: &str,
    ) -> Result<String, Error> {
        match fields.remove(tag) {
            Some(Value::String(name)) => Ok(name),
            Some(other) => Err(error(format!(
                "{type_name}.{tag} must be a variant name, got {}",
                other.kind()
            ))),
            None => Err(missing(type_name, tag)),
        }
    }

    /// The single `{ variant: payload }` entry of an externally tagged enum.
    pub fn external(
        fields: BTreeMap<String, Value>,
        type_name: &str,
    ) -> Result<(String, Value), Error> {
        let mut entries = fields.into_iter();
        match (entries.next(), entries.next()) {
            (Some(entry), None) => Ok(entry),
            _ => Err(error(format!(
                "{type_name} must be a variant name or an object with one variant key"
            ))),
        }
    }

    /// The table a stored record's `id` names.
    pub fn record_table(value: &Value, type_name: &str) -> Result<String, Error> {
        match value {
            Value::Object(object) => match object.get("id") {
                Some(Value::RecordId(id)) => Ok(id.table.as_str().to_owned()),
                _ => Err(error(format!(
                    "{type_name} must be a record with a record id"
                ))),
            },
            other => Err(error(format!(
                "{type_name} must be a record, got {}",
                other.kind()
            ))),
        }
    }

    pub fn unknown_variant(type_name: &str, name: &str) -> Error {
        error(format!("{type_name} has no variant `{name}`"))
    }

    /// `value` once its validators pass, as serde's read would return it.
    pub fn validated<T: Validate>(value: T, type_name: &str) -> Result<T, Error> {
        match value.validate() {
            Ok(()) => Ok(value),
            Err(failures) => Err(error(format!("{type_name}: {failures}"))),
        }
    }

    /// A value as the database's JSON renders it as text, which is how a
    /// query's row read it: a record id as `table:key`, a datetime in RFC 3339,
    /// a string as itself.
    pub fn text(value: Value) -> Result<String, Error> {
        match value.into_json_value() {
            serde_json::Value::String(text) => Ok(text),
            other => Err(error(format!("expected text, got {other}"))),
        }
    }

    /// Each item of a list as [`text`] reads it.
    pub fn texts(value: Value) -> Result<Vec<String>, Error> {
        match value {
            Value::Array(items) => items.into_inner().into_iter().map(text).collect(),
            other => Err(error(format!("expected a list, got {}", other.kind()))),
        }
    }

    /// The keys no sibling read, as the object a flattened field reads.
    pub fn rest(fields: &BTreeMap<String, Value>) -> Value {
        object_value(fields.clone())
    }

    /// A flattened field's own fields beside its siblings', nothing for an
    /// absent `Option`, or the value under its key when it is not an object.
    pub fn merge(fields: &mut BTreeMap<String, Value>, key: &str, value: Value) {
        match value {
            Value::Object(object) => fields.extend(object.into_inner()),
            Value::None => {}
            other => {
                fields.insert(key.to_owned(), other);
            }
        }
    }

    pub fn object_value(fields: BTreeMap<String, Value>) -> Value {
        Value::Object(fields.into_iter().collect())
    }

    pub fn array_value(items: Vec<Value>) -> Value {
        Value::Array(items.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::__private::{Convert, Field, Value};
    use ordered_float::OrderedFloat;

    #[test]
    fn a_type_without_surreal_value_converts_through_serde() {
        let value = (&Field::<OrderedFloat<f64>>::new()).write(OrderedFloat(2.5));
        assert_eq!(value, Value::from_t(2.5_f64));
        let read: OrderedFloat<f64> = (&Field::<OrderedFloat<f64>>::new())
            .read(value)
            .expect("a float reads back");
        assert_eq!(read, OrderedFloat(2.5));
    }

    #[test]
    fn a_surreal_value_type_keeps_its_own_form() {
        let duration = std::time::Duration::from_secs(90);
        let value = Convert::write(&Field::<std::time::Duration>::new(), duration);
        assert!(matches!(value, Value::Duration(_)));
    }
}

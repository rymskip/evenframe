use evenframe_derive::Evenframe;

/// A newtype is written as its one field's value, checked by its validators.
#[derive(Evenframe, serde::Serialize)]
#[validators(StringValidator::Trim, StringValidator::NonEmpty)]
pub struct NonEmptyString(String);

/// A transparent struct is a newtype of the field serde writes.
#[derive(Evenframe, serde::Serialize)]
#[serde(transparent)]
pub struct Label {
    #[validators(StringValidator::MinLength(2))]
    value: String,
    #[serde(skip)]
    cached: Option<usize>,
}

#[derive(Evenframe, serde::Serialize, serde::Deserialize)]
pub struct Profile {
    name: NonEmptyString,
    label: Option<Label>,
}

fn main() {}

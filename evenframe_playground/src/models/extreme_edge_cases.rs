//! EXTREME EDGE CASES - The most complex, convoluted validator combinations
//! Testing every feature Evenframe has to offer in the most challenging ways possible.

use evenframe::types::RecordLink;
use evenframe::Evenframe;
use serde::{Deserialize, Serialize};

// ============================================================================
// EDGE CASE 1: Kitchen Sink String Field
// Every string validator that makes sense together on a single Option<String>
// ============================================================================
#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 5)]
pub struct KitchenSinkString {
    pub id: String,

    /// A username that must be: alphanumeric, lowercase, between 3-20 chars,
    /// start with a letter pattern, trimmed, and non-empty
    #[validators(
        StringValidator::Alphanumeric,
        StringValidator::Lowercased,
        StringValidator::MinLength(3),
        StringValidator::MaxLength(20),
        StringValidator::NonEmpty,
        StringValidator::Trimmed
    )]
    pub strict_username: String,

    /// Optional version of the above - ALL validators should only run if Some
    #[validators(
        StringValidator::Alphanumeric,
        StringValidator::Lowercased,
        StringValidator::MinLength(3),
        StringValidator::MaxLength(20),
        StringValidator::NonEmpty,
        StringValidator::Trimmed
    )]
    pub optional_strict_username: Option<String>,

    /// Email with multiple constraints
    #[format(Email)]
    #[validators(
        StringValidator::Email,
        StringValidator::MaxLength(255),
        StringValidator::Includes("@"),
        StringValidator::NonEmpty
    )]
    pub validated_email: String,

    /// Optional email with format and validators
    #[format(Email)]
    #[validators(
        StringValidator::Email,
        StringValidator::MaxLength(255),
        StringValidator::NonEmpty
    )]
    pub optional_validated_email: Option<String>,

    /// URL with domain constraint and validators
    #[format(Url("api.example.com"))]
    #[validators(
        StringValidator::Url,
        StringValidator::MaxLength(2000),
        StringValidator::StartsWith("https://"),
        StringValidator::NonEmpty
    )]
    pub api_endpoint: String,

    /// Optional URL
    #[format(Url("cdn.example.com"))]
    #[validators(StringValidator::Url, StringValidator::MaxLength(500))]
    pub optional_cdn_url: Option<String>,
}

// ============================================================================
// EDGE CASE 2: All Integer Types With All Applicable NumberValidators
// ============================================================================
#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 3)]
pub struct ExtremeIntegerValidation {
    pub id: String,

    // u8 with multiple constraints
    #[validators(
        NumberValidator::Positive,
        NumberValidator::LessThanOrEqualTo(255.0),
        NumberValidator::Between(1.0, 100.0)
    )]
    pub constrained_u8: u8,

    #[validators(NumberValidator::Positive, NumberValidator::Between(1.0, 100.0))]
    pub optional_constrained_u8: Option<u8>,

    // u16 with bounds
    #[validators(
        NumberValidator::NonNegative,
        NumberValidator::LessThan(65000.0),
        NumberValidator::GreaterThan(0.0)
    )]
    pub bounded_u16: u16,

    #[validators(NumberValidator::NonNegative, NumberValidator::LessThan(1000.0))]
    pub optional_bounded_u16: Option<u16>,

    // u32 percentage (0-100)
    #[validators(
        NumberValidator::NonNegative,
        NumberValidator::LessThanOrEqualTo(100.0),
        NumberValidator::Between(0.0, 100.0)
    )]
    pub percentage_u32: u32,

    #[validators(NumberValidator::Between(0.0, 100.0))]
    pub optional_percentage_u32: Option<u32>,

    // u64 large range
    #[validators(
        NumberValidator::Positive,
        NumberValidator::GreaterThanOrEqualTo(1.0),
        NumberValidator::LessThan(1000000000.0)
    )]
    pub large_u64: u64,

    #[validators(NumberValidator::Positive)]
    pub optional_large_u64: Option<u64>,

    // Signed integers with negative validators
    #[validators(NumberValidator::Negative, NumberValidator::GreaterThan(-100.0))]
    pub negative_i8: i8,

    #[validators(NumberValidator::Negative)]
    pub optional_negative_i8: Option<i8>,

    #[validators(NumberValidator::NonPositive, NumberValidator::GreaterThanOrEqualTo(-1000.0))]
    pub non_positive_i16: i16,

    #[validators(NumberValidator::NonPositive)]
    pub optional_non_positive_i16: Option<i16>,

    // i32 that can be any sign but bounded
    #[validators(NumberValidator::Between(-1000.0, 1000.0))]
    pub bounded_i32: i32,

    #[validators(NumberValidator::Between(-500.0, 500.0))]
    pub optional_bounded_i32: Option<i32>,

    // i64 with multiple constraints
    #[validators(
        NumberValidator::GreaterThan(-9999999.0),
        NumberValidator::LessThan(9999999.0)
    )]
    pub constrained_i64: i64,

    #[validators(NumberValidator::GreaterThanOrEqualTo(0.0))]
    pub optional_non_negative_i64: Option<i64>,

    // usize and isize
    #[validators(NumberValidator::NonNegative, NumberValidator::LessThan(10000.0))]
    pub bounded_usize: usize,

    #[validators(NumberValidator::NonNegative)]
    pub optional_bounded_usize: Option<usize>,

    #[validators(NumberValidator::Between(-100.0, 100.0))]
    pub bounded_isize: isize,

    #[validators(NumberValidator::Positive)]
    pub optional_positive_isize: Option<isize>,
}

// ============================================================================
// EDGE CASE 3: Float Validation Extremes
// ============================================================================
#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 3)]
pub struct ExtremeFloatValidation {
    pub id: String,

    // f32 with all applicable validators
    #[validators(
        NumberValidator::Positive,
        NumberValidator::Between(0.001, 999.999),
        NumberValidator::LessThan(1000.0),
        NumberValidator::GreaterThan(0.0)
    )]
    pub multi_constrained_f32: f32,

    #[validators(NumberValidator::Positive, NumberValidator::LessThan(100.0))]
    pub optional_f32: Option<f32>,

    // f64 with precise bounds
    #[validators(
        NumberValidator::NonNegative,
        NumberValidator::LessThanOrEqualTo(1.0),
        NumberValidator::GreaterThanOrEqualTo(0.0)
    )]
    pub normalized_f64: f64,

    #[validators(NumberValidator::Between(0.0, 1.0))]
    pub optional_normalized_f64: Option<f64>,

    // Negative floats
    #[validators(
        NumberValidator::Negative,
        NumberValidator::GreaterThan(-1000.0),
        NumberValidator::LessThan(0.0)
    )]
    pub negative_f64: f64,

    #[validators(NumberValidator::Negative)]
    pub optional_negative_f64: Option<f64>,

    // Currency-like (2 decimal precision conceptually)
    #[format(CurrencyAmount)]
    #[validators(NumberValidator::NonNegative, NumberValidator::LessThan(1000000.0))]
    pub currency_amount: f64,

    #[format(CurrencyAmount)]
    #[validators(NumberValidator::Positive)]
    pub optional_currency: Option<f64>,

    // Percentage
    #[format(Percentage)]
    #[validators(NumberValidator::Between(0.0, 100.0))]
    pub percentage: f64,

    #[format(Percentage)]
    #[validators(NumberValidator::Between(0.0, 100.0))]
    pub optional_percentage: Option<f64>,

    // Coordinates with format and validation
    #[format(Latitude)]
    #[validators(NumberValidator::Between(-90.0, 90.0))]
    pub latitude: f64,

    #[format(Longitude)]
    #[validators(NumberValidator::Between(-180.0, 180.0))]
    pub longitude: f64,

    #[format(Latitude)]
    #[validators(NumberValidator::Between(-90.0, 90.0))]
    pub optional_latitude: Option<f64>,

    #[format(Longitude)]
    #[validators(NumberValidator::Between(-180.0, 180.0))]
    pub optional_longitude: Option<f64>,
}

// ============================================================================
// EDGE CASE 4: Complex UUID and Identifier Validation
// ============================================================================
#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 5)]
pub struct ComplexIdentifiers {
    pub id: String,

    // UUID with format
    #[format(Uuid)]
    #[validators(StringValidator::Uuid)]
    pub uuid_field: String,

    #[format(Uuid)]
    #[validators(StringValidator::Uuid)]
    pub optional_uuid: Option<String>,

    // Hex identifiers
    #[format(HexString(32))]
    #[validators(StringValidator::Hex, StringValidator::MinLength(32), StringValidator::MaxLength(32))]
    pub hex_id_32: String,

    #[format(HexString(16))]
    #[validators(StringValidator::Hex, StringValidator::MinLength(16))]
    pub optional_hex_id: Option<String>,

    // Base64 tokens
    #[format(Base64String(64))]
    #[validators(StringValidator::MaxLength(100))]
    pub base64_token: String,

    #[format(Base64String(32))]
    #[validators(StringValidator::MaxLength(50))]
    pub optional_base64_token: Option<String>,

    // Version strings
    #[format(Version)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(20))]
    pub version: String,

    #[format(Version)]
    #[validators(StringValidator::MaxLength(20))]
    pub optional_version: Option<String>,

    // Hash values
    #[format(Hash)]
    #[validators(StringValidator::Hex, StringValidator::MinLength(64), StringValidator::MaxLength(64))]
    pub sha256_hash: String,

    #[format(Hash)]
    #[validators(StringValidator::Hex)]
    pub optional_hash: Option<String>,
}

// ============================================================================
// EDGE CASE 5: Network and Address Types
// ============================================================================
#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 5)]
pub struct NetworkTypes {
    pub id: String,

    // IP addresses
    #[format(IpAddress)]
    #[validators(StringValidator::Ip)]
    pub ip_address: String,

    #[format(IpAddress)]
    #[validators(StringValidator::Ip)]
    pub optional_ip: Option<String>,

    // MAC address
    #[format(MacAddress)]
    #[validators(StringValidator::MaxLength(17))]
    pub mac_address: String,

    #[format(MacAddress)]
    #[validators(StringValidator::MaxLength(17))]
    pub optional_mac: Option<String>,

    // URLs with different domains
    #[format(Url("api.internal.example.com"))]
    #[validators(StringValidator::Url, StringValidator::StartsWith("https://"))]
    pub internal_api_url: String,

    #[format(Url("storage.example.com"))]
    #[validators(StringValidator::Url)]
    pub optional_storage_url: Option<String>,

    // User agent
    #[format(UserAgent)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(500))]
    pub user_agent: String,

    #[format(UserAgent)]
    #[validators(StringValidator::MaxLength(500))]
    pub optional_user_agent: Option<String>,
}

// ============================================================================
// EDGE CASE 6: Date and Time With All Formats
// ============================================================================
#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 5)]
pub struct DateTimeExtremes {
    pub id: String,

    // Full datetime
    #[format(DateTime)]
    #[validators(StringValidator::NonEmpty)]
    pub created_at: String,

    #[format(DateTime)]
    pub optional_updated_at: Option<String>,

    // Date only
    #[format(Date)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(10))]
    pub birth_date: String,

    #[format(Date)]
    #[validators(StringValidator::MaxLength(10))]
    pub optional_expiry_date: Option<String>,

    // Time only
    #[format(Time)]
    #[validators(StringValidator::NonEmpty)]
    pub start_time: String,

    #[format(Time)]
    pub optional_end_time: Option<String>,

    // Appointment datetime (special format)
    #[format(AppointmentDateTime)]
    #[validators(StringValidator::NonEmpty)]
    pub appointment: String,

    #[format(AppointmentDateTime)]
    pub optional_followup: Option<String>,

    // Duration
    #[format(Iso8601DurationString)]
    #[validators(StringValidator::NonEmpty)]
    pub duration: String,

    #[format(Iso8601DurationString)]
    pub optional_duration: Option<String>,

    // Timezone
    #[format(TimeZone)]
    #[validators(StringValidator::NonEmpty)]
    pub timezone: String,

    #[format(TimeZone)]
    pub optional_timezone: Option<String>,

    // Date within days
    #[format(DateWithinDays(30))]
    #[validators(StringValidator::NonEmpty)]
    pub deadline: String,

    #[format(DateWithinDays(90))]
    pub optional_target_date: Option<String>,
}

// ============================================================================
// EDGE CASE 7: Personal Information Types
// ============================================================================
#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 10)]
pub struct PersonalInfoExtremes {
    pub id: String,

    // Names
    #[format(FirstName)]
    #[validators(StringValidator::Alpha, StringValidator::NonEmpty, StringValidator::MaxLength(50))]
    pub first_name: String,

    #[format(LastName)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(50))]
    pub last_name: String,

    #[format(FullName)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(100))]
    pub full_name: String,

    #[format(FirstName)]
    #[validators(StringValidator::MaxLength(50))]
    pub optional_nickname: Option<String>,

    // Phone
    #[format(PhoneNumber)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(20))]
    pub phone: String,

    #[format(PhoneNumber)]
    #[validators(StringValidator::MaxLength(20))]
    pub optional_mobile: Option<String>,

    // Email
    #[format(Email)]
    #[validators(StringValidator::Email, StringValidator::MaxLength(255))]
    pub email: String,

    #[format(Email)]
    #[validators(StringValidator::Email)]
    pub optional_work_email: Option<String>,

    // Address components
    #[format(StreetAddress)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(200))]
    pub street: String,

    #[format(City)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(100))]
    pub city: String,

    #[format(State)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(50))]
    pub state: String,

    #[format(PostalCode)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(20))]
    pub postal_code: String,

    #[format(Country)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(100))]
    pub country: String,

    // Optional address
    #[format(StreetAddress)]
    #[validators(StringValidator::MaxLength(200))]
    pub optional_street: Option<String>,

    #[format(City)]
    #[validators(StringValidator::MaxLength(100))]
    pub optional_city: Option<String>,
}

// ============================================================================
// EDGE CASE 8: Business/Commerce Types
// ============================================================================
#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 5)]
pub struct BusinessExtremes {
    pub id: String,

    // Company info
    #[format(CompanyName)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(200))]
    pub company_name: String,

    #[format(JobTitle)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(100))]
    pub job_title: String,

    #[format(CompanyName)]
    #[validators(StringValidator::MaxLength(200))]
    pub optional_parent_company: Option<String>,

    // Product info
    #[format(ProductName)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(200))]
    pub product_name: String,

    #[format(ProductSku)]
    #[validators(StringValidator::Alphanumeric, StringValidator::MinLength(6), StringValidator::MaxLength(20))]
    pub sku: String,

    #[format(ProductName)]
    #[validators(StringValidator::MaxLength(200))]
    pub optional_variant_name: Option<String>,

    #[format(ProductSku)]
    #[validators(StringValidator::MaxLength(20))]
    pub optional_variant_sku: Option<String>,

    // Pricing
    #[format(CurrencyAmount)]
    #[validators(NumberValidator::Positive, NumberValidator::LessThan(100000.0))]
    pub price: f64,

    #[format(CurrencyAmount)]
    #[validators(NumberValidator::NonNegative)]
    pub optional_discount: Option<f64>,

    // Quantity
    #[validators(NumberValidator::NonNegative, NumberValidator::LessThan(10000.0))]
    pub quantity: u32,

    #[validators(NumberValidator::Positive)]
    pub optional_min_quantity: Option<u32>,

    // Credit card (for testing - obviously don't store real ones!)
    #[format(CreditCardNumber)]
    #[validators(StringValidator::Digits, StringValidator::MinLength(13), StringValidator::MaxLength(19))]
    pub test_card_number: String,

    #[format(CreditCardNumber)]
    #[validators(StringValidator::Digits)]
    pub optional_backup_card: Option<String>,
}

// ============================================================================
// EDGE CASE 9: Complex Nested Structures with Validators
// ============================================================================
#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct ValidatedAddress {
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(200))]
    pub street: String,

    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(100))]
    pub city: String,

    #[validators(StringValidator::Uppercased, StringValidator::MinLength(2), StringValidator::MaxLength(3))]
    pub state_code: String,

    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(20))]
    pub postal_code: String,

    #[validators(StringValidator::Uppercased, StringValidator::MinLength(2), StringValidator::MaxLength(2))]
    pub country_code: String,

    #[validators(NumberValidator::Between(-90.0, 90.0))]
    pub latitude: Option<f64>,

    #[validators(NumberValidator::Between(-180.0, 180.0))]
    pub longitude: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct ValidatedContact {
    #[format(FullName)]
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(100))]
    pub name: String,

    #[format(Email)]
    #[validators(StringValidator::Email)]
    pub email: String,

    #[format(PhoneNumber)]
    #[validators(StringValidator::MaxLength(20))]
    pub phone: Option<String>,

    #[validators(StringValidator::MaxLength(100))]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 5)]
pub struct DeepNestedValidation {
    pub id: String,

    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(200))]
    pub name: String,

    /// Nested struct with its own validators
    pub primary_address: ValidatedAddress,

    /// Optional nested struct
    pub billing_address: Option<ValidatedAddress>,

    /// Vec of nested validated structs
    pub shipping_addresses: Vec<ValidatedAddress>,

    /// Primary contact
    pub primary_contact: ValidatedContact,

    /// Optional secondary contact
    pub secondary_contact: Option<ValidatedContact>,

    /// Multiple contacts
    pub additional_contacts: Vec<ValidatedContact>,

    /// Numeric fields at top level
    #[validators(NumberValidator::NonNegative)]
    pub total_orders: u32,

    #[validators(NumberValidator::NonNegative)]
    pub total_spent: f64,

    /// Optional numeric
    #[validators(NumberValidator::Positive)]
    pub credit_limit: Option<f64>,
}

// ============================================================================
// EDGE CASE 10: Edge Relationships with Validators
// ============================================================================
#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 20)]
pub struct EdgeCaseUser {
    pub id: String,

    #[format(Email)]
    #[validators(StringValidator::Email, StringValidator::MaxLength(255))]
    pub email: String,

    #[validators(StringValidator::Alphanumeric, StringValidator::MinLength(3), StringValidator::MaxLength(30))]
    pub username: String,

    #[format(DateTime)]
    pub created_at: String,

    #[validators(NumberValidator::NonNegative)]
    pub login_count: u32,

    #[validators(NumberValidator::Between(0.0, 5.0))]
    pub rating: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 50)]
pub struct EdgeCasePost {
    pub id: String,

    #[edge(name = "post_author", from = "EdgeCasePost", to = "EdgeCaseUser", direction = "from")]
    pub author: RecordLink<EdgeCaseUser>,

    #[validators(StringValidator::NonEmpty, StringValidator::MinLength(5), StringValidator::MaxLength(200))]
    pub title: String,

    #[validators(StringValidator::NonEmpty, StringValidator::MinLength(10))]
    pub content: String,

    #[validators(StringValidator::Lowercased, StringValidator::MaxLength(100))]
    pub slug: String,

    #[format(DateTime)]
    pub created_at: String,

    #[format(DateTime)]
    pub updated_at: Option<String>,

    #[validators(NumberValidator::NonNegative)]
    pub view_count: u64,

    #[validators(NumberValidator::NonNegative)]
    pub like_count: u32,

    pub is_published: bool,

    #[format(Url("images.example.com"))]
    #[validators(StringValidator::Url)]
    pub featured_image: Option<String>,

    #[validators(StringValidator::MaxLength(300))]
    pub meta_description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 100)]
pub struct EdgeCaseComment {
    pub id: String,

    #[edge(name = "comment_post", from = "EdgeCaseComment", to = "EdgeCasePost", direction = "from")]
    pub post: RecordLink<EdgeCasePost>,

    #[edge(name = "comment_author", from = "EdgeCaseComment", to = "EdgeCaseUser", direction = "from")]
    pub author: RecordLink<EdgeCaseUser>,

    #[validators(StringValidator::NonEmpty, StringValidator::MinLength(1), StringValidator::MaxLength(10000))]
    pub content: String,

    #[format(DateTime)]
    pub created_at: String,

    #[format(DateTime)]
    pub edited_at: Option<String>,

    #[validators(NumberValidator::NonNegative)]
    pub like_count: u32,

    /// Self-referential for nested comments
    pub parent_comment_id: Option<String>,

    pub is_approved: bool,

    #[validators(StringValidator::Ip)]
    pub author_ip: Option<String>,
}

// ============================================================================
// EDGE CASE 11: Enums with Complex Validated Structs
// ============================================================================
#[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]
pub enum PaymentStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Refunded,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]
pub enum PaymentMethod {
    CreditCard,
    DebitCard,
    BankTransfer,
    PayPal,
    Crypto,
    Cash,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 30)]
pub struct ComplexPayment {
    pub id: String,

    #[edge(name = "payment_user", from = "ComplexPayment", to = "EdgeCaseUser", direction = "from")]
    pub user: RecordLink<EdgeCaseUser>,

    pub status: PaymentStatus,
    pub method: PaymentMethod,

    #[format(CurrencyAmount)]
    #[validators(NumberValidator::Positive, NumberValidator::LessThan(1000000.0))]
    pub amount: f64,

    #[validators(StringValidator::Uppercased, StringValidator::MinLength(3), StringValidator::MaxLength(3))]
    pub currency: String,

    #[format(CurrencyAmount)]
    #[validators(NumberValidator::NonNegative)]
    pub fee: f64,

    #[format(CurrencyAmount)]
    #[validators(NumberValidator::NonNegative)]
    pub tax: f64,

    #[format(CurrencyAmount)]
    #[validators(NumberValidator::Positive)]
    pub total: f64,

    // Transaction details
    #[format(HexString(32))]
    #[validators(StringValidator::Hex, StringValidator::MinLength(32))]
    pub transaction_id: String,

    #[format(HexString(16))]
    #[validators(StringValidator::Hex)]
    pub reference_code: Option<String>,

    // Timestamps
    #[format(DateTime)]
    pub created_at: String,

    #[format(DateTime)]
    pub processed_at: Option<String>,

    #[format(DateTime)]
    pub completed_at: Option<String>,

    // Billing address as nested struct
    pub billing_address: ValidatedAddress,

    // Optional notes
    #[validators(StringValidator::MaxLength(1000))]
    pub notes: Option<String>,

    // Metadata
    #[validators(NumberValidator::NonNegative)]
    pub retry_count: u8,

    #[validators(StringValidator::MaxLength(500))]
    pub failure_reason: Option<String>,
}

// ============================================================================
// EDGE CASE 12: Maximum Validator Stacking on Single Fields
// ============================================================================
#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 3)]
pub struct MaxValidatorStacking {
    pub id: String,

    /// String with maximum possible validators that make sense together
    #[validators(
        StringValidator::NonEmpty,
        StringValidator::Trimmed,
        StringValidator::MinLength(8),
        StringValidator::MaxLength(128),
        StringValidator::Alphanumeric
    )]
    pub mega_validated_string: String,

    /// Same but optional
    #[validators(
        StringValidator::NonEmpty,
        StringValidator::Trimmed,
        StringValidator::MinLength(8),
        StringValidator::MaxLength(128),
        StringValidator::Alphanumeric
    )]
    pub optional_mega_validated: Option<String>,

    /// Number with maximum stacking
    #[validators(
        NumberValidator::Positive,
        NumberValidator::GreaterThan(0.0),
        NumberValidator::GreaterThanOrEqualTo(1.0),
        NumberValidator::LessThan(1000.0),
        NumberValidator::LessThanOrEqualTo(999.0),
        NumberValidator::Between(1.0, 999.0)
    )]
    pub mega_validated_number: f64,

    /// Same but on u32
    #[validators(
        NumberValidator::Positive,
        NumberValidator::GreaterThanOrEqualTo(1.0),
        NumberValidator::LessThanOrEqualTo(100.0),
        NumberValidator::Between(1.0, 100.0)
    )]
    pub mega_validated_u32: u32,

    /// Optional mega validated u32
    #[validators(
        NumberValidator::Positive,
        NumberValidator::Between(1.0, 100.0)
    )]
    pub optional_mega_u32: Option<u32>,

    /// Mega validated i64
    #[validators(
        NumberValidator::GreaterThan(-1000000.0),
        NumberValidator::LessThan(1000000.0),
        NumberValidator::Between(-999999.0, 999999.0)
    )]
    pub mega_validated_i64: i64,

    #[validators(NumberValidator::Between(-1000.0, 1000.0))]
    pub optional_mega_i64: Option<i64>,
}

// ============================================================================
// EDGE CASE 13: Arrays/Vectors with Validators
// ============================================================================
#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 5)]
pub struct ArrayValidationExtremes {
    pub id: String,

    /// Vec of validated strings
    pub tags: Vec<String>,

    /// Vec with items that have their own validators (nested in the element type)
    pub validated_emails: Vec<String>,

    /// Optional vec
    pub optional_tags: Option<Vec<String>>,

    /// Vec of integers
    pub scores: Vec<u32>,

    /// Optional vec of integers
    pub optional_scores: Option<Vec<u32>>,

    /// Vec of floats
    pub measurements: Vec<f64>,

    /// Nested validated structs in vec
    pub addresses: Vec<ValidatedAddress>,

    /// Optional nested validated structs
    pub backup_addresses: Option<Vec<ValidatedAddress>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Kitchen Sink String tests
    #[test]
    fn test_kitchen_sink_string_required() {
        let item = KitchenSinkString {
            id: "test:1".to_string(),
            strict_username: "validuser123".to_string(),
            optional_strict_username: None,
            validated_email: "test@example.com".to_string(),
            optional_validated_email: None,
            api_endpoint: "https://api.example.com/v1".to_string(),
            optional_cdn_url: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("validuser123"));
    }

    #[test]
    fn test_kitchen_sink_string_all_optional_filled() {
        let item = KitchenSinkString {
            id: "test:1".to_string(),
            strict_username: "validuser123".to_string(),
            optional_strict_username: Some("optionaluser1".to_string()),
            validated_email: "test@example.com".to_string(),
            optional_validated_email: Some("optional@example.com".to_string()),
            api_endpoint: "https://api.example.com/v1".to_string(),
            optional_cdn_url: Some("https://cdn.example.com/file.jpg".to_string()),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("optionaluser1"));
        assert!(json.contains("optional@example.com"));
    }

    // Extreme Integer tests
    #[test]
    fn test_extreme_integer_all_types() {
        let item = ExtremeIntegerValidation {
            id: "test:1".to_string(),
            constrained_u8: 50,
            optional_constrained_u8: Some(25),
            bounded_u16: 1000,
            optional_bounded_u16: Some(500),
            percentage_u32: 75,
            optional_percentage_u32: Some(50),
            large_u64: 1000000,
            optional_large_u64: Some(500000),
            negative_i8: -50,
            optional_negative_i8: Some(-25),
            non_positive_i16: -100,
            optional_non_positive_i16: Some(0),
            bounded_i32: 500,
            optional_bounded_i32: Some(-250),
            constrained_i64: 1000000,
            optional_non_negative_i64: Some(0),
            bounded_usize: 5000,
            optional_bounded_usize: Some(2500),
            bounded_isize: 50,
            optional_positive_isize: Some(25),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"constrained_u8\":50"));
        assert!(json.contains("\"negative_i8\":-50"));
    }

    #[test]
    fn test_extreme_integer_all_none() {
        let item = ExtremeIntegerValidation {
            id: "test:1".to_string(),
            constrained_u8: 50,
            optional_constrained_u8: None,
            bounded_u16: 1000,
            optional_bounded_u16: None,
            percentage_u32: 75,
            optional_percentage_u32: None,
            large_u64: 1000000,
            optional_large_u64: None,
            negative_i8: -50,
            optional_negative_i8: None,
            non_positive_i16: -100,
            optional_non_positive_i16: None,
            bounded_i32: 500,
            optional_bounded_i32: None,
            constrained_i64: 1000000,
            optional_non_negative_i64: None,
            bounded_usize: 5000,
            optional_bounded_usize: None,
            bounded_isize: 50,
            optional_positive_isize: None,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("null"));
    }

    // Float validation tests
    #[test]
    fn test_extreme_float_coordinates() {
        let item = ExtremeFloatValidation {
            id: "test:1".to_string(),
            multi_constrained_f32: 500.0,
            optional_f32: Some(50.0),
            normalized_f64: 0.5,
            optional_normalized_f64: Some(0.75),
            negative_f64: -500.0,
            optional_negative_f64: Some(-100.0),
            currency_amount: 999.99,
            optional_currency: Some(50.0),
            percentage: 75.5,
            optional_percentage: Some(25.0),
            latitude: 40.7128,
            longitude: -74.0060,
            optional_latitude: Some(51.5074),
            optional_longitude: Some(-0.1278),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("40.7128"));
        assert!(json.contains("-74.006"));
    }

    // Complex identifiers tests
    #[test]
    fn test_complex_identifiers() {
        let item = ComplexIdentifiers {
            id: "test:1".to_string(),
            uuid_field: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            optional_uuid: Some("550e8400-e29b-41d4-a716-446655440001".to_string()),
            hex_id_32: "0123456789abcdef0123456789abcdef".to_string(),
            optional_hex_id: Some("0123456789abcdef".to_string()),
            base64_token: "dGVzdCB0b2tlbiB2YWx1ZQ==".to_string(),
            optional_base64_token: Some("b3B0aW9uYWw=".to_string()),
            version: "1.2.3".to_string(),
            optional_version: Some("2.0.0-beta".to_string()),
            sha256_hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
            optional_hash: Some("abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string()),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("550e8400"));
        assert!(json.contains("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"));
    }

    // Network types tests
    #[test]
    fn test_network_types() {
        let item = NetworkTypes {
            id: "test:1".to_string(),
            ip_address: "192.168.1.1".to_string(),
            optional_ip: Some("10.0.0.1".to_string()),
            mac_address: "00:1A:2B:3C:4D:5E".to_string(),
            optional_mac: Some("AA:BB:CC:DD:EE:FF".to_string()),
            internal_api_url: "https://api.internal.example.com/v1".to_string(),
            optional_storage_url: Some("https://storage.example.com/files".to_string()),
            user_agent: "Mozilla/5.0 (compatible; TestBot/1.0)".to_string(),
            optional_user_agent: Some("curl/7.68.0".to_string()),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("192.168.1.1"));
        assert!(json.contains("00:1A:2B:3C:4D:5E"));
    }

    // Date time tests
    #[test]
    fn test_datetime_extremes() {
        let item = DateTimeExtremes {
            id: "test:1".to_string(),
            created_at: "2024-01-15T10:30:00Z".to_string(),
            optional_updated_at: Some("2024-01-16T14:00:00Z".to_string()),
            birth_date: "1990-05-15".to_string(),
            optional_expiry_date: Some("2025-12-31".to_string()),
            start_time: "09:00:00".to_string(),
            optional_end_time: Some("17:00:00".to_string()),
            appointment: "2024-02-01T14:30:00Z".to_string(),
            optional_followup: Some("2024-02-15T10:00:00Z".to_string()),
            duration: "PT2H30M".to_string(),
            optional_duration: Some("P1D".to_string()),
            timezone: "America/New_York".to_string(),
            optional_timezone: Some("Europe/London".to_string()),
            deadline: "2024-02-28".to_string(),
            optional_target_date: Some("2024-06-30".to_string()),
        };
        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("2024-01-15"));
        assert!(json.contains("America/New_York"));
    }

    // Deep nested validation tests
    #[test]
    fn test_deep_nested_validation() {
        let address = ValidatedAddress {
            street: "123 Main St".to_string(),
            city: "New York".to_string(),
            state_code: "NY".to_string(),
            postal_code: "10001".to_string(),
            country_code: "US".to_string(),
            latitude: Some(40.7128),
            longitude: Some(-74.0060),
        };

        let contact = ValidatedContact {
            name: "John Doe".to_string(),
            email: "john@example.com".to_string(),
            phone: Some("+1-555-123-4567".to_string()),
            title: Some("CEO".to_string()),
        };

        let item = DeepNestedValidation {
            id: "test:1".to_string(),
            name: "Acme Corporation".to_string(),
            primary_address: address.clone(),
            billing_address: Some(address.clone()),
            shipping_addresses: vec![address.clone(), address.clone()],
            primary_contact: contact.clone(),
            secondary_contact: Some(contact.clone()),
            additional_contacts: vec![contact.clone()],
            total_orders: 150,
            total_spent: 50000.0,
            credit_limit: Some(100000.0),
        };

        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("Acme Corporation"));
        assert!(json.contains("123 Main St"));
        assert!(json.contains("John Doe"));
    }

    // Edge relationships test
    #[test]
    fn test_edge_case_post() {
        let post = EdgeCasePost {
            id: "post:1".to_string(),
            author: RecordLink::Id("user:1".to_string().into()),
            title: "Test Post Title".to_string(),
            content: "This is the test post content that is long enough.".to_string(),
            slug: "test-post-title".to_string(),
            created_at: "2024-01-15T10:30:00Z".to_string(),
            updated_at: Some("2024-01-16T14:00:00Z".to_string()),
            view_count: 1000,
            like_count: 50,
            is_published: true,
            featured_image: Some("https://images.example.com/post1.jpg".to_string()),
            meta_description: Some("A test post for validation".to_string()),
        };

        let json = serde_json::to_string(&post).unwrap();
        assert!(json.contains("Test Post Title"));
        assert!(json.contains("test-post-title"));
    }

    // Complex payment test
    #[test]
    fn test_complex_payment() {
        let payment = ComplexPayment {
            id: "payment:1".to_string(),
            user: RecordLink::Id("user:1".to_string().into()),
            status: PaymentStatus::Completed,
            method: PaymentMethod::CreditCard,
            amount: 99.99,
            currency: "USD".to_string(),
            fee: 2.99,
            tax: 8.00,
            total: 110.98,
            transaction_id: "0123456789abcdef0123456789abcdef".to_string(),
            reference_code: Some("abcdef1234567890".to_string()),
            created_at: "2024-01-15T10:30:00Z".to_string(),
            processed_at: Some("2024-01-15T10:30:05Z".to_string()),
            completed_at: Some("2024-01-15T10:30:10Z".to_string()),
            billing_address: ValidatedAddress {
                street: "456 Payment Ave".to_string(),
                city: "Chicago".to_string(),
                state_code: "IL".to_string(),
                postal_code: "60601".to_string(),
                country_code: "US".to_string(),
                latitude: None,
                longitude: None,
            },
            notes: Some("Test payment".to_string()),
            retry_count: 0,
            failure_reason: None,
        };

        let json = serde_json::to_string(&payment).unwrap();
        assert!(json.contains("99.99"));
        assert!(json.contains("USD"));
        assert!(json.contains("Completed"));
    }

    // Max validator stacking test
    #[test]
    fn test_max_validator_stacking() {
        let item = MaxValidatorStacking {
            id: "test:1".to_string(),
            mega_validated_string: "validstring123".to_string(),
            optional_mega_validated: Some("optional12345".to_string()),
            mega_validated_number: 500.0,
            mega_validated_u32: 50,
            optional_mega_u32: Some(75),
            mega_validated_i64: 500000,
            optional_mega_i64: Some(-500),
        };

        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("validstring123"));
        assert!(json.contains("500000"));
    }

    // Array validation test
    #[test]
    fn test_array_validation_extremes() {
        let item = ArrayValidationExtremes {
            id: "test:1".to_string(),
            tags: vec!["rust".to_string(), "programming".to_string()],
            validated_emails: vec!["a@b.com".to_string(), "c@d.com".to_string()],
            optional_tags: Some(vec!["optional".to_string()]),
            scores: vec![100, 95, 88],
            optional_scores: Some(vec![50, 60]),
            measurements: vec![1.5, 2.7, 3.5],
            addresses: vec![ValidatedAddress {
                street: "789 Array St".to_string(),
                city: "Boston".to_string(),
                state_code: "MA".to_string(),
                postal_code: "02101".to_string(),
                country_code: "US".to_string(),
                latitude: None,
                longitude: None,
            }],
            backup_addresses: None,
        };

        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("rust"));
        assert!(json.contains("789 Array St"));
    }
}

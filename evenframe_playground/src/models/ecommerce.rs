use super::auth::User;
use evenframe::types::RecordLink;
use evenframe::Evenframe;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]
pub enum OrderStatus {
    Pending,
    Processing,
    Shipped,
    Delivered,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]
pub enum ProductCategory {
    Electronics,
    Clothing,
    Books,
    Home,
    Sports,
    Other,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 100)]
pub struct Product {
    pub id: String,

    /// Product name - non-empty, max 200 chars
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(200))]
    pub name: String,

    /// Product description - max 5000 chars
    #[validators(StringValidator::MaxLength(5000))]
    pub description: String,

    /// Price - must be positive
    #[validators(NumberValidator::Positive)]
    pub price: f64,

    /// Stock quantity - non-negative
    /// BUG: NumberValidator::NonNegative compares with 0.0 (f64) but field is u32
    #[validators(NumberValidator::NonNegative)]
    pub stock_quantity: u32,

    pub category: ProductCategory,

    /// Image URL - valid URL
    /// BUG: StringValidator::Url on Option<String> doesn't unwrap the Option
    #[format(Url("example.com"))]
    #[validators(StringValidator::Url)]
    pub image_url: Option<String>,

    pub is_available: bool,

    #[format(DateTime)]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Address {
    /// Street address - non-empty, max 200 chars
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(200))]
    pub street: String,

    /// City - non-empty, max 100 chars
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(100))]
    pub city: String,

    /// State/Province - max 100 chars
    #[validators(StringValidator::MaxLength(100))]
    pub state: String,

    /// Postal/ZIP code - non-empty, max 20 chars
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(20))]
    pub postal_code: String,

    /// Country code - 2-char ISO code
    #[validators(StringValidator::NonEmpty, StringValidator::MinLength(2), StringValidator::MaxLength(2), StringValidator::Uppercased)]
    pub country: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]
#[mock_data(n = 50)]
pub struct Customer {
    pub id: String,

    #[edge(name = "customer_user", from = "Customer", to = "User", direction = "from")]
    pub user: RecordLink<User>,

    pub shipping_address: Option<Address>,

    pub billing_address: Option<Address>,

    #[format(PhoneNumber)]
    pub phone: Option<String>,

    #[format(DateTime)]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct CartItem {
    /// Product ID reference - non-empty
    #[validators(StringValidator::NonEmpty)]
    pub product_id: String,

    /// Product name - non-empty, max 200 chars
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(200))]
    pub product_name: String,

    /// Quantity - must be positive (at least 1)
    /// BUG: NumberValidator::Positive compares with 0.0 (f64) but field is u32
    #[validators(NumberValidator::Positive)]
    pub quantity: u32,

    /// Unit price - must be positive
    #[validators(NumberValidator::Positive)]
    pub unit_price: f64,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 200)]
pub struct Order {
    pub id: String,

    #[edge(name = "order_customer", from = "Order", to = "Customer", direction = "from")]
    pub customer: RecordLink<Customer>,

    /// Order items
    pub items: Vec<CartItem>,

    /// Subtotal - non-negative
    #[validators(NumberValidator::NonNegative)]
    pub subtotal: f64,

    /// Tax - non-negative
    #[validators(NumberValidator::NonNegative)]
    pub tax: f64,

    /// Shipping cost - non-negative
    #[validators(NumberValidator::NonNegative)]
    pub shipping_cost: f64,

    /// Total - must be positive
    #[validators(NumberValidator::Positive)]
    pub total: f64,

    pub status: OrderStatus,

    pub shipping_address: Address,

    /// Order notes - max 1000 chars
    /// BUG: StringValidator on Option<String> doesn't unwrap the Option
    #[validators(StringValidator::MaxLength(1000))]
    pub notes: Option<String>,

    #[format(DateTime)]
    pub created_at: String,

    #[format(DateTime)]
    pub shipped_at: Option<String>,

    #[format(DateTime)]
    pub delivered_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_status_serialization() {
        let statuses = vec![
            (OrderStatus::Pending, "Pending"),
            (OrderStatus::Processing, "Processing"),
            (OrderStatus::Shipped, "Shipped"),
            (OrderStatus::Delivered, "Delivered"),
            (OrderStatus::Cancelled, "Cancelled"),
        ];

        for (status, expected) in statuses {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, format!("\"{}\"", expected));
        }
    }

    #[test]
    fn test_order_status_deserialization() {
        let json = "\"Processing\"";
        let status: OrderStatus = serde_json::from_str(json).unwrap();
        assert!(matches!(status, OrderStatus::Processing));
    }

    #[test]
    fn test_product_category_serialization() {
        let categories = vec![
            (ProductCategory::Electronics, "Electronics"),
            (ProductCategory::Clothing, "Clothing"),
            (ProductCategory::Books, "Books"),
            (ProductCategory::Home, "Home"),
            (ProductCategory::Sports, "Sports"),
            (ProductCategory::Other, "Other"),
        ];

        for (category, expected) in categories {
            let json = serde_json::to_string(&category).unwrap();
            assert_eq!(json, format!("\"{}\"", expected));
        }
    }

    #[test]
    fn test_product_serialization() {
        let product = Product {
            id: "product:1".to_string(),
            name: "Test Product".to_string(),
            description: "A test product".to_string(),
            price: 99.99,
            stock_quantity: 50,
            category: ProductCategory::Electronics,
            image_url: Some("https://example.com/img.png".to_string()),
            is_available: true,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&product).unwrap();
        assert!(json.contains("\"name\":\"Test Product\""));
        assert!(json.contains("\"price\":99.99"));
    }

    #[test]
    fn test_product_deserialization() {
        let json = r#"{
            "id": "product:1",
            "name": "Laptop",
            "description": "A powerful laptop",
            "price": 1299.99,
            "stock_quantity": 10,
            "category": "Electronics",
            "image_url": null,
            "is_available": true,
            "created_at": "2024-01-01T00:00:00Z"
        }"#;

        let product: Product = serde_json::from_str(json).unwrap();
        assert_eq!(product.id, "product:1");
        assert_eq!(product.name, "Laptop");
        assert_eq!(product.price, 1299.99);
        assert!(matches!(product.category, ProductCategory::Electronics));
    }

    #[test]
    fn test_address_serialization() {
        let address = Address {
            street: "123 Main St".to_string(),
            city: "Springfield".to_string(),
            state: "IL".to_string(),
            postal_code: "62701".to_string(),
            country: "US".to_string(),
        };

        let json = serde_json::to_string(&address).unwrap();
        assert!(json.contains("\"street\":\"123 Main St\""));
        assert!(json.contains("\"city\":\"Springfield\""));
    }

    #[test]
    fn test_address_deserialization() {
        let json = r#"{
            "street": "456 Oak Ave",
            "city": "Chicago",
            "state": "IL",
            "postal_code": "60601",
            "country": "US"
        }"#;

        let address: Address = serde_json::from_str(json).unwrap();
        assert_eq!(address.street, "456 Oak Ave");
        assert_eq!(address.city, "Chicago");
        assert_eq!(address.country, "US");
    }

    #[test]
    fn test_customer_serialization() {
        let customer = Customer {
            id: "customer:1".to_string(),
            user: RecordLink::Id("user:1".to_string().into()),
            shipping_address: Some(Address {
                street: "123 Main St".to_string(),
                city: "Springfield".to_string(),
                state: "IL".to_string(),
                postal_code: "62701".to_string(),
                country: "US".to_string(),
            }),
            billing_address: None,
            phone: Some("+1234567890".to_string()),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&customer).unwrap();
        assert!(json.contains("\"phone\":\"+1234567890\""));
    }

    #[test]
    fn test_customer_deserialization() {
        let json = r#"{
            "id": "customer:1",
            "user": "user:1",
            "shipping_address": null,
            "billing_address": null,
            "phone": null,
            "created_at": "2024-01-01T00:00:00Z"
        }"#;

        let customer: Customer = serde_json::from_str(json).unwrap();
        assert_eq!(customer.id, "customer:1");
        assert!(customer.shipping_address.is_none());
        assert!(customer.phone.is_none());
    }

    #[test]
    fn test_cart_item_serialization() {
        let item = CartItem {
            product_id: "product:1".to_string(),
            product_name: "Laptop".to_string(),
            quantity: 2,
            unit_price: 999.99,
        };

        let json = serde_json::to_string(&item).unwrap();
        assert!(json.contains("\"product_name\":\"Laptop\""));
        assert!(json.contains("\"quantity\":2"));
    }

    #[test]
    fn test_cart_item_deserialization() {
        let json = r#"{
            "product_id": "product:1",
            "product_name": "Phone",
            "quantity": 1,
            "unit_price": 799.99
        }"#;

        let item: CartItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.product_id, "product:1");
        assert_eq!(item.quantity, 1);
        assert_eq!(item.unit_price, 799.99);
    }

    #[test]
    fn test_order_serialization() {
        let order = Order {
            id: "order:1".to_string(),
            customer: RecordLink::Id("customer:1".to_string().into()),
            items: vec![CartItem {
                product_id: "product:1".to_string(),
                product_name: "Laptop".to_string(),
                quantity: 1,
                unit_price: 999.99,
            }],
            subtotal: 999.99,
            tax: 99.99,
            shipping_cost: 10.00,
            total: 1109.98,
            status: OrderStatus::Pending,
            shipping_address: Address {
                street: "123 Main St".to_string(),
                city: "Springfield".to_string(),
                state: "IL".to_string(),
                postal_code: "62701".to_string(),
                country: "US".to_string(),
            },
            notes: Some("Please leave at door".to_string()),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            shipped_at: None,
            delivered_at: None,
        };

        let json = serde_json::to_string(&order).unwrap();
        assert!(json.contains("\"total\":1109.98"));
        assert!(json.contains("\"status\":\"Pending\""));
    }

    #[test]
    fn test_order_deserialization() {
        let json = r#"{
            "id": "order:1",
            "customer": "customer:1",
            "items": [],
            "subtotal": 100.00,
            "tax": 10.00,
            "shipping_cost": 5.00,
            "total": 115.00,
            "status": "Shipped",
            "shipping_address": {
                "street": "123 Main St",
                "city": "Springfield",
                "state": "IL",
                "postal_code": "62701",
                "country": "US"
            },
            "notes": null,
            "created_at": "2024-01-01T00:00:00Z",
            "shipped_at": "2024-01-02T00:00:00Z",
            "delivered_at": null
        }"#;

        let order: Order = serde_json::from_str(json).unwrap();
        assert_eq!(order.id, "order:1");
        assert_eq!(order.total, 115.00);
        assert!(matches!(order.status, OrderStatus::Shipped));
        assert!(order.shipped_at.is_some());
        assert!(order.delivered_at.is_none());
    }

    #[test]
    fn test_order_with_multiple_items() {
        let order = Order {
            id: "order:1".to_string(),
            customer: RecordLink::Id("customer:1".to_string().into()),
            items: vec![
                CartItem {
                    product_id: "product:1".to_string(),
                    product_name: "Laptop".to_string(),
                    quantity: 1,
                    unit_price: 999.99,
                },
                CartItem {
                    product_id: "product:2".to_string(),
                    product_name: "Mouse".to_string(),
                    quantity: 2,
                    unit_price: 29.99,
                },
            ],
            subtotal: 1059.97,
            tax: 105.99,
            shipping_cost: 0.0,
            total: 1165.96,
            status: OrderStatus::Processing,
            shipping_address: Address {
                street: "123 Main St".to_string(),
                city: "Springfield".to_string(),
                state: "IL".to_string(),
                postal_code: "62701".to_string(),
                country: "US".to_string(),
            },
            notes: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            shipped_at: None,
            delivered_at: None,
        };

        assert_eq!(order.items.len(), 2);
    }

    #[test]
    fn test_delivered_order() {
        let order = Order {
            id: "order:1".to_string(),
            customer: RecordLink::Id("customer:1".to_string().into()),
            items: vec![],
            subtotal: 100.0,
            tax: 10.0,
            shipping_cost: 5.0,
            total: 115.0,
            status: OrderStatus::Delivered,
            shipping_address: Address {
                street: "123 Main St".to_string(),
                city: "Springfield".to_string(),
                state: "IL".to_string(),
                postal_code: "62701".to_string(),
                country: "US".to_string(),
            },
            notes: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            shipped_at: Some("2024-01-02T00:00:00Z".to_string()),
            delivered_at: Some("2024-01-03T00:00:00Z".to_string()),
        };

        assert!(matches!(order.status, OrderStatus::Delivered));
        assert!(order.shipped_at.is_some());
        assert!(order.delivered_at.is_some());
    }

    #[test]
    fn test_product_clone() {
        let product = Product {
            id: "product:1".to_string(),
            name: "Test".to_string(),
            description: "Desc".to_string(),
            price: 99.99,
            stock_quantity: 10,
            category: ProductCategory::Electronics,
            image_url: None,
            is_available: true,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let cloned = product.clone();
        assert_eq!(product.id, cloned.id);
        assert_eq!(product.price, cloned.price);
    }

    #[test]
    fn test_address_clone() {
        let address = Address {
            street: "123 Main St".to_string(),
            city: "Springfield".to_string(),
            state: "IL".to_string(),
            postal_code: "62701".to_string(),
            country: "US".to_string(),
        };

        let cloned = address.clone();
        assert_eq!(address.street, cloned.street);
        assert_eq!(address.city, cloned.city);
    }
}

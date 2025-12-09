use evenframe::types::RecordLink;
use evenframe::Evenframe;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]
pub enum Role {
    Admin,
    Moderator,
    User,
    Guest,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 50)]
pub struct User {
    pub id: String,

    /// Email address - validated as email format, min 5 chars, max 255 chars
    #[format(Email)]
    #[validators(StringValidator::Email, StringValidator::MinLength(5), StringValidator::MaxLength(255))]
    pub email: String,

    /// Username - alphanumeric, min 3 chars, max 50 chars
    #[validators(StringValidator::Alphanumeric, StringValidator::MinLength(3), StringValidator::MaxLength(50))]
    pub username: String,

    /// Password hash - non-empty
    #[validators(StringValidator::NonEmpty)]
    pub password_hash: String,

    /// User roles
    pub roles: Vec<Role>,

    pub is_active: bool,

    #[format(DateTime)]
    pub created_at: String,

    #[format(DateTime)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 100)]
pub struct Session {
    pub id: String,

    #[edge(name = "session_user", from = "Session", to = "User", direction = "from")]
    pub user: RecordLink<User>,

    /// Session token - non-empty, min 32 chars for security
    #[validators(StringValidator::NonEmpty, StringValidator::MinLength(32))]
    pub token: String,

    #[format(DateTime)]
    pub expires_at: String,

    #[format(DateTime)]
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_serialization() {
        let role = Role::Admin;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"Admin\"");
    }

    #[test]
    fn test_role_deserialization() {
        let json = "\"Moderator\"";
        let role: Role = serde_json::from_str(json).unwrap();
        assert!(matches!(role, Role::Moderator));
    }

    #[test]
    fn test_all_role_variants_serialize() {
        let roles = vec![Role::Admin, Role::Moderator, Role::User, Role::Guest];
        let expected = vec!["Admin", "Moderator", "User", "Guest"];

        for (role, expected_str) in roles.into_iter().zip(expected.into_iter()) {
            let json = serde_json::to_string(&role).unwrap();
            assert_eq!(json, format!("\"{}\"", expected_str));
        }
    }

    #[test]
    fn test_user_serialization() {
        let user = User {
            id: "user:1".to_string(),
            email: "test@example.com".to_string(),
            username: "testuser".to_string(),
            password_hash: "hash123".to_string(),
            roles: vec![Role::User],
            is_active: true,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&user).unwrap();
        assert!(json.contains("\"email\":\"test@example.com\""));
        assert!(json.contains("\"username\":\"testuser\""));
        assert!(json.contains("\"is_active\":true"));
    }

    #[test]
    fn test_user_deserialization() {
        let json = r#"{
            "id": "user:1",
            "email": "test@example.com",
            "username": "testuser",
            "password_hash": "hash123",
            "roles": ["Admin", "User"],
            "is_active": true,
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        }"#;

        let user: User = serde_json::from_str(json).unwrap();
        assert_eq!(user.id, "user:1");
        assert_eq!(user.email, "test@example.com");
        assert_eq!(user.roles.len(), 2);
        assert!(user.is_active);
    }

    #[test]
    fn test_user_with_multiple_roles() {
        let user = User {
            id: "user:1".to_string(),
            email: "admin@example.com".to_string(),
            username: "admin".to_string(),
            password_hash: "hash".to_string(),
            roles: vec![Role::Admin, Role::Moderator, Role::User],
            is_active: true,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&user).unwrap();
        let deserialized: User = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.roles.len(), 3);
    }

    #[test]
    fn test_session_serialization() {
        let session = Session {
            id: "session:1".to_string(),
            user: RecordLink::Id("user:1".to_string().into()),
            token: "token123".to_string(),
            expires_at: "2024-12-31T23:59:59Z".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&session).unwrap();
        assert!(json.contains("\"token\":\"token123\""));
    }

    #[test]
    fn test_session_with_record_link() {
        // Token must be at least 32 characters per validator
        let json = r#"{
            "id": "session:1",
            "user": "user:1",
            "token": "abc123def456ghi789jkl012mno345pq",
            "expires_at": "2024-12-31T23:59:59Z",
            "created_at": "2024-01-01T00:00:00Z"
        }"#;

        let session: Session = serde_json::from_str(json).unwrap();
        assert_eq!(session.id, "session:1");
        assert_eq!(session.token, "abc123def456ghi789jkl012mno345pq");
    }

    #[test]
    fn test_user_clone() {
        let user = User {
            id: "user:1".to_string(),
            email: "test@example.com".to_string(),
            username: "testuser".to_string(),
            password_hash: "hash".to_string(),
            roles: vec![Role::User],
            is_active: true,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let cloned = user.clone();
        assert_eq!(user.id, cloned.id);
        assert_eq!(user.email, cloned.email);
    }
}

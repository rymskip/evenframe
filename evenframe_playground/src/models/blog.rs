use super::auth::User;
use evenframe::types::RecordLink;
use evenframe::Evenframe;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 20)]
pub struct Tag {
    pub id: String,

    /// Tag name - non-empty, max 100 chars
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(100))]
    pub name: String,

    /// URL-safe slug - lowercase, max 100 chars
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(100), StringValidator::Lowercased)]
    pub slug: String,

    /// Optional description - max 500 chars
    /// BUG: StringValidator on Option<String> doesn't unwrap the Option
    #[validators(StringValidator::MaxLength(500))]
    pub description: Option<String>,

    /// Post count - non-negative
    /// BUG: NumberValidator::NonNegative compares with 0.0 (f64) but field is u32
    #[validators(NumberValidator::NonNegative)]
    pub post_count: u32,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 10)]
pub struct Author {
    pub id: String,

    #[edge(name = "author_user", from = "Author", to = "User", direction = "from")]
    pub user: RecordLink<User>,

    /// Author bio - max 2000 chars
    /// BUG: StringValidator on Option<String> doesn't unwrap the Option
    #[validators(StringValidator::MaxLength(2000))]
    pub bio: Option<String>,

    /// Avatar URL - valid URL format
    /// BUG: StringValidator::Url on Option<String> doesn't unwrap the Option
    #[format(Url("example.com"))]
    #[validators(StringValidator::Url)]
    pub avatar_url: Option<String>,

    /// Twitter handle - starts with @, max 16 chars
    /// BUG: StringValidator on Option<String> doesn't unwrap the Option
    #[validators(StringValidator::StartsWith("@"), StringValidator::MaxLength(16))]
    pub twitter_handle: Option<String>,

    /// GitHub handle - max 39 chars
    /// BUG: StringValidator on Option<String> doesn't unwrap the Option
    #[validators(StringValidator::MaxLength(39))]
    pub github_handle: Option<String>,

    #[format(DateTime)]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 50)]
pub struct Post {
    pub id: String,

    /// Post title - non-empty, max 200 chars
    #[validators(StringValidator::NonEmpty, StringValidator::MinLength(1), StringValidator::MaxLength(200))]
    pub title: String,

    /// URL-safe slug - lowercase, max 250 chars
    #[validators(StringValidator::NonEmpty, StringValidator::MaxLength(250), StringValidator::Lowercased)]
    pub slug: String,

    /// Post content - non-empty
    #[validators(StringValidator::NonEmpty)]
    pub content: String,

    /// Optional excerpt - max 500 chars
    /// BUG: StringValidator on Option<String> doesn't unwrap the Option
    #[validators(StringValidator::MaxLength(500))]
    pub excerpt: Option<String>,

    #[edge(name = "post_author", from = "Post", to = "Author", direction = "from")]
    pub author: RecordLink<Author>,

    /// Tags
    #[edge(name = "post_tags", from = "Post", to = "Tag", direction = "from")]
    pub tags: Vec<RecordLink<Tag>>,

    /// Featured image URL - valid URL
    /// BUG: StringValidator::Url on Option<String> doesn't unwrap the Option
    #[format(Url("example.com"))]
    #[validators(StringValidator::Url)]
    pub featured_image: Option<String>,

    pub published: bool,

    #[format(DateTime)]
    pub published_at: Option<String>,

    /// View count - non-negative
    /// BUG: NumberValidator::NonNegative compares with 0.0 (f64) but field is u32
    #[validators(NumberValidator::NonNegative)]
    pub view_count: u32,

    #[format(DateTime)]
    pub created_at: String,

    #[format(DateTime)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 200)]
pub struct Comment {
    pub id: String,

    #[edge(name = "comment_post", from = "Comment", to = "Post", direction = "from")]
    pub post: RecordLink<Post>,

    #[edge(name = "comment_author", from = "Comment", to = "Author", direction = "from")]
    pub author: RecordLink<Author>,

    /// Comment content - non-empty, max 5000 chars
    #[validators(StringValidator::NonEmpty, StringValidator::MinLength(1), StringValidator::MaxLength(5000))]
    pub content: String,

    pub parent_comment_id: Option<String>,

    pub is_approved: bool,

    #[format(DateTime)]
    pub created_at: String,

    #[format(DateTime)]
    pub edited_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_serialization() {
        let tag = Tag {
            id: "tag:1".to_string(),
            name: "Rust".to_string(),
            slug: "rust".to_string(),
            description: Some("Rust programming language".to_string()),
            post_count: 10,
        };

        let json = serde_json::to_string(&tag).unwrap();
        assert!(json.contains("\"name\":\"Rust\""));
        assert!(json.contains("\"slug\":\"rust\""));
    }

    #[test]
    fn test_tag_deserialization() {
        let json = r#"{
            "id": "tag:1",
            "name": "Rust",
            "slug": "rust",
            "description": null,
            "post_count": 5
        }"#;

        let tag: Tag = serde_json::from_str(json).unwrap();
        assert_eq!(tag.id, "tag:1");
        assert_eq!(tag.name, "Rust");
        assert!(tag.description.is_none());
        assert_eq!(tag.post_count, 5);
    }

    #[test]
    fn test_tag_with_optional_description() {
        let tag_with_desc = Tag {
            id: "tag:1".to_string(),
            name: "Test".to_string(),
            slug: "test".to_string(),
            description: Some("Description".to_string()),
            post_count: 0,
        };

        let tag_without_desc = Tag {
            id: "tag:2".to_string(),
            name: "Other".to_string(),
            slug: "other".to_string(),
            description: None,
            post_count: 0,
        };

        assert!(tag_with_desc.description.is_some());
        assert!(tag_without_desc.description.is_none());
    }

    #[test]
    fn test_author_serialization() {
        let author = Author {
            id: "author:1".to_string(),
            user: RecordLink::Id("user:1".to_string().into()),
            bio: Some("A developer".to_string()),
            avatar_url: Some("https://example.com/avatar.png".to_string()),
            twitter_handle: Some("@dev".to_string()),
            github_handle: Some("dev".to_string()),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&author).unwrap();
        assert!(json.contains("\"bio\":\"A developer\""));
    }

    #[test]
    fn test_author_deserialization() {
        let json = r#"{
            "id": "author:1",
            "user": "user:1",
            "bio": "Writer and developer",
            "avatar_url": null,
            "twitter_handle": "@writer",
            "github_handle": null,
            "created_at": "2024-01-01T00:00:00Z"
        }"#;

        let author: Author = serde_json::from_str(json).unwrap();
        assert_eq!(author.id, "author:1");
        assert!(author.bio.is_some());
        assert!(author.avatar_url.is_none());
        assert!(author.twitter_handle.is_some());
        assert!(author.github_handle.is_none());
    }

    #[test]
    fn test_post_serialization() {
        let post = Post {
            id: "post:1".to_string(),
            title: "Test Post".to_string(),
            slug: "test-post".to_string(),
            content: "Post content".to_string(),
            excerpt: Some("Brief excerpt".to_string()),
            author: RecordLink::Id("author:1".to_string().into()),
            tags: vec![RecordLink::Id("tag:1".to_string().into())],
            featured_image: None,
            published: true,
            published_at: Some("2024-01-01T00:00:00Z".to_string()),
            view_count: 100,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&post).unwrap();
        assert!(json.contains("\"title\":\"Test Post\""));
        assert!(json.contains("\"published\":true"));
    }

    #[test]
    fn test_post_deserialization() {
        let json = r#"{
            "id": "post:1",
            "title": "Test",
            "slug": "test",
            "content": "Content",
            "excerpt": null,
            "author": "author:1",
            "tags": ["tag:1", "tag:2"],
            "featured_image": null,
            "published": false,
            "published_at": null,
            "view_count": 0,
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        }"#;

        let post: Post = serde_json::from_str(json).unwrap();
        assert_eq!(post.id, "post:1");
        assert!(!post.published);
        assert!(post.published_at.is_none());
        assert_eq!(post.tags.len(), 2);
    }

    #[test]
    fn test_post_with_multiple_tags() {
        let post = Post {
            id: "post:1".to_string(),
            title: "Multi-tag Post".to_string(),
            slug: "multi-tag".to_string(),
            content: "Content".to_string(),
            excerpt: None,
            author: RecordLink::Id("author:1".to_string().into()),
            tags: vec![
                RecordLink::Id("tag:1".to_string().into()),
                RecordLink::Id("tag:2".to_string().into()),
                RecordLink::Id("tag:3".to_string().into()),
            ],
            featured_image: None,
            published: true,
            published_at: Some("2024-01-01T00:00:00Z".to_string()),
            view_count: 50,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        assert_eq!(post.tags.len(), 3);
    }

    #[test]
    fn test_comment_serialization() {
        let comment = Comment {
            id: "comment:1".to_string(),
            post: RecordLink::Id("post:1".to_string().into()),
            author: RecordLink::Id("author:1".to_string().into()),
            content: "Great post!".to_string(),
            parent_comment_id: None,
            is_approved: true,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            edited_at: None,
        };

        let json = serde_json::to_string(&comment).unwrap();
        assert!(json.contains("\"content\":\"Great post!\""));
        assert!(json.contains("\"is_approved\":true"));
    }

    #[test]
    fn test_comment_deserialization() {
        let json = r#"{
            "id": "comment:1",
            "post": "post:1",
            "author": "author:1",
            "content": "Nice article",
            "parent_comment_id": null,
            "is_approved": false,
            "created_at": "2024-01-01T00:00:00Z",
            "edited_at": null
        }"#;

        let comment: Comment = serde_json::from_str(json).unwrap();
        assert_eq!(comment.id, "comment:1");
        assert!(!comment.is_approved);
        assert!(comment.parent_comment_id.is_none());
    }

    #[test]
    fn test_nested_comment() {
        let reply = Comment {
            id: "comment:2".to_string(),
            post: RecordLink::Id("post:1".to_string().into()),
            author: RecordLink::Id("author:2".to_string().into()),
            content: "Reply to parent".to_string(),
            parent_comment_id: Some("comment:1".to_string()),
            is_approved: true,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            edited_at: None,
        };

        assert!(reply.parent_comment_id.is_some());
        assert_eq!(reply.parent_comment_id.unwrap(), "comment:1");
    }

    #[test]
    fn test_edited_comment() {
        let comment = Comment {
            id: "comment:1".to_string(),
            post: RecordLink::Id("post:1".to_string().into()),
            author: RecordLink::Id("author:1".to_string().into()),
            content: "Edited content".to_string(),
            parent_comment_id: None,
            is_approved: true,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            edited_at: Some("2024-01-02T00:00:00Z".to_string()),
        };

        assert!(comment.edited_at.is_some());
    }

    #[test]
    fn test_tag_clone() {
        let tag = Tag {
            id: "tag:1".to_string(),
            name: "Test".to_string(),
            slug: "test".to_string(),
            description: Some("Desc".to_string()),
            post_count: 5,
        };

        let cloned = tag.clone();
        assert_eq!(tag.id, cloned.id);
        assert_eq!(tag.name, cloned.name);
    }
}

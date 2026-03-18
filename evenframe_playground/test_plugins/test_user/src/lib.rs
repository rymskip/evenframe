//! Dealdraft e2e test user plugin.
//!
//! Record 0 gets fixed test credentials, all others fall back to evenframe defaults.

use evenframe_plugin::define_field_plugin;

define_field_plugin!(|ctx: &FieldContext| {
    if ctx.record_index != 0 {
        return None;
    }
    match ctx.field_name.as_str() {
        "email" => Some("'test@example.com'"),
        "password" => Some("'TestPassword123!'"),
        "first_name" => Some("'Test'"),
        "last_name" => Some("'User'"),
        "role" => Some("'Administrator'"),
        "email_verified" => Some("true"),
        "dnd_enabled" => Some("false"),
        "bio" => Some("'E2E test user account'"),
        "avatar_url" | "status_message" | "status_emoji" | "status_expires_at" | "dnd_start"
        | "dnd_end" | "verification_token" | "verification_expires"
        | "password_reset_token" | "password_reset_expires" => Some("NONE"),
        _ => None,
    }
});

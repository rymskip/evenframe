//! A table whose JSON and database names differ from its Rust names.

use evenframe::Evenframe;
use serde::{Deserialize, Serialize};
use surrealdb_types::SurrealValue;

#[derive(Debug, Clone, Serialize, SurrealValue, Evenframe)]
#[serde(rename_all = "camelCase")]
#[surreal(crate = "surrealdb_types", rename_all = "camelCase")]
#[mock_data(n = 20)]
pub struct Membership {
    pub id: String,

    #[validators(StringValidator::MinLength(2))]
    pub display_name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,

    /// Held to tenths, which plain float `%` gets wrong for most of them.
    #[validators(
        NumberValidator::GreaterThanOrEqualTo(0.0),
        NumberValidator::MultipleOf(0.1)
    )]
    pub score_weight: f64,

    pub tier: MembershipTier,
}

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue, Evenframe)]
#[serde(rename_all = "kebab-case")]
#[surreal(crate = "surrealdb_types", rename_all = "kebab-case")]
pub enum MembershipTier {
    Free,
    PaidMonthly,
}

#[cfg(test)]
mod tests {
    use super::{Membership, MembershipTier};
    use evenframe::validator::validate::Validate;

    #[test]
    fn serde_names_are_read_and_written() {
        let membership: Membership = serde_json::from_value(serde_json::json!({
            "id": "membership:1",
            "displayName": "Ada",
            "scoreWeight": 0.3,
            "tier": "paid-monthly",
        }))
        .unwrap();
        assert_eq!(membership.nickname, None);
        assert!(matches!(membership.tier, MembershipTier::PaidMonthly));
        let written = serde_json::to_value(&membership).unwrap();
        assert_eq!(written["displayName"], "Ada");
        assert!(written.get("nickname").is_none());
    }

    #[test]
    fn every_failing_field_is_reported() {
        let error = serde_json::from_value::<Membership>(serde_json::json!({
            "id": "membership:1",
            "displayName": "A",
            "scoreWeight": 0.35,
            "tier": "free",
        }))
        .unwrap_err();
        assert!(
            error.to_string().starts_with(
                "displayName: must be at least 2 characters; scoreWeight: must be a multiple of 0.1"
            ),
            "{error}"
        );
        let membership = Membership {
            id: "membership:2".to_owned(),
            display_name: "Bo".to_owned(),
            nickname: None,
            score_weight: 0.3,
            tier: MembershipTier::Free,
        };
        assert!(membership.validate().is_ok());
    }
}

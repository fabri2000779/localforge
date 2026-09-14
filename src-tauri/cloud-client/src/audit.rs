//! Audit-log producer (POST /v1/audit). Callers decide whether to await or fire-and-forget.

use serde::Serialize;

use crate::api::{self, ApiError};

#[derive(Debug, Serialize)]
struct AuditPayload<'a> {
    action: &'a str,
    target: Option<&'a str>,
    metadata: Option<serde_json::Value>,
    /// Active org the action targeted, so a sub-user's entry lands in the owner's feed.
    #[serde(rename = "organizationId", skip_serializing_if = "Option::is_none")]
    organization_id: Option<String>,
}

/// POST one audit entry. Unknown actions are dropped server-side, not rejected.
pub async fn emit(
    action: &str,
    target: Option<&str>,
    metadata: Option<serde_json::Value>,
    token: &str,
) -> Result<(), ApiError> {
    let _: serde_json::Value = api::post(
        "/v1/audit",
        &AuditPayload {
            action,
            target,
            metadata,
            organization_id: api::active_org(),
        },
        Some(token),
    )
    .await?;
    Ok(())
}

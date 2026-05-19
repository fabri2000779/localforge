//! Audit log producer — pure HTTP slice.
//!
//! POST /v1/audit. One call per user-initiated action — start, stop,
//! restart, send console command, file write, etc. Free users still
//! call this; the cloud drops their entries server-side.
//!
//! Caller controls whether to await or fire-and-forget. We don't
//! spawn here because spawning is a platform decision — desktop has
//! `tauri::async_runtime::spawn`, mobile has the same, but a CLI /
//! test consumer might want sequential behaviour.

use serde::Serialize;

use crate::api::{self, ApiError};

#[derive(Debug, Serialize)]
struct AuditPayload<'a> {
    action: &'a str,
    target: Option<&'a str>,
    metadata: Option<serde_json::Value>,
}

/// POST a single audit-log entry. `action` is a stable enum the cloud
/// recognises (server.start, server.stop, etc.) — anything else is
/// dropped server-side rather than rejected, so this call rarely
/// errors on a well-formed token.
///
/// The cloud picks the org from the caller's primary membership for
/// now; a future API revision will accept an explicit `organization_id`
/// in the body when we surface the org switcher in the mobile UI.
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
        },
        Some(token),
    )
    .await?;
    Ok(())
}

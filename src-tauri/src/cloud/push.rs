//! Desktop → cloud crash-push trigger.
//!
//! When a locally-hosted server changes crash-state, the desktop tells the
//! cloud so it can fan out an ID-only push to the org's members' phones (handy
//! for teammates who don't have this desktop open). The active org rides along
//! as the `X-LocalForge-Org` header injected by the api layer, so we only send
//! the opaque serverId + kind. Delivery is credential-gated + best-effort
//! server-side; this call is fire-and-forget and must never disrupt the
//! crash/restart flow.

use super::{api, auth};

fn unauth() -> api::ApiError {
    api::ApiError::Server {
        status: 401,
        code: "unauthenticated".into(),
        message: None,
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NotifyBody<'a> {
    server_id: &'a str,
    kind: &'a str,
}

/// Report a crash-state change (`kind` = "crashed" | "restarted" | "backoff")
/// so the cloud can push the org's members. Returns `Ok(())` even on a
/// best-effort no-op; only auth/transport errors surface.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_push_notify(server_id: String, kind: String) -> Result<(), api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    let _: serde_json::Value = api::post(
        "/v1/push/notify",
        &NotifyBody {
            server_id: &server_id,
            kind: &kind,
        },
        Some(&token),
    )
    .await?;
    Ok(())
}

//! Crash-state push trigger: tells the cloud so it can notify the org's members' phones. Best-effort.

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

/// Report a crash-state change (`kind` = crashed | restarted | backoff).
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

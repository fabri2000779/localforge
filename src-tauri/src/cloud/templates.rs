//! Desktop community-template gallery commands.
//!
//! Thin adapters over the cloud `/v1/templates` endpoints (publish / browse /
//! fetch / unpublish), reading the bearer token from the OS keychain. A
//! "template" is a Custom Game definition (the JSON `export_game` produces);
//! importing one feeds it straight into `import_game`.

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
struct PublishBody<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    game_label: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<&'a str>,
    config: &'a str,
}

#[derive(serde::Deserialize)]
struct PublishResp {
    id: String,
}

/// Publish a Custom Game definition to the community gallery. `config` is the
/// JSON from `export_game`. Returns the new template id.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_template_publish(
    name: String,
    game_label: Option<String>,
    description: Option<String>,
    config: String,
) -> Result<String, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    let r: PublishResp = api::post(
        "/v1/templates",
        &PublishBody {
            name: &name,
            game_label: game_label.as_deref(),
            description: description.as_deref(),
            config: &config,
        },
        Some(&token),
    )
    .await?;
    Ok(r.id)
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSummary {
    pub id: String,
    pub publisher_name: String,
    pub name: String,
    #[serde(default)]
    pub game_label: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub downloads: i64,
    pub created_at: i64,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateList {
    pub templates: Vec<TemplateSummary>,
    #[serde(default)]
    pub next_before: Option<i64>,
    /// Rowid tiebreaker for the composite cursor — pass back alongside
    /// `next_before` so a template sharing the boundary millisecond isn't
    /// skipped (audit finding; mirrors the audit feed's cursor).
    #[serde(default)]
    pub next_before_id: Option<i64>,
}

/// Browse the gallery (newest first, cursor-paged by the composite
/// `(before, beforeId)` cursor).
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_templates_list(
    before: Option<i64>,
    before_id: Option<i64>,
    limit: Option<u32>,
) -> Result<TemplateList, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    let mut path = format!("/v1/templates?limit={}", limit.unwrap_or(40));
    if let Some(b) = before {
        path.push_str(&format!("&before={b}"));
    }
    if let Some(bid) = before_id {
        path.push_str(&format!("&beforeId={bid}"));
    }
    api::get(&path, Some(&token)).await
}

#[derive(serde::Deserialize)]
struct ConfigResp {
    config: String,
}

/// Fetch a template's GameConfig JSON (the caller then `import_game`s it).
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_template_get(id: String) -> Result<String, api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    let r: ConfigResp = api::get(&format!("/v1/templates/{id}"), Some(&token)).await?;
    Ok(r.config)
}

/// Unpublish one of your own templates.
#[tauri::command(rename_all = "camelCase")]
pub async fn cloud_template_delete(id: String) -> Result<(), api::ApiError> {
    let token = auth::current_token().ok_or_else(unauth)?;
    let _: serde_json::Value = api::delete(&format!("/v1/templates/{id}"), Some(&token)).await?;
    Ok(())
}

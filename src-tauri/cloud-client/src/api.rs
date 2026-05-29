//! Thin reqwest wrapper around `api.localforge.gg`. Single shared
//! client per process so we get connection pooling.
//!
//! Moved unchanged from the desktop's `src/cloud/api.rs` as part of
//! the cloud-client extraction (stage 1). The only edit is the path
//! that imports `api_origin` / `user_agent` — those now live at the
//! crate root instead of in a sibling module.

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::{OnceLock, RwLock};

use crate::{api_origin, user_agent};

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// The org every subsequent request should act on. Sent as the
/// `X-LocalForge-Org` header so a sub-user's calls (sync, machine listing)
/// target the OWNER's org instead of the caller's primary one. `None` →
/// header omitted → the server falls back to the caller's primary org
/// (the historical single-org behaviour).
static ACTIVE_ORG: OnceLock<RwLock<Option<String>>> = OnceLock::new();

fn active_org_cell() -> &'static RwLock<Option<String>> {
    ACTIVE_ORG.get_or_init(|| RwLock::new(None))
}

/// Point the API client at a specific org. The host app calls this when the
/// user switches the active org (and clears it on sign-out). Empty strings
/// are treated as `None`.
pub fn set_active_org(org_id: Option<String>) {
    // Recover from a poisoned lock instead of silently dropping the write: this
    // value decides which org every subsequent request targets, so a no-op'd
    // update would leave calls pointed at the previous (wrong) org.
    let mut g = active_org_cell().write().unwrap_or_else(|e| e.into_inner());
    *g = org_id.filter(|s| !s.trim().is_empty());
}

fn active_org() -> Option<String> {
    active_org_cell()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// The shared reqwest client. Public so callers that need a custom
/// HTTP method (PUT, DELETE) or response handling beyond what `get()`
/// / `post()` provide can build their own request. Stage 2 of the
/// refactor will add proper helpers for those cases so this exposure
/// can shrink back to crate-private.
pub fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(user_agent())
            .timeout(std::time::Duration::from_secs(30))
            // Hand reqwest an explicit rustls config so it does NOT fall
            // back to its default verifier (rustls-platform-verifier),
            // which panics on Android without JNI init. See
            // `webpki_tls_config` below.
            .use_preconfigured_tls(webpki_tls_config())
            .build()
            .expect("reqwest client build")
    })
}

/// A rustls client config that trusts the bundled Mozilla webpki root
/// store and nothing else.
///
/// reqwest 0.13's rustls backend, left to its defaults, builds a
/// `rustls_platform_verifier::Verifier`. On Android that verifier
/// aborts the process ("Expect rustls-platform-verifier to be
/// initialized") unless the app wires the JNI context + a companion
/// Kotlin class into it at startup. Rather than carry that brittle
/// per-platform plumbing, we pin verification to the compiled-in
/// Mozilla roots — identical behaviour on every OS, no init required.
/// `api.localforge.gg` (Cloudflare) chains to a public CA in the
/// bundle. This mirrors `localforge-backend-remote::build_tls_config`.
fn webpki_tls_config() -> rustls::ClientConfig {
    // reqwest is built with `rustls-no-provider`, so no process-default
    // CryptoProvider is installed automatically and
    // `ClientConfig::builder()` would panic without one. Install ring
    // here; it's idempotent, so it's harmless if the host app (mobile
    // `lib.rs`, or backend-remote) already installed it.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth()
}

/// Shape of an error response from the cloud API.
/// Most endpoints return `{ "error": "<code>", "message"?: "<detail>" }`.
///
/// Public for the same reason `client()` is — call sites that hand-
/// roll their HTTP request need a way to parse the error body into
/// the same shape `get()` / `post()` produce. Refactor stage 2 will
/// hide this again once `put()` / `delete()` helpers exist.
#[derive(Debug, serde::Deserialize)]
pub struct ApiErrorBody {
    pub error: String,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("decode error: {0}")]
    Decode(String),
    /// Server returned a non-2xx. `status` is the HTTP code, `code` is
    /// the machine-readable `error` field from the JSON body, `message`
    /// is the optional human detail.
    #[error("{code} (HTTP {status})")]
    Server {
        status: u16,
        code: String,
        message: Option<String>,
    },
}

/// Serialize for tauri::command return values — the frontend gets a
/// `{ code, status, message }` object on rejection.
///
/// Note: this `Serialize` impl is independent of Tauri (it's just
/// serde) but exists specifically so consuming apps can return
/// `Result<_, ApiError>` from their `#[tauri::command]` handlers
/// without any wrapping. Mobile reuses the exact same contract.
impl serde::Serialize for ApiError {
    fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut m = ser.serialize_map(Some(3))?;
        match self {
            ApiError::Server {
                status,
                code,
                message,
            } => {
                m.serialize_entry("status", status)?;
                m.serialize_entry("code", code)?;
                m.serialize_entry("message", message)?;
            }
            ApiError::Network(e) => {
                m.serialize_entry("status", &0u16)?;
                m.serialize_entry("code", "network")?;
                m.serialize_entry("message", &Some(e.to_string()))?;
            }
            ApiError::Decode(s) => {
                m.serialize_entry("status", &0u16)?;
                m.serialize_entry("code", "decode")?;
                m.serialize_entry("message", &Some(s.clone()))?;
            }
        }
        m.end()
    }
}

pub async fn post<B: Serialize, R: DeserializeOwned>(
    path: &str,
    body: &B,
    bearer: Option<&str>,
) -> Result<R, ApiError> {
    request(reqwest::Method::POST, path, Some(body), bearer).await
}

pub async fn put<B: Serialize, R: DeserializeOwned>(
    path: &str,
    body: &B,
    bearer: Option<&str>,
) -> Result<R, ApiError> {
    request(reqwest::Method::PUT, path, Some(body), bearer).await
}

pub async fn get<R: DeserializeOwned>(path: &str, bearer: Option<&str>) -> Result<R, ApiError> {
    request::<(), R>(reqwest::Method::GET, path, None, bearer).await
}

pub async fn delete<R: DeserializeOwned>(path: &str, bearer: Option<&str>) -> Result<R, ApiError> {
    request::<(), R>(reqwest::Method::DELETE, path, None, bearer).await
}

async fn request<B: Serialize, R: DeserializeOwned>(
    method: reqwest::Method,
    path: &str,
    body: Option<&B>,
    bearer: Option<&str>,
) -> Result<R, ApiError> {
    let url = format!("{}{}", api_origin(), path);
    let mut req = client().request(method, &url);
    if let Some(t) = bearer {
        req = req.bearer_auth(t);
    }
    // Act on the user's chosen active org (sub-user → the owner's org). The
    // server verifies membership before honouring it; omitted → primary org.
    if let Some(org) = active_org() {
        req = req.header("x-localforge-org", org);
    }
    if let Some(b) = body {
        req = req.json(b);
    }
    let res = req.send().await?;
    let status = res.status();
    if status.is_success() {
        if status == reqwest::StatusCode::NO_CONTENT {
            // R must be `()` here in practice; if it isn't this just
            // fails at deserialization, which is the right signal.
            return serde_json::from_value(serde_json::Value::Null)
                .map_err(|e| ApiError::Decode(e.to_string()));
        }
        res.json::<R>()
            .await
            .map_err(|e| ApiError::Decode(e.to_string()))
    } else {
        let code_num = status.as_u16();
        let body = res.json::<ApiErrorBody>().await.ok();
        Err(ApiError::Server {
            status: code_num,
            code: body
                .as_ref()
                .map(|b| b.error.clone())
                .unwrap_or_else(|| format!("http_{}", code_num)),
            message: body.and_then(|b| b.message),
        })
    }
}

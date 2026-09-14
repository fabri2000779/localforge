//! Thin reqwest wrapper around the cloud API; one shared client per process.

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::{OnceLock, RwLock};

use crate::{api_origin, user_agent};

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Org every request acts on, sent as `X-LocalForge-Org`; `None` = the caller's primary org.
static ACTIVE_ORG: OnceLock<RwLock<Option<String>>> = OnceLock::new();

fn active_org_cell() -> &'static RwLock<Option<String>> {
    ACTIVE_ORG.get_or_init(|| RwLock::new(None))
}

/// Pin the client to an org (cleared on sign-out). Empty strings mean `None`.
pub fn set_active_org(org_id: Option<String>) {
    // Recover from a poisoned lock: a dropped write would leave requests aimed at the wrong org.
    let mut g = active_org_cell().write().unwrap_or_else(|e| e.into_inner());
    *g = org_id.filter(|s| !s.trim().is_empty());
}

/// The pinned org; producers such as the audit log stamp it into request bodies too.
pub fn active_org() -> Option<String> {
    active_org_cell()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Shared reqwest client; public for callers that need PUT/DELETE or raw responses.
pub fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(user_agent())
            .timeout(std::time::Duration::from_secs(30))
            // Explicit rustls config: reqwest's default platform verifier aborts on Android without JNI init.
            .use_preconfigured_tls(webpki_tls_config())
            .build()
            .expect("reqwest client build")
    })
}

/// rustls config trusting only the bundled Mozilla roots: identical on every OS and needs no
/// platform init. Mirrors `localforge-backend-remote::build_tls_config`.
fn webpki_tls_config() -> rustls::ClientConfig {
    // Built with `rustls-no-provider`: install ring (idempotent) before touching ClientConfig.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth()
}

/// Error body shape: `{ "error": "<code>", "message"?: "<detail>" }`.
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
    /// Non-2xx response: HTTP status, machine-readable `error` code, optional detail.
    #[error("{code} (HTTP {status})")]
    Server {
        status: u16,
        code: String,
        message: Option<String>,
    },
}

/// Serialized as `{ status, code, message }` so commands can return `Result<_, ApiError>` directly.
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
    // Active-org header; the server verifies membership before honouring it.
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

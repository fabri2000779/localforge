//! Thin reqwest wrapper around `api.localforge.gg`. Single shared client
//! per process so we get connection pooling.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::OnceLock;

use super::{api_origin, user_agent};

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub(crate) fn client() -> &'static reqwest::Client {
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(user_agent())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("reqwest client build")
    })
}

/// Shape of an error response from the cloud API.
/// Most endpoints return `{ "error": "<code>", "message"?: "<detail>" }`.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct ApiErrorBody {
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

impl ApiError {
    /// Machine-readable error code. Useful for callers that want to
    /// pattern-match on `code` without destructuring the full enum.
    #[allow(dead_code)]
    pub fn code(&self) -> &str {
        match self {
            ApiError::Server { code, .. } => code,
            ApiError::Network(_) => "network",
            ApiError::Decode(_) => "decode",
        }
    }
}

/// Serialize for tauri::command return values — the frontend gets a
/// `{ code, status, message }` object on rejection.
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

pub async fn get<R: DeserializeOwned>(path: &str, bearer: Option<&str>) -> Result<R, ApiError> {
    request::<(), R>(reqwest::Method::GET, path, None, bearer).await
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
    if let Some(b) = body {
        req = req.json(b);
    }
    let res = req.send().await?;
    let status = res.status();
    if status.is_success() {
        if status == reqwest::StatusCode::NO_CONTENT {
            // R must be `()` here in practice; if it isn't this just fails
            // at deserialization, which is the right signal.
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

//! Relay client utilities — WS URL building only, for now.
//!
//! The full relay implementation (connection state machine,
//! reconnect logic, event emission, command routing) is desktop-
//! resident for the moment because it's tightly coupled to Tauri's
//! event sink and `tauri::async_runtime::spawn`. Extracting the full
//! WebSocket loop into a portable shape needs a callback / event-sink
//! abstraction that survives both consumers' threading models — that
//! design lives in the next round of refactoring.
//!
//! What we DO extract now is the small but exact-replication-needed
//! bit: deriving the WebSocket origin from the HTTP api_origin. Any
//! drift here between desktop and mobile would mean they connect to
//! different relay endpoints, which would silently break sync without
//! any visible error. Putting it in shared code prevents that.

/// Strip the HTTP scheme from an api_origin so we can prepend `wss://`
/// at connect time. Defaults to `api.localforge.gg` if the origin
/// doesn't start with `https://` or `http://` — that's the prod host
/// and what tests / staging configs without a scheme should fall back
/// to.
///
/// Examples:
///   `https://api.localforge.gg`        → `api.localforge.gg`
///   `http://localhost:8787`            → `localhost:8787`
///   `https://api-staging.localforge.gg/` → `api-staging.localforge.gg/`
///   (trailing slash preserved — caller appends the WS path)
pub fn ws_host(api_origin: &str) -> String {
    api_origin
        .strip_prefix("https://")
        .or_else(|| api_origin.strip_prefix("http://"))
        .unwrap_or("api.localforge.gg")
        .to_string()
}

/// Build the full `wss://…/v1/relay` URL given an api_origin.
pub fn ws_url(api_origin: &str) -> String {
    format!("wss://{}/v1/relay", ws_host(api_origin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_https() {
        assert_eq!(ws_host("https://api.localforge.gg"), "api.localforge.gg");
    }

    #[test]
    fn strips_http_for_local_dev() {
        assert_eq!(ws_host("http://localhost:8787"), "localhost:8787");
    }

    #[test]
    fn falls_back_when_no_scheme() {
        assert_eq!(ws_host("api.localforge.gg"), "api.localforge.gg");
    }

    #[test]
    fn ws_url_default() {
        assert_eq!(
            ws_url("https://api.localforge.gg"),
            "wss://api.localforge.gg/v1/relay"
        );
    }
}

//! Relay WebSocket URL derivation, shared so desktop and mobile target the same endpoint.

/// Host part of the api origin (scheme stripped); falls back to the production host.
pub fn ws_host(api_origin: &str) -> String {
    api_origin
        .strip_prefix("https://")
        .or_else(|| api_origin.strip_prefix("http://"))
        .unwrap_or("api.localforge.gg")
        .to_string()
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
}

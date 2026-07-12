//! OAuth URL building + deep-link callback parsing.
//!
//! Platform-agnostic slice of the OAuth flow. Each consumer wires this
//! to its OS-specific browser-opening API (`tauri_plugin_opener` on
//! desktop, custom browser intent on mobile) and to its deep-link
//! receiver (single-instance + deep-link plugin on desktop, mobile
//! Universal Links / App Links / custom scheme).
//!
//! The desktop's `cloud/oauth.rs` consumes these to keep the wire
//! contract identical between desktop and mobile — the cloud API
//! doesn't have to know which client is calling it.

/// Validated set of OAuth providers we support. The cloud's
/// `/v1/auth/<provider>/start` route only accepts these.
const PROVIDERS: &[&str] = &["apple", "discord", "google", "github"];

/// Build the cloud-side OAuth-start URL for a given provider.
///
///   `api_origin`   — base URL of the cloud API.
///   `provider`     — one of `apple`, `discord`, `google`, `github`.
///                    Other values return `Err`.
///   `redirect_uri` — where the cloud should 302 the user after they
///                    sign in. Desktop uses `localforge://auth/callback`;
///                    mobile uses a platform-appropriate scheme or
///                    Universal Link.
///
/// Returns the full URL to open in the user's default browser. The
/// caller is responsible for opening it.
pub fn start_url(
    api_origin: &str,
    provider: &str,
    redirect_uri: &str,
) -> Result<String, OAuthError> {
    if !PROVIDERS.contains(&provider) {
        return Err(OAuthError::UnknownProvider(provider.to_string()));
    }
    Ok(format!(
        "{}/v1/auth/{}/start?redirect_to={}",
        api_origin,
        provider,
        urlencode(redirect_uri),
    ))
}

/// Extract the `?token=…` from an OAuth callback URL. Returns `None`
/// if the URL is malformed or the token parameter is missing.
///
/// Deliberately tolerant — the deep-link receiver feeds us anything
/// the OS hands us, including unrelated `localforge://invite?…` URLs.
/// Callers route by URL prefix before calling this.
pub fn parse_callback_token(url: &str) -> Option<String> {
    parse_query_param(url, "token")
}

/// Extract any single query parameter from a URL. Useful for the
/// invite-acceptance flow too (`localforge://invite?token=…`).
///
/// The query is truncated at `#` first: an invite deep link carries the handoff
/// secret in its fragment (`…?token=INV#k=SECRET`), and without the cut the
/// `token` value came back as `INV#k=SECRET` — a contaminated token the cloud
/// 404s, plus the secret leaking into the token field (audit finding). The
/// fragment is parsed separately by the caller's `parse_fragment_param`.
pub fn parse_query_param(url: &str, name: &str) -> Option<String> {
    let (_, after_q) = url.split_once('?')?;
    // Drop the fragment before splitting params.
    let q = after_q.split_once('#').map_or(after_q, |(before, _)| before);
    q.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == name).then(|| urldecode(v))
    })
}

#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    #[error("unknown provider: {0}")]
    UnknownProvider(String),
}

// ---------------------------------------------------------------------------
// Minimal URL utilities — pulling `urlencoding` for this would be
// overkill for the handful of fixed strings we encode/decode.
// ---------------------------------------------------------------------------

/// Percent-encode unreserved chars (RFC 3986 §2.3). Used to embed the
/// custom-scheme redirect URI into the `redirect_to=` query parameter.
pub fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", b));
            }
        }
    }
    out
}

/// Symmetric of urlencode. Used on the JWT we receive in the
/// callback — base64url tokens shouldn't carry percent-escapes, but
/// the API hands them through `URLSearchParams` which can encode `+`
/// and `=`, so we still decode defensively.
pub fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (
                (bytes[i + 1] as char).to_digit(16),
                (bytes[i + 2] as char).to_digit(16),
            ) {
                out.push(((h << 4) | l) as u8);
                i += 3;
                continue;
            }
        } else if b == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_provider_rejected() {
        assert!(start_url("https://api.localforge.gg", "twitch", "x").is_err());
    }

    #[test]
    fn token_extracted() {
        let url = "localforge://auth/callback?token=abc.def.ghi";
        assert_eq!(parse_callback_token(url), Some("abc.def.ghi".to_string()));
    }

    #[test]
    fn percent_encoded_token_decoded() {
        let url = "localforge://auth/callback?token=a%2Bb%3D";
        assert_eq!(parse_callback_token(url), Some("a+b=".to_string()));
    }

    #[test]
    fn missing_token_returns_none() {
        let url = "localforge://auth/callback";
        assert_eq!(parse_callback_token(url), None);
    }

    #[test]
    fn token_not_contaminated_by_fragment() {
        // An invite deep link carries the handoff secret in the #fragment; the
        // token must come back clean, not `INV123#k=SECRET` (audit finding).
        let url = "localforge://invite?token=INV123#k=c2VjcmV0";
        assert_eq!(parse_query_param(url, "token"), Some("INV123".to_string()));
        // A `k` in the QUERY (web-bridge form) is still readable; a `k` that
        // lives only in the fragment is not this function's job.
        assert_eq!(parse_query_param(url, "k"), None);
        assert_eq!(
            parse_query_param("localforge://invite?token=A&k=B#frag", "k"),
            Some("B".to_string()),
        );
    }

    #[test]
    fn redirect_uri_encoded() {
        let u = start_url(
            "https://api.localforge.gg",
            "google",
            "localforge://auth/callback",
        )
        .unwrap();
        // The colon and slashes in the custom-scheme URI need %-escapes
        // for OAuth providers to accept the redirect_to.
        assert!(u.contains("redirect_to=localforge%3A%2F%2Fauth%2Fcallback"));
    }
}

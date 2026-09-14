//! OAuth start-URL building and callback parsing shared by desktop and mobile.

/// Providers accepted by the cloud's `/v1/auth/<provider>/start`.
const PROVIDERS: &[&str] = &["apple", "discord", "google", "github"];

/// Build the cloud OAuth-start URL; the cloud 302s to `redirect_uri` with `?token=` appended.
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

/// The `token` query parameter of a callback URL, if present.
pub fn parse_callback_token(url: &str) -> Option<String> {
    parse_query_param(url, "token")
}

/// One query parameter of a URL. The fragment is cut first so an invite's `#k=` secret
/// never contaminates the token.
pub fn parse_query_param(url: &str, name: &str) -> Option<String> {
    let (_, after_q) = url.split_once('?')?;
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

/// Percent-encode everything but RFC 3986 unreserved characters.
fn urlencode(s: &str) -> String {
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

/// Percent-decode (`+` becomes a space); the cloud's URLSearchParams may escape `+`/`=` in tokens.
fn urldecode(s: &str) -> String {
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
        // The #fragment secret must not contaminate the token.
        let url = "localforge://invite?token=INV123#k=c2VjcmV0";
        assert_eq!(parse_query_param(url, "token"), Some("INV123".to_string()));
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
        // The custom-scheme URI must be %-escaped inside redirect_to.
        assert!(u.contains("redirect_to=localforge%3A%2F%2Fauth%2Fcallback"));
    }
}

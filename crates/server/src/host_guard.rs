//! Request admission at the final serving boundary, after all routes/static fallbacks exist.
use axum::{
    extract::Request,
    http::{uri::Authority, StatusCode},
    middleware::{self, Next},
    response::IntoResponse,
    Router,
};

fn allowed(authority: &str, port: u16) -> bool {
    let Ok(a) = authority.parse::<Authority>() else {
        return false;
    };
    let host = a.host();
    // Reject userinfo and malformed/empty ports even if the URI parser accepts them.
    !authority.contains('@')
        && !authority.ends_with(':')
        && (host.eq_ignore_ascii_case("localhost") || host == "127.0.0.1" || host == "[::1]")
        && if authority.len() == host.len() {
            port == 80
        } else {
            authority
                .get(host.len()..)
                .and_then(|s| s.strip_prefix(':'))
                .is_some_and(|p| {
                    !p.is_empty()
                        && p.bytes().all(|b| b.is_ascii_digit())
                        && p.parse::<u16>().ok() == Some(port)
                })
        }
}

/// Protect the complete router using the actual bound port (including bind port 0).
/// No forwarded headers or implicit remote/sidecar exceptions are trusted.
pub fn protect(app: Router, port: u16) -> Router {
    app.layer(middleware::from_fn(
        move |request: Request, next: Next| async move {
            let values: Vec<_> = request.headers().get_all("host").iter().collect();
            let host = match values.as_slice() {
                [h] => match h.to_str() {
                    Ok(h) => Some(h),
                    Err(_) => {
                        return (StatusCode::FORBIDDEN, "E_HTTP_HOST: Host 不被允许")
                            .into_response()
                    }
                },
                [] => None,
                _ => return (StatusCode::FORBIDDEN, "E_HTTP_HOST: Host 不被允许").into_response(),
            };
            let uri_authority = request.uri().authority().map(Authority::as_str);
            // HTTP/2 may supply only :authority; HTTP/1 still requires exactly one Host.
            let authority = match (host, uri_authority) {
                (Some(h), Some(u)) if !h.eq_ignore_ascii_case(u) => None,
                (Some(h), _) => Some(h),
                (None, Some(u)) if request.version() == axum::http::Version::HTTP_2 => Some(u),
                _ => None,
            };
            if !authority.is_some_and(|a| allowed(a, port)) {
                return (StatusCode::FORBIDDEN, "E_HTTP_HOST: Host 不被允许").into_response();
            }
            next.run(request).await
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authority_requires_exact_loopback_and_valid_port() {
        for a in [
            "localhost:8787",
            "LOCALHOST:8787",
            "127.0.0.1:8787",
            "[::1]:8787",
        ] {
            assert!(allowed(a, 8787), "{a}");
        }
        for a in [
            "localhost:999999",
            "localhost:",
            "localhost:abc",
            "localhost:0",
            "localhost.",
            "localhost.evil:8787",
            "user@localhost:8787",
            "[::1]:8788",
            "127.1:8787",
            "localhost:80,localhost:8787",
        ] {
            assert!(!allowed(a, 8787), "{a}");
        }
    }
}

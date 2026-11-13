//! Request admission at the final serving boundary, after all routes/static fallbacks exist.
use axum::{
    extract::Request,
    http::{uri::Authority, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
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

/// 请求尚未进入 handler、也尚无 task_id；只记录用于排查的最小摘要。
fn rejected(request: &Request) -> Response {
    let host = request
        .headers()
        .get("host")
        .map(|h| {
            String::from_utf8_lossy(h.as_bytes())
                .chars()
                .take(100)
                .collect::<String>()
        })
        .unwrap_or_default();
    tracing::warn!(
        event = "http.host_rejected",
        code = an_core::codes::HTTP_HOST,
        task_id = "",
        method = %request.method(),
        path = request.uri().path(),
        host = %host,
        "HTTP Host 请求已拒绝"
    );
    (
        StatusCode::FORBIDDEN,
        format!("{}: Host 不被允许", an_core::codes::HTTP_HOST),
    )
        .into_response()
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
                    Err(_) => return rejected(&request),
                },
                [] => None,
                _ => return rejected(&request),
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
                return rejected(&request);
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

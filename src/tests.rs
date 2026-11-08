use super::handle_request;
use http::{header::CONTENT_TYPE, Method, Request, StatusCode};
use worker::Result;

#[test]
fn get_health_returns_ok_json() -> Result<()> {
    let request = Request::builder()
        .method(Method::GET)
        .uri("https://example.com/health")
        .body(())?;

    let response = handle_request(request, None)?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE),
        Some(&"application/json".parse()?)
    );
    assert_eq!(response.body(), r#"{"status":"ok"}"#);
    assert!(!response.headers().contains_key("X-SettleBot-Commit"));
    Ok(())
}

#[test]
fn unknown_paths_return_not_found() -> Result<()> {
    for path in ["/", "/missing", "/health/", "/HEALTH"] {
        assert_not_found(Method::GET, path)?;
    }
    Ok(())
}

#[test]
fn unsupported_health_methods_return_not_found() -> Result<()> {
    for method in [
        Method::POST,
        Method::PUT,
        Method::PATCH,
        Method::DELETE,
        Method::HEAD,
        Method::OPTIONS,
        Method::TRACE,
        Method::CONNECT,
    ] {
        assert_not_found(method, "/health")?;
    }
    Ok(())
}

fn assert_not_found(method: Method, path: &str) -> Result<()> {
    let request = Request::builder().method(method).uri(path).body(())?;

    assert_eq!(
        handle_request(request, None)?.status(),
        StatusCode::NOT_FOUND
    );
    Ok(())
}

#[test]
fn deployment_health_identifies_the_release_without_caching() -> Result<()> {
    let sha = "1234567890abcdef1234567890abcdef12345678";
    let request = Request::builder().uri("/health").body(())?;
    let response = handle_request(request, Some(sha))?;
    assert_eq!(response.headers()["X-SettleBot-Commit"], sha);
    assert_eq!(response.headers()["Cache-Control"], "no-store");
    assert_eq!(response.body(), r#"{"status":"ok"}"#);
    Ok(())
}

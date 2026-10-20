use super::handle_request;
use http::{header::CONTENT_TYPE, Method, Request, StatusCode};
use worker::Result;

#[test]
fn get_health_returns_ok_json() -> Result<()> {
    let request = Request::builder()
        .method(Method::GET)
        .uri("https://example.com/health")
        .body(())?;

    let response = handle_request(request)?;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(CONTENT_TYPE),
        Some(&"application/json".parse()?)
    );
    assert_eq!(response.body(), r#"{"status":"ok"}"#);
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

    assert_eq!(handle_request(request)?.status(), StatusCode::NOT_FOUND);
    Ok(())
}

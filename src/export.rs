use http::{
    header::{AUTHORIZATION, CACHE_CONTROL, CONTENT_TYPE},
    Response, StatusCode,
};
use worker::{Env, HttpRequest};

use crate::persistence::{
    d1::{D1Repository, DATABASE_BINDING},
    ExportRepository, ExportRequest, PersistenceError,
};

pub(crate) const EXPORT_PATH: &str = "/admin/export";
const EXPORT_SECRET: &str = "EXPORT_SECRET";
const BEARER_PREFIX: &str = "Bearer ";
const MINIMUM_SECRET_BYTES: usize = 16;

#[derive(Debug)]
enum Error {
    Forbidden,
    Invalid,
    Unavailable,
}

pub(crate) async fn response(
    request: HttpRequest,
    environment: Env,
) -> worker::Result<Response<String>> {
    let result = process(&request, &environment).await;
    let (status, body) = match result {
        Ok(body) => (StatusCode::OK, body),
        Err(Error::Forbidden) => (StatusCode::FORBIDDEN, error_body("Forbidden")),
        Err(Error::Invalid) => (StatusCode::BAD_REQUEST, error_body("Invalid cursor")),
        Err(Error::Unavailable) => (StatusCode::SERVICE_UNAVAILABLE, error_body("Unavailable")),
    };
    Ok(Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "application/json")
        .header(CACHE_CONTROL, "no-store")
        .header("x-content-type-options", "nosniff")
        .body(body)?)
}

async fn process(request: &HttpRequest, environment: &Env) -> Result<String, Error> {
    authorize(request, environment)?;
    let export_request = parse_request(request.uri().query())?;
    let database = environment
        .d1(DATABASE_BINDING)
        .map_err(|_| Error::Unavailable)?;
    D1Repository::new(database)
        .export_json(&export_request)
        .await
        .map_err(map_persistence_error)
}

fn authorize(request: &HttpRequest, environment: &Env) -> Result<(), Error> {
    let expected = environment
        .secret(EXPORT_SECRET)
        .map(|value| value.to_string())
        .map_err(|_| Error::Unavailable)?;
    if expected.len() < MINIMUM_SECRET_BYTES {
        return Err(Error::Unavailable);
    }
    let supplied = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix(BEARER_PREFIX))
        .ok_or(Error::Forbidden)?;
    if !constant_time_equal(expected.as_bytes(), supplied.as_bytes()) {
        return Err(Error::Forbidden);
    }
    Ok(())
}

fn parse_request(query: Option<&str>) -> Result<ExportRequest, Error> {
    let Some(query) = query.filter(|value| !value.is_empty()) else {
        return Ok(ExportRequest::first());
    };
    let cursor = query.strip_prefix("cursor=").ok_or(Error::Invalid)?;
    if cursor.is_empty() || cursor.contains('&') {
        return Err(Error::Invalid);
    }
    ExportRequest::resume(cursor).map_err(|_| Error::Invalid)
}

fn constant_time_equal(expected: &[u8], supplied: &[u8]) -> bool {
    if expected.len() != supplied.len() {
        return false;
    }
    expected
        .iter()
        .zip(supplied)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn map_persistence_error(error: PersistenceError) -> Error {
    match error {
        PersistenceError::InvalidExportCursor => Error::Invalid,
        _ => Error::Unavailable,
    }
}

fn error_body(message: &str) -> String {
    format!(r#"{{"error":"{message}"}}"#)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_queries_accept_only_the_cursor_parameter() {
        assert_eq!(parse_request(None).unwrap(), ExportRequest::first());
        assert!(parse_request(Some("cursor=1.2.10.20.1.2.3.4")).is_ok());
        for query in [
            "page=1",
            "cursor=",
            "cursor=1.2.3",
            "cursor=1.2.10.20.1.2.3.4&x=1",
        ] {
            assert!(matches!(parse_request(Some(query)), Err(Error::Invalid)));
        }
    }

    #[test]
    fn bearer_comparison_requires_exact_bytes() {
        assert!(constant_time_equal(b"secret", b"secret"));
        assert!(!constant_time_equal(b"secret", b"Secret"));
        assert!(!constant_time_equal(b"secret", b"secret-extra"));
    }
}

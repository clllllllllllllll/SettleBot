use http::{header::CONTENT_TYPE, Response, StatusCode};
use serde::Serialize;
use worker::Result;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

pub fn response(release_sha: Option<&str>) -> Result<Response<String>> {
    let body = serde_json::to_string(&HealthResponse { status: "ok" })?;

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/json")
        .header("Cache-Control", "no-store");
    if let Some(sha) = release_sha {
        response = response.header("X-SettleBot-Commit", sha);
    }
    Ok(response.body(body)?)
}

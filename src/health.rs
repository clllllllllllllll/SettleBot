use http::{header::CONTENT_TYPE, Response, StatusCode};
use serde::Serialize;
use worker::Result;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

pub fn response() -> Result<Response<String>> {
    let body = serde_json::to_string(&HealthResponse { status: "ok" })?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "application/json")
        .body(body)?)
}

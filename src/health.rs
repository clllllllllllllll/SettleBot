use serde::Serialize;
use worker::{Response, Result};

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
}

pub fn response() -> Result<Response> {
    Response::from_json(&HealthResponse { status: "ok" })
}

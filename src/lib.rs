mod health;

#[cfg(test)]
mod tests;

use http::{header::CONTENT_TYPE, Method, Request, Response, StatusCode};
use worker::{event, Context, Env, HttpRequest, Result};

const HEALTH_PATH: &str = "/health";

#[event(fetch)]
async fn fetch(
    request: HttpRequest,
    _environment: Env,
    _context: Context,
) -> Result<Response<String>> {
    handle_request(request)
}

fn handle_request<B>(request: Request<B>) -> Result<Response<String>> {
    if request.uri().path() == HEALTH_PATH && request.method() == Method::GET {
        return health::response();
    }

    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(CONTENT_TYPE, "text/plain; charset=utf-8")
        .body("Not found".to_owned())?)
}

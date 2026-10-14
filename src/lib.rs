mod health;

use worker::*;

const HEALTH_PATH: &str = "/health";

#[event(fetch)]
async fn fetch(request: Request, _environment: Env, _context: Context) -> Result<Response> {
    if request.path() == HEALTH_PATH && request.method() == Method::Get {
        return health::response();
    }

    Response::error("Not found", 404)
}

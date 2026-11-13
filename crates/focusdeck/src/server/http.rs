//! REST routes of `focusdeck server`: bearer auth, one JSON error shape and idempotent writes.

use std::collections::HashMap;
use std::io;
use std::sync::Arc;

use axum::Router;
use axum::body::{Bytes, to_bytes};
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use chrono::{DateTime, FixedOffset};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::board::{Board, BoardError};
use super::idempotency::{Claim, IdempotencyStore, body_hash};
use crate::api::{ActionRequest, ApiError, ApiErrorBody, CaptureRequest, DropRequest, ErrorCode};
use crate::i18n::{msg, t, t_args};

const BODY_LIMIT: usize = 1024 * 1024;
const MAX_KEY_LEN: usize = 255;

#[derive(Clone)]
struct App {
    board: Arc<Board>,
    keys: Arc<IdempotencyStore>,
    token_digest: [u8; 32],
}

pub fn router(board: Arc<Board>, keys: Arc<IdempotencyStore>, token: String) -> Router {
    let app = App {
        board,
        keys,
        token_digest: Sha256::digest(token.as_bytes()).into(),
    };
    Router::new()
        .route("/api/v1/profiles", get(profiles))
        .route("/api/v1/profiles/{p}", get(profile))
        .route("/api/v1/profiles/{p}/boxes/{b}/items", get(items))
        .route("/api/v1/profiles/{p}/boxes/{b}/actions", post(actions))
        .route("/api/v1/profiles/{p}/boxes/{b}/drops", post(drops))
        .route("/api/v1/profiles/{p}/captures", post(captures))
        .route("/api/v1/profiles/{p}/reminders", get(reminders))
        .fallback(unknown_route)
        .layer(middleware::from_fn_with_state(app.clone(), auth))
        .with_state(app)
}

fn now() -> DateTime<FixedOffset> {
    chrono::Local::now().fixed_offset()
}

fn json_response(status: StatusCode, body: &impl Serialize) -> Response {
    let bytes = serde_json::to_vec(body).expect("API bodies serialize");
    let mut response = Response::new(bytes.into());
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    response
}

fn api_error(code: ErrorCode, message: String) -> Response {
    let status = StatusCode::from_u16(code.status()).expect("error statuses are valid");
    json_response(
        status,
        &ApiError {
            error: ApiErrorBody { code, message },
        },
    )
}

type Rejection = Box<Response>;

fn reject(code: ErrorCode, message: String) -> Rejection {
    Box::new(api_error(code, message))
}

fn board_error(e: BoardError) -> Response {
    api_error(e.code(), e.message())
}

fn respond<T: Serialize>(result: Result<T, BoardError>) -> Response {
    match result {
        Ok(body) => json_response(StatusCode::OK, &body),
        Err(e) => board_error(e),
    }
}

// the closed error-code table has no server fault, so this one is spelled out here
fn internal(e: &io::Error) -> Response {
    eprintln!(
        "{}",
        t_args(
            msg::SERVER_SAVE_FAILED,
            &[("error", e.to_string().as_str().into())]
        )
    );
    json_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        &json!({"error": {"code": "INTERNAL", "message": t(msg::SERVER_INTERNAL)}}),
    )
}

fn digests_equal(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

async fn auth(State(app): State<App>, req: Request, next: Next) -> Response {
    let given = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let digest: [u8; 32] = Sha256::digest(given.as_bytes()).into();
    if !digests_equal(&digest, &app.token_digest) {
        return api_error(ErrorCode::Unauthorized, t(msg::SERVER_UNAUTHORIZED));
    }
    next.run(req).await
}

async fn unknown_route() -> Response {
    api_error(ErrorCode::NotFound, t(msg::BOARD_NOT_FOUND))
}

async fn read_body(req: Request) -> Result<Bytes, Rejection> {
    to_bytes(req.into_body(), BODY_LIMIT)
        .await
        .map_err(|_| reject(ErrorCode::Invalid, t(msg::SERVER_BODY_TOO_LARGE)))
}

fn parse<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Rejection> {
    serde_json::from_slice(bytes).map_err(|e| {
        reject(
            ErrorCode::Invalid,
            t_args(
                msg::SERVER_INVALID_BODY,
                &[("error", e.to_string().as_str().into())],
            ),
        )
    })
}

async fn read_json<T: DeserializeOwned>(req: Request) -> Result<(Bytes, T), Rejection> {
    let bytes = read_body(req).await?;
    let parsed = parse(&bytes)?;
    Ok((bytes, parsed))
}

async fn profiles(State(app): State<App>) -> Response {
    respond(Ok(app.board.profiles().await))
}

async fn profile(State(app): State<App>, Path(p): Path<String>) -> Response {
    respond(app.board.profile_view(&p).await)
}

async fn items(State(app): State<App>, Path((p, b)): Path<(String, String)>) -> Response {
    respond(app.board.items(&p, &b, now()).await)
}

async fn actions(
    State(app): State<App>,
    Path((p, b)): Path<(String, String)>,
    req: Request,
) -> Response {
    match read_json::<ActionRequest>(req).await {
        Ok((_, body)) => respond(app.board.action(&p, &b, body).await),
        Err(rejection) => *rejection,
    }
}

async fn drops(
    State(app): State<App>,
    Path((p, b)): Path<(String, String)>,
    req: Request,
) -> Response {
    let headers = req.headers().clone();
    match read_json::<DropRequest>(req).await {
        Ok((bytes, body)) => {
            idempotent(
                &app,
                &headers,
                &bytes,
                Board::drop(&app.board, &p, &b, body),
            )
            .await
        }
        Err(rejection) => *rejection,
    }
}

async fn captures(State(app): State<App>, Path(p): Path<String>, req: Request) -> Response {
    let headers = req.headers().clone();
    match read_json::<CaptureRequest>(req).await {
        Ok((bytes, body)) => idempotent(&app, &headers, &bytes, app.board.capture(&p, body)).await,
        Err(rejection) => *rejection,
    }
}

async fn reminders(
    State(app): State<App>,
    Path(p): Path<String>,
    Query(query): Query<HashMap<String, String>>,
) -> Response {
    let since = query
        .get("since")
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok());
    match since {
        Some(since) => respond(app.board.reminders(&p, since).await),
        None => api_error(ErrorCode::Invalid, t(msg::SERVER_BAD_SINCE)),
    }
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(io::Error::other)?
}

fn log_store_error(result: io::Result<()>) {
    if let Err(e) = result {
        eprintln!(
            "{}",
            t_args(
                msg::SERVER_SAVE_FAILED,
                &[("error", e.to_string().as_str().into())]
            )
        );
    }
}

/// Gives the claim back unless the plugin call finished, also when the handler is dropped or panics.
struct ReleaseGuard {
    keys: Arc<IdempotencyStore>,
    key: String,
    armed: bool,
}

impl ReleaseGuard {
    async fn release(&mut self) {
        self.armed = false;
        let (keys, key) = (Arc::clone(&self.keys), self.key.clone());
        log_store_error(blocking(move || keys.release(&key)).await);
    }
}

impl Drop for ReleaseGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let (keys, key) = (Arc::clone(&self.keys), self.key.clone());
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn_blocking(move || log_store_error(keys.release(&key)));
            }
            Err(_) => log_store_error(keys.release(&key)),
        }
    }
}

fn key_header(headers: &HeaderMap) -> Result<Option<String>, Rejection> {
    let Some(raw) = headers.get("idempotency-key") else {
        return Ok(None);
    };
    match raw.to_str() {
        Ok(key) if !key.is_empty() && key.len() <= MAX_KEY_LEN => Ok(Some(key.to_owned())),
        _ => Err(reject(ErrorCode::Invalid, t(msg::SERVER_BAD_KEY))),
    }
}

async fn idempotent<T, F>(app: &App, headers: &HeaderMap, body: &[u8], run: F) -> Response
where
    T: Serialize,
    F: Future<Output = Result<T, BoardError>>,
{
    let key = match key_header(headers) {
        Ok(Some(key)) => key,
        Ok(None) => return respond(run.await),
        Err(rejection) => return *rejection,
    };
    let claim = {
        let (keys, key, hash) = (Arc::clone(&app.keys), key.clone(), body_hash(body));
        blocking(move || keys.claim(&key, hash, now())).await
    };
    match claim {
        Ok(Claim::New) => {}
        Ok(Claim::Replay(stored)) => return json_response(StatusCode::OK, &stored),
        Ok(Claim::InProgress) => {
            return api_error(ErrorCode::InProgress, t(msg::SERVER_IN_PROGRESS));
        }
        Ok(Claim::Reused) => return api_error(ErrorCode::KeyReused, t(msg::SERVER_KEY_REUSED)),
        Err(e) => return internal(&e),
    }
    let mut guard = ReleaseGuard {
        keys: Arc::clone(&app.keys),
        key: key.clone(),
        armed: true,
    };
    let done = match run.await {
        Ok(done) => done,
        Err(e) => {
            guard.release().await;
            return board_error(e);
        }
    };
    // the plugin did the work: from here the key must stay claimed
    guard.armed = false;
    let stored: Value = match serde_json::to_value(&done) {
        Ok(stored) => stored,
        Err(e) => return internal(&io::Error::other(e)),
    };
    let (keys, saved) = (Arc::clone(&app.keys), stored.clone());
    log_store_error(blocking(move || keys.complete(&key, &saved)).await);
    json_response(StatusCode::OK, &stored)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Arc<IdempotencyStore>) {
        let dir = tempfile::tempdir().unwrap();
        let (keys, _) = IdempotencyStore::open(dir.path().join("idempotency.json"), now()).unwrap();
        (dir, Arc::new(keys))
    }

    fn guard(keys: &Arc<IdempotencyStore>, armed: bool) -> ReleaseGuard {
        ReleaseGuard {
            keys: Arc::clone(keys),
            key: "k".into(),
            armed,
        }
    }

    #[tokio::test]
    async fn dropping_an_armed_guard_releases_the_claim() {
        let (_dir, keys) = store();
        let hash = body_hash(b"a");
        assert_eq!(keys.claim("k", hash, now()).unwrap(), Claim::New);
        drop(guard(&keys, true));
        for _ in 0..100 {
            if keys.claim("k", hash, now()).unwrap() == Claim::New {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("the claim was never released");
    }

    #[tokio::test]
    async fn a_disarmed_guard_keeps_the_claim() {
        let (_dir, keys) = store();
        let hash = body_hash(b"a");
        assert_eq!(keys.claim("k", hash, now()).unwrap(), Claim::New);
        drop(guard(&keys, false));
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(keys.claim("k", hash, now()).unwrap(), Claim::InProgress);
    }
}

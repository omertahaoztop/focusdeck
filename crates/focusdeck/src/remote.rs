//! HTTP client of a `focusdeck server` and the box that stands in for a plugin process.

use std::error::Error as _;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, FixedOffset};
use focusdeck_protocol::{
    ActionParams, Describe, DropParams, ListResult, Reminder, RemindersParams, RemindersResult,
};
use reqwest::{StatusCode, Url};
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::api::{
    ActionRequest, ActionResponse, BoxView, CaptureRequest, CaptureResponse, DropRequest,
    DropResponse, ErrorCode, ItemsResponse, ProfileView, ProfilesResponse, RemindersResponse,
};
use crate::config::{Config, ServerConfig};
use crate::host::{HostError, clip};
use crate::i18n::{msg, t, t_args};
use crate::profile::{self, ProfileError};
use crate::secrets;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BODY_BYTES: usize = 16 * 1024 * 1024;

/// `Network` also carries every server-side fault (5xx, unknown code, non-JSON body) as text.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteError {
    Network(String),
    Unauthorized,
    Api { code: ErrorCode, message: String },
}

pub struct RemoteClient {
    http: reqwest::Client,
    base: Url,
    address: String,
    token: String,
}

// the token must stay out of any `{:?}` that ends up in a log or a panic message
impl std::fmt::Debug for RemoteClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteClient")
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct WireError {
    error: WireBody,
}

#[derive(Deserialize)]
struct WireBody {
    code: String,
    message: String,
}

/// The server address as shown to the user: no credentials, query or fragment.
fn public_address(url: &str) -> String {
    match Url::parse(url) {
        Ok(mut shown) => {
            let _ = shown.set_username("");
            let _ = shown.set_password(None);
            shown.set_query(None);
            shown.set_fragment(None);
            shown.as_str().trim_end_matches('/').to_owned()
        }
        Err(_) => url.to_owned(),
    }
}

fn explain_error(address: &str, e: &RemoteError) -> String {
    match e {
        RemoteError::Network(reason) => t_args(
            msg::REMOTE_FAILED,
            &[
                ("address", address.into()),
                ("error", reason.as_str().into()),
            ],
        ),
        RemoteError::Unauthorized => {
            t_args(msg::REMOTE_UNAUTHORIZED, &[("address", address.into())])
        }
        RemoteError::Api { message, .. } => message.clone(),
    }
}

/// Startup errors always name the server; the box red line keeps the plugin text as it came.
fn startup_text(address: &str, e: &RemoteError) -> String {
    match e {
        RemoteError::Api { message, .. } => t_args(
            msg::REMOTE_FAILED,
            &[
                ("address", address.into()),
                ("error", message.as_str().into()),
            ],
        ),
        other => explain_error(address, other),
    }
}

fn error_chain(e: &reqwest::Error) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

fn timeout_text() -> String {
    t_args(
        msg::REMOTE_TIMEOUT,
        &[(
            "seconds",
            REQUEST_TIMEOUT.as_secs().to_string().as_str().into(),
        )],
    )
}

fn transport(e: reqwest::Error) -> RemoteError {
    if e.is_timeout() {
        return RemoteError::Network(timeout_text());
    }
    RemoteError::Network(clip(&error_chain(&e.without_url())))
}

fn status_text(status: StatusCode, message: Option<&str>) -> String {
    let status = status.as_u16().to_string();
    match message {
        Some(message) => t_args(
            msg::REMOTE_STATUS_MESSAGE,
            &[
                ("status", status.as_str().into()),
                ("message", clip(message).as_str().into()),
            ],
        ),
        None => t_args(msg::REMOTE_STATUS, &[("status", status.as_str().into())]),
    }
}

fn classify(status: StatusCode, bytes: &[u8]) -> RemoteError {
    let wire = serde_json::from_slice::<WireError>(bytes)
        .ok()
        .map(|w| w.error);
    let code = wire
        .as_ref()
        .and_then(|w| serde_json::from_value::<ErrorCode>(Value::String(w.code.clone())).ok());
    if status == StatusCode::UNAUTHORIZED || code == Some(ErrorCode::Unauthorized) {
        return RemoteError::Unauthorized;
    }
    match (code, wire) {
        (Some(code), Some(w)) if !status.is_server_error() || code == ErrorCode::Plugin => {
            RemoteError::Api {
                code,
                message: w.message,
            }
        }
        (_, Some(w)) => RemoteError::Network(status_text(status, Some(&w.message))),
        _ => RemoteError::Network(status_text(status, None)),
    }
}

async fn read_capped(mut response: reqwest::Response) -> Result<Vec<u8>, RemoteError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(transport)? {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(RemoteError::Network(t(msg::REMOTE_TOO_LARGE)));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

impl RemoteClient {
    pub fn connect(cfg: &ServerConfig, home: &Path) -> Result<RemoteClient, RemoteError> {
        if !crate::install::https_or_loopback(&cfg.url) {
            return Err(RemoteError::Network(t_args(
                msg::ERROR_SERVER_URL_INSECURE,
                &[("url", cfg.url.as_str().into())],
            )));
        }
        let base = Url::parse(&cfg.url).map_err(|e| RemoteError::Network(e.to_string()))?;
        let token = secrets::read_token(&cfg.token_file, home)
            .map_err(|e| RemoteError::Network(e.to_string()))?;
        // a redirect would carry the bearer token to whatever host answers next
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(transport)?;
        Ok(RemoteClient {
            http,
            address: public_address(&cfg.url),
            base,
            token,
        })
    }

    pub fn address(&self) -> &str {
        &self.address
    }

    /// The text for the red line or the startup error; it names the server and never the token.
    pub fn explain(&self, e: &RemoteError) -> String {
        explain_error(&self.address, e)
    }

    /// Like `explain`, but a server-sent message also names the server.
    pub fn explain_naming_server(&self, e: &RemoteError) -> String {
        startup_text(&self.address, e)
    }

    fn endpoint(&self, segments: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.set_query(None);
        url.set_fragment(None);
        url.path_segments_mut()
            .expect("http urls can be a base")
            .pop_if_empty()
            .extend(["api", "v1"])
            .extend(segments);
        url
    }

    async fn execute<T: DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
        key: Option<&str>,
    ) -> Result<T, RemoteError> {
        let mut request = request.bearer_auth(&self.token);
        if let Some(key) = key {
            request = request.header("Idempotency-Key", key);
        }
        let response = request.send().await.map_err(transport)?;
        let status = response.status();
        let bytes = read_capped(response).await?;
        if !status.is_success() {
            return Err(classify(status, &bytes));
        }
        serde_json::from_slice(&bytes).map_err(|e| {
            RemoteError::Network(t_args(
                msg::REMOTE_INVALID_ANSWER,
                &[("error", clip(&e.to_string()).as_str().into())],
            ))
        })
    }

    async fn get<T: DeserializeOwned>(&self, url: Url) -> Result<T, RemoteError> {
        self.execute(self.http.get(url), None).await
    }

    async fn post<T: DeserializeOwned>(
        &self,
        url: Url,
        body: &impl Serialize,
        key: Option<&str>,
    ) -> Result<T, RemoteError> {
        self.execute(self.http.post(url).json(body), key).await
    }

    pub async fn profiles(&self) -> Result<ProfilesResponse, RemoteError> {
        self.get(self.endpoint(&["profiles"])).await
    }

    pub async fn profile(&self, name: &str) -> Result<ProfileView, RemoteError> {
        self.get(self.endpoint(&["profiles", name])).await
    }

    pub async fn items(&self, profile: &str, bx: &str) -> Result<ItemsResponse, RemoteError> {
        self.get(self.endpoint(&["profiles", profile, "boxes", bx, "items"]))
            .await
    }

    pub async fn action(
        &self,
        profile: &str,
        bx: &str,
        req: &ActionRequest,
    ) -> Result<ActionResponse, RemoteError> {
        let url = self.endpoint(&["profiles", profile, "boxes", bx, "actions"]);
        self.post(url, req, None).await
    }

    pub async fn drop(
        &self,
        profile: &str,
        bx: &str,
        req: &DropRequest,
        key: Option<&str>,
    ) -> Result<DropResponse, RemoteError> {
        let url = self.endpoint(&["profiles", profile, "boxes", bx, "drops"]);
        self.post(url, req, key).await
    }

    pub async fn capture(
        &self,
        profile: &str,
        req: &CaptureRequest,
        key: Option<&str>,
    ) -> Result<CaptureResponse, RemoteError> {
        let url = self.endpoint(&["profiles", profile, "captures"]);
        self.post(url, req, key).await
    }

    pub async fn reminders(
        &self,
        profile: &str,
        since: DateTime<FixedOffset>,
    ) -> Result<RemindersResponse, RemoteError> {
        let mut url = self.endpoint(&["profiles", profile, "reminders"]);
        url.query_pairs_mut()
            .append_pair("since", &since.to_rfc3339());
        self.get(url).await
    }
}

/// The server could not be reached or answered with a fault; the text names it.
#[derive(Debug)]
pub struct Unreachable(pub String);

impl std::fmt::Display for Unreachable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unreachable {}

/// Picks the profile: flag, environment, config, the server's default, then the only one it has.
pub async fn resolve_profile(
    client: &RemoteClient,
    cli: Option<&str>,
    env: Option<&str>,
    cfg: &Config,
) -> anyhow::Result<String> {
    let listed = client.profiles().await.map_err(|e| {
        let text = startup_text(client.address(), &e);
        match e {
            RemoteError::Network(_) => anyhow::Error::new(Unreachable(text)),
            _ => anyhow::anyhow!(text),
        }
    })?;
    let name = match profile::select_name(cli, env, cfg) {
        Ok(name) => name,
        Err(ProfileError::NoProfileSelected) => listed
            .default
            .clone()
            .or_else(|| match listed.profiles.as_slice() {
                [only] => Some(only.clone()),
                _ => None,
            })
            .ok_or(ProfileError::NoProfileSelected)?,
        Err(e) => return Err(e.into()),
    };
    if !listed.profiles.contains(&name) {
        anyhow::bail!(t_args(
            msg::REMOTE_PROFILE_UNKNOWN,
            &[
                ("address", client.address().into()),
                ("name", name.as_str().into())
            ]
        ));
    }
    Ok(name)
}

/// Connects and picks the profile; the errors name the server and never the token.
pub async fn connect_profile(
    server: &ServerConfig,
    home: &Path,
    cli: Option<&str>,
    env: Option<&str>,
    cfg: &Config,
) -> anyhow::Result<(Arc<RemoteClient>, String)> {
    let client = RemoteClient::connect(server, home)
        .map_err(|e| anyhow::anyhow!(startup_text(&public_address(&server.url), &e)))?;
    let client = Arc::new(client);
    let name = resolve_profile(&client, cli, env, cfg).await?;
    Ok((client, name))
}

/// Connects and loads the chosen profile (its server key and view); any failure here keeps the TUI from opening.
pub async fn open(
    server: &ServerConfig,
    home: &Path,
    cli: Option<&str>,
    env: Option<&str>,
    cfg: &Config,
) -> anyhow::Result<(Arc<RemoteClient>, String, ProfileView)> {
    let (client, name) = connect_profile(server, home, cli, env, cfg).await?;
    let view = client
        .profile(&name)
        .await
        .map_err(|e| anyhow::anyhow!(startup_text(client.address(), &e)))?;
    Ok((client, name, view))
}

pub struct RemoteBox {
    client: Arc<RemoteClient>,
    profile: String,
    box_id: String,
    describe: Result<Describe, String>,
}

fn describe_of(view: &BoxView) -> Result<Describe, String> {
    match (&view.describe, &view.error) {
        (Some(describe), _) => Ok(describe.clone()),
        (None, Some(error)) => Err(error.clone()),
        (None, None) => Err(t(msg::REMOTE_DESCRIBE_UNAVAILABLE)),
    }
}

impl RemoteBox {
    pub fn new(client: Arc<RemoteClient>, profile: String, view: &BoxView) -> RemoteBox {
        RemoteBox {
            client,
            profile,
            box_id: view.id.clone(),
            describe: describe_of(view),
        }
    }

    /// A box whose plugin could not start is asked for again, since the server may have recovered.
    pub async fn describe(&mut self) -> Result<Describe, HostError> {
        if self.describe.is_err() {
            let view = self
                .client
                .profile(&self.profile)
                .await
                .map_err(|e| self.fail(&e))?;
            let bx = view
                .boxes
                .iter()
                .find(|b| b.id == self.box_id)
                .ok_or_else(|| {
                    HostError::Remote(t_args(
                        msg::REMOTE_BOX_MISSING,
                        &[
                            ("profile", self.profile.as_str().into()),
                            ("box", self.box_id.as_str().into()),
                        ],
                    ))
                })?;
            self.describe = describe_of(bx);
        }
        self.describe.clone().map_err(HostError::Remote)
    }

    fn fail(&self, e: &RemoteError) -> HostError {
        HostError::Remote(self.client.explain(e))
    }

    pub async fn call<P: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        params: &P,
    ) -> Result<R, HostError> {
        let params = serde_json::to_value(params).map_err(|e| HostError::Plugin(e.to_string()))?;
        let out = match method {
            "list" => {
                let r = self.client.items(&self.profile, &self.box_id).await;
                let items = r.map_err(|e| self.fail(&e))?.items;
                to_value(&ListResult { items })?
            }
            "action" => {
                let p: ActionParams = from_params(params)?;
                let req = ActionRequest {
                    item: p.item,
                    action: p.action,
                    args: p.args,
                };
                let r = self.client.action(&self.profile, &self.box_id, &req).await;
                to_value(&r.map_err(|e| self.fail(&e))?.item)?
            }
            "drop" => {
                let req: DropParams = from_params(params)?;
                // a fresh key per drop: nothing retries on its own, and a duplicate from a proxy is absorbed
                let key = crate::fsio::unique_id();
                let r =
                    RemoteClient::drop(&self.client, &self.profile, &self.box_id, &req, Some(&key))
                        .await;
                to_value(&r.map_err(|e| self.fail(&e))?.item)?
            }
            "reminders" => {
                let p: RemindersParams = from_params(params)?;
                let r = self.client.reminders(&self.profile, p.since).await;
                let reminders: Vec<Reminder> = r
                    .map_err(|e| self.fail(&e))?
                    .reminders
                    .into_iter()
                    .filter(|r| r.box_id == self.box_id)
                    .map(|r| Reminder {
                        at: r.at,
                        item: r.item,
                    })
                    .collect();
                to_value(&RemindersResult { reminders })?
            }
            _ => {
                return Err(HostError::Remote(t_args(
                    msg::REMOTE_UNSUPPORTED_CALL,
                    &[("method", method.into())],
                )));
            }
        };
        serde_json::from_value(out).map_err(|e| {
            HostError::BadLine(t_args(
                msg::HOST_INVALID_RESULT,
                &[("error", e.to_string().as_str().into())],
            ))
        })
    }
}

fn from_params<T: DeserializeOwned>(params: Value) -> Result<T, HostError> {
    serde_json::from_value(params).map_err(|e| HostError::Plugin(e.to_string()))
}

fn to_value(value: &impl Serialize) -> Result<Value, HostError> {
    serde_json::to_value(value).map_err(|e| HostError::Plugin(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ErrorCode;
    use focusdeck_protocol::{Item, ListParams, View};
    use mockito::{Server, ServerGuard};
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";

    struct Env {
        _home: tempfile::TempDir,
        server: ServerGuard,
        client: Arc<RemoteClient>,
    }

    async fn env() -> Env {
        let server = Server::new_async().await;
        let home = tempfile::tempdir().unwrap();
        let client = client_for(&server.url(), home.path());
        Env {
            _home: home,
            server,
            client,
        }
    }

    fn client_for(url: &str, home: &Path) -> Arc<RemoteClient> {
        let token = home.join("token");
        std::fs::write(&token, format!("{TOKEN}\n")).unwrap();
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        let cfg = ServerConfig {
            url: url.into(),
            token_file: token.display().to_string(),
        };
        Arc::new(RemoteClient::connect(&cfg, home).unwrap())
    }

    fn describe_json() -> Value {
        json!({"name":"local","version":"1","protocol":1,"actions":["done"],"accepts_drop":true,"reminders":true})
    }

    fn box_view(describe: bool) -> BoxView {
        BoxView {
            id: "notes".into(),
            title: "Notes".into(),
            plugin: "local".into(),
            row: 0,
            col: 0,
            width: 1,
            height: 1,
            refresh: 60,
            plan_into: None,
            describe: describe.then(|| serde_json::from_value(describe_json()).unwrap()),
            error: (!describe).then(|| "plugin is down".into()),
        }
    }

    fn item_json(id: &str) -> Value {
        json!({"id": id, "title": format!("t{id}")})
    }

    fn error_body(code: &str, message: &str) -> String {
        json!({"error": {"code": code, "message": message}}).to_string()
    }

    fn list_params() -> ListParams {
        ListParams { view: View::Full }
    }

    fn view_json(boxes: Vec<Value>) -> String {
        json!({
            "name": "p", "capture_into": null, "capture_refs": {}, "capture_label_aliases": {},
            "capture_words": {}, "boxes": boxes
        })
        .to_string()
    }

    fn box_json(describe: bool) -> Value {
        serde_json::to_value(box_view(describe)).unwrap()
    }

    #[tokio::test]
    async fn remote_box_lists_items() {
        let mut env = env().await;
        let mock = env
            .server
            .mock("GET", "/api/v1/profiles/p/boxes/notes/items")
            .match_header("authorization", format!("Bearer {TOKEN}").as_str())
            .with_body(
                json!({"items": [item_json("1"), item_json("2")], "listed_at": "2026-11-13T10:00:00+03:00"})
                    .to_string(),
            )
            .create_async()
            .await;
        let mut bx = RemoteBox::new(env.client.clone(), "p".into(), &box_view(true));
        assert!(bx.describe().await.unwrap().accepts_drop);
        let listed: ListResult = bx.call("list", &list_params()).await.unwrap();
        let ids: Vec<_> = listed.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["1", "2"]);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn remote_plugin_error_message_passes_through() {
        let mut env = env().await;
        env.server
            .mock("GET", "/api/v1/profiles/p/boxes/notes/items")
            .with_status(502)
            .with_body(error_body("PLUGIN", "vikunja said no"))
            .create_async()
            .await;
        let mut bx = RemoteBox::new(env.client.clone(), "p".into(), &box_view(true));
        let err = bx
            .call::<_, ListResult>("list", &list_params())
            .await
            .unwrap_err();
        assert!(
            matches!(&err, HostError::Remote(m) if m == "vikunja said no"),
            "{err:?}"
        );
        assert_eq!(err.to_string(), "vikunja said no");
    }

    #[tokio::test]
    async fn unauthorized_names_server_address() {
        let mut env = env().await;
        env.server
            .mock("GET", "/api/v1/profiles")
            .with_status(401)
            .with_body(error_body("UNAUTHORIZED", "the token is missing or wrong"))
            .create_async()
            .await;
        let err = env.client.profiles().await.unwrap_err();
        assert_eq!(err, RemoteError::Unauthorized);
        let text = env.client.explain(&err);
        assert!(text.contains(&env.server.url()), "{text}");
        assert!(!text.contains(TOKEN), "{text}");
        assert!(!format!("{:?} {err:?}", env.client).contains(TOKEN));
        let mut bx = RemoteBox::new(env.client.clone(), "p".into(), &box_view(true));
        let err = bx
            .call::<_, ListResult>("list", &list_params())
            .await
            .unwrap_err();
        assert!(!err.to_string().contains(TOKEN));
    }

    #[tokio::test]
    async fn remote_box_network_error_is_red_line_then_recovers() {
        let mut env = env().await;
        env.server
            .mock("GET", "/api/v1/profiles/p/boxes/notes/items")
            .with_status(500)
            .with_body(error_body("INTERNAL", "the server could not use its store"))
            .expect(1)
            .create_async()
            .await;
        env.server
            .mock("GET", "/api/v1/profiles/p/boxes/notes/items")
            .with_body(
                json!({"items": [item_json("1")], "listed_at": "2026-11-13T10:00:00+03:00"})
                    .to_string(),
            )
            .create_async()
            .await;
        let mut bx = RemoteBox::new(env.client.clone(), "p".into(), &box_view(true));
        let err = bx
            .call::<_, ListResult>("list", &list_params())
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(&env.server.url()) && err.contains("500"),
            "{err}"
        );
        assert!(err.contains("could not use its store"), "{err}");
        let listed: ListResult = bx.call("list", &list_params()).await.unwrap();
        assert_eq!(listed.items.len(), 1);
    }

    #[tokio::test]
    async fn unreachable_server_names_its_address() {
        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}", l.local_addr().unwrap())
        };
        let home = tempfile::tempdir().unwrap();
        let client = client_for(&dead, home.path());
        let err = client.profiles().await.unwrap_err();
        assert!(matches!(err, RemoteError::Network(_)), "{err:?}");
        assert!(client.explain(&err).contains(&dead));
    }

    #[tokio::test]
    async fn server_faults_are_network_errors_not_parse_failures() {
        let mut env = env().await;
        let not_found = RemoteError::Api {
            code: ErrorCode::NotFound,
            message: "not found".into(),
        };
        let cases: Vec<(usize, String, Option<RemoteError>, &str)> = vec![
            (500, "<html>oops</html>".into(), None, "HTTP 500"),
            (
                404,
                error_body("SOMETHING_NEW", "odd"),
                None,
                "HTTP 404: odd",
            ),
            (
                503,
                error_body("NOT_FOUND", "behind a proxy"),
                None,
                "HTTP 503: behind a proxy",
            ),
            (
                404,
                error_body("NOT_FOUND", "not found"),
                Some(not_found),
                "",
            ),
            (
                401,
                error_body("SOMETHING_NEW", "x"),
                Some(RemoteError::Unauthorized),
                "",
            ),
            (200, "not json".into(), None, "not valid"),
        ];
        for (status, body, expected, needle) in cases {
            let mock = env
                .server
                .mock("GET", "/api/v1/profiles")
                .with_status(status)
                .with_body(body)
                .create_async()
                .await;
            let err = env.client.profiles().await.unwrap_err();
            mock.remove_async().await;
            match expected {
                Some(expected) => assert_eq!(err, expected, "{status}"),
                None => match &err {
                    RemoteError::Network(text) => assert!(text.contains(needle), "{text}"),
                    other => panic!("{status}: {other:?}"),
                },
            }
        }
    }

    #[tokio::test]
    async fn redirects_are_not_followed() {
        let mut env = env().await;
        let mut elsewhere = Server::new_async().await;
        let hit = elsewhere
            .mock("GET", "/api/v1/profiles")
            .expect(0)
            .create_async()
            .await;
        env.server
            .mock("GET", "/api/v1/profiles")
            .with_status(302)
            .with_header("location", &format!("{}/api/v1/profiles", elsewhere.url()))
            .create_async()
            .await;
        let err = env.client.profiles().await.unwrap_err();
        assert!(
            matches!(&err, RemoteError::Network(t) if t.contains("302")),
            "{err:?}"
        );
        hit.assert_async().await;
    }

    #[test]
    fn timeout_text_carries_the_configured_seconds() {
        assert_eq!(timeout_text(), "no answer within 20 s");
    }

    #[tokio::test]
    async fn oversized_answers_and_texts_are_cut() {
        let mut env = env().await;
        env.server
            .mock("GET", "/api/v1/profiles")
            .with_body(vec![b' '; MAX_BODY_BYTES + 1])
            .create_async()
            .await;
        let err = env.client.profiles().await.unwrap_err();
        assert!(
            matches!(&err, RemoteError::Network(t) if t.contains("16 MiB")),
            "{err:?}"
        );

        env.server.reset();
        env.server
            .mock("GET", "/api/v1/profiles")
            .with_status(500)
            .with_body(error_body("INTERNAL", &"x".repeat(5000)))
            .create_async()
            .await;
        let RemoteError::Network(text) = env.client.profiles().await.unwrap_err() else {
            panic!("expected a network error");
        };
        assert!(text.chars().count() < 300, "{}", text.len());
    }

    #[tokio::test]
    async fn startup_api_errors_name_the_address_but_the_box_line_stays_verbatim() {
        let mut env = env().await;
        env.server
            .mock("GET", "/api/v1/profiles")
            .with_status(404)
            .with_body(error_body("NOT_FOUND", "not found"))
            .create_async()
            .await;
        let err = resolve_profile(&env.client, None, None, &Config::default())
            .await
            .unwrap_err()
            .to_string();
        assert!(
            err.contains(&env.server.url()) && err.contains("not found"),
            "{err}"
        );
        let api = RemoteError::Api {
            code: ErrorCode::Plugin,
            message: "vikunja said no".into(),
        };
        assert_eq!(env.client.explain(&api), "vikunja said no");
    }

    #[test]
    fn plain_http_to_a_remote_host_is_refused_before_any_request() {
        let home = tempfile::tempdir().unwrap();
        let token = home.path().join("token");
        std::fs::write(&token, TOKEN).unwrap();
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        let config = |url: &str| ServerConfig {
            url: url.into(),
            token_file: token.display().to_string(),
        };
        let url = "http://deck.example.com";
        let err = RemoteClient::connect(&config(url), home.path()).unwrap_err();
        let expected = t_args(msg::ERROR_SERVER_URL_INSECURE, &[("url", url.into())]);
        assert_eq!(err, RemoteError::Network(expected));
        assert!(RemoteClient::connect(&config("http://127.0.0.1:8740"), home.path()).is_ok());
    }

    #[tokio::test]
    async fn drops_carry_a_fresh_idempotency_key() {
        let mut env = env().await;
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let record = Arc::clone(&seen);
        let mock = env
            .server
            .mock("POST", "/api/v1/profiles/p/boxes/notes/drops")
            .match_request(move |req| {
                let key = req.header("idempotency-key");
                record
                    .lock()
                    .unwrap()
                    .extend(key.iter().map(|v| v.to_str().unwrap().to_owned()));
                !key.is_empty()
            })
            .with_body(json!({"item": item_json("9")}).to_string())
            .expect(2)
            .create_async()
            .await;
        let mut bx = RemoteBox::new(env.client.clone(), "p".into(), &box_view(true));
        let params = DropParams {
            item: serde_json::from_value(item_json("9")).unwrap(),
            from: None,
        };
        let item: Item = bx.call("drop", &params).await.unwrap();
        assert_eq!(item.id, "9");
        bx.call::<_, Item>("drop", &params).await.unwrap();
        mock.assert_async().await;
        let keys = seen.lock().unwrap().clone();
        assert!(keys.len() >= 2, "{keys:?}");
        assert!(
            keys.iter()
                .all(|k| k.chars().all(|c| c.is_ascii_hexdigit()))
        );
        assert_ne!(keys[0], keys[keys.len() - 1], "{keys:?}");
    }

    #[tokio::test]
    async fn describe_error_refetches_the_profile() {
        let mut env = env().await;
        env.server
            .mock("GET", "/api/v1/profiles/p")
            .with_body(view_json(vec![box_json(true)]))
            .create_async()
            .await;
        let mut bx = RemoteBox::new(env.client.clone(), "p".into(), &box_view(false));
        assert!(bx.describe().await.unwrap().reminders);
    }

    #[tokio::test]
    async fn describe_error_stays_when_the_server_still_reports_it() {
        let mut env = env().await;
        env.server
            .mock("GET", "/api/v1/profiles/p")
            .with_body(view_json(vec![box_json(false)]))
            .create_async()
            .await;
        let mut bx = RemoteBox::new(env.client.clone(), "p".into(), &box_view(false));
        let err = bx.describe().await.unwrap_err();
        assert!(
            matches!(&err, HostError::Remote(m) if m == "plugin is down"),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn resolve_profile_follows_flag_env_default_single() {
        let mut env = env().await;
        let list = |profiles: &[&str], default: Option<&str>| {
            json!({"profiles": profiles, "default": default}).to_string()
        };
        let cfg_with = |name: Option<&str>| Config {
            default_profile: name.map(str::to_owned),
            ..Config::default()
        };
        let client = env.client.clone();
        let mut set = |body: String| {
            env.server.reset();
            env.server
                .mock("GET", "/api/v1/profiles")
                .with_body(body)
                .create()
        };

        let _m = set(list(&["a", "b", "c", "d"], Some("d")));
        let resolve = |cli, env, cfg| {
            let client = client.clone();
            async move { resolve_profile(&client, cli, env, &cfg).await }
        };
        let name = resolve(Some("a"), Some("b"), cfg_with(Some("c"))).await;
        assert_eq!(name.unwrap(), "a");
        assert_eq!(
            resolve(None, Some("b"), cfg_with(Some("c"))).await.unwrap(),
            "b"
        );
        assert_eq!(resolve(None, None, cfg_with(Some("c"))).await.unwrap(), "c");
        assert_eq!(resolve(None, None, cfg_with(None)).await.unwrap(), "d");
        let unknown = resolve(Some("zz"), None, cfg_with(None)).await.unwrap_err();
        assert!(
            unknown.to_string().contains("zz") && unknown.to_string().contains(client.address())
        );
        let bad = resolve(Some("../x"), None, cfg_with(None))
            .await
            .unwrap_err();
        assert!(bad.downcast_ref::<ProfileError>().is_some());

        let _m = set(list(&["only"], None));
        assert_eq!(resolve(None, None, cfg_with(None)).await.unwrap(), "only");

        let _m = set(list(&["a", "b"], None));
        let err = resolve(None, None, cfg_with(None)).await.unwrap_err();
        assert!(matches!(
            err.downcast_ref::<ProfileError>(),
            Some(ProfileError::NoProfileSelected)
        ));
    }

    #[tokio::test]
    async fn startup_failure_names_the_address_and_never_the_token() {
        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}", l.local_addr().unwrap())
        };
        let home = tempfile::tempdir().unwrap();
        let token = home.path().join("token");
        std::fs::write(&token, TOKEN).unwrap();
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        let cfg = ServerConfig {
            url: dead.clone(),
            token_file: token.display().to_string(),
        };
        let err = open(&cfg, home.path(), None, None, &Config::default())
            .await
            .err()
            .unwrap();
        let text = format!("{err:#}");
        assert!(text.contains(&dead) && !text.contains(TOKEN), "{text}");
    }
}

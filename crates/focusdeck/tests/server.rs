use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef";

const PROFILE: &str = r#"
name = "pano"
capture_into = "notes"

[[box]]
id = "notes"
title = "Notlar"
plugin = "local"
row = 0
col = 0
refresh = "1s"

[[box]]
id = "today"
title = "Bugün"
plugin = "local"
row = 0
col = 1
refresh = "1s"
config = { key_file = "~/secret.key", email = "me@example.com" }
"#;

struct Proc(Child);

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn stderr_log(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("stderr.log")).unwrap_or_default()
}

fn spawn_server(dir: &Path, stdout: Stdio) -> Proc {
    let stderr = std::fs::File::create(dir.join("stderr.log")).unwrap();
    Proc(
        command(dir)
            .arg("server")
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .unwrap(),
    )
}

/// Runs the server expecting it to exit by itself; a server that keeps running fails the test.
fn exit_with_error(dir: &Path) -> String {
    let mut proc = spawn_server(dir, Stdio::null());
    for _ in 0..600 {
        if let Some(status) = proc.0.try_wait().unwrap() {
            assert!(!status.success());
            return stderr_log(dir);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("the server kept running");
}

struct Server {
    _proc: Proc,
    base: String,
    client: reqwest::Client,
    _dir: tempfile::TempDir,
}

fn command(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_focusdeck"));
    command
        .env("HOME", dir.join("home"))
        .env("FOCUSDECK_CONFIG_DIR", dir.join("config"))
        .env("FOCUSDECK_DATA_DIR", dir.join("data"))
        .env("FOCUSDECK_STATE_DIR", dir.join("state"))
        .env_remove("FOCUSDECK_PROFILE")
        .env_remove("FOCUSDECK_LANG")
        .env_remove("LANG")
        .env_remove("LC_MESSAGES")
        .env("LC_ALL", "C");
    command
}

fn prepare(dir: &Path, config: &str) {
    std::fs::create_dir_all(dir.join("home")).unwrap();
    std::fs::create_dir_all(dir.join("config/profiles")).unwrap();
    std::fs::create_dir_all(dir.join("state")).unwrap();
    let token = dir.join("home").join("token");
    std::fs::write(&token, format!("{TOKEN}\n")).unwrap();
    std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
    let secret = dir.join("home").join("secret.key");
    std::fs::write(&secret, "x").unwrap();
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(dir.join("config/config.toml"), config).unwrap();
    std::fs::write(dir.join("config/profiles/pano.toml"), PROFILE).unwrap();
}

const SERVE: &str = "[serve]\nlisten = \"127.0.0.1:0\"\ntoken_file = \"~/token\"\n";

impl Server {
    fn start() -> Server {
        let dir = tempfile::tempdir().unwrap();
        prepare(dir.path(), SERVE);
        let mut proc = spawn_server(dir.path(), Stdio::piped());
        let stdout = proc.0.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = tx.send(line);
            }
        });
        let line = rx
            .recv_timeout(Duration::from_secs(30))
            .unwrap_or_else(|_| panic!("no listening line, stderr: {}", stderr_log(dir.path())));
        let addr = line
            .split_whitespace()
            .find(|word| word.starts_with("127.0.0.1:"))
            .unwrap_or_else(|| panic!("no address in {line:?}"));
        Server {
            _proc: proc,
            base: format!("http://{addr}/api/v1"),
            client: reqwest::Client::new(),
            _dir: dir,
        }
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .get(format!("{}{path}", self.base))
            .bearer_auth(TOKEN)
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .post(format!("{}{path}", self.base))
            .bearer_auth(TOKEN)
    }

    async fn capture(&self, text: &str, key: Option<&str>) -> (u16, Value) {
        let mut req = self.post("/profiles/pano/captures").json(&json!({
            "text": text,
            "at": "2026-11-13T10:00:00+03:00",
            "lang": "en",
        }));
        if let Some(key) = key {
            req = req.header("Idempotency-Key", key);
        }
        send(req).await
    }

    async fn titles(&self, box_id: &str) -> Vec<String> {
        let (status, body) = send(self.get(&format!("/profiles/pano/boxes/{box_id}/items"))).await;
        assert_eq!(status, 200, "{body}");
        body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["title"].as_str().unwrap().to_owned())
            .collect()
    }
}

async fn send(req: reqwest::RequestBuilder) -> (u16, Value) {
    let response = req.send().await.unwrap();
    let status = response.status().as_u16();
    let text = response.text().await.unwrap();
    let body = serde_json::from_str(&text).unwrap_or_else(|_| panic!("not JSON: {text}"));
    (status, body)
}

#[tokio::test]
async fn requests_without_valid_token_get_401() {
    let server = Server::start();
    let url = format!("{}/profiles", server.base);
    let wrong = format!("{}x", &TOKEN[..TOKEN.len() - 1]);
    let attempts = [
        server.client.get(&url),
        server.client.get(&url).bearer_auth(wrong),
        server.client.get(&url).header("Authorization", TOKEN),
        server
            .client
            .get(format!("{}/profiles/pano/boxes/notes/items", server.base)),
    ];
    for req in attempts {
        let (status, body) = send(req).await;
        assert_eq!(status, 401);
        assert_eq!(body["error"]["code"], "UNAUTHORIZED");
        assert!(body["error"]["message"].is_string());
        assert!(!body.to_string().contains(TOKEN));
    }
    let (status, body) = send(server.get("/profiles")).await;
    assert_eq!(status, 200);
    assert_eq!(body, json!({"profiles": ["pano"], "default": "pano"}));
}

#[tokio::test]
async fn profile_has_no_config_or_secret_paths() {
    let server = Server::start();
    let (status, body) = send(server.get("/profiles/pano")).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["name"], "pano");
    assert_eq!(body["capture_into"], "notes");
    assert_eq!(body["boxes"].as_array().unwrap().len(), 2);
    assert_eq!(body["boxes"][0]["describe"]["name"], "local");
    assert_eq!(body["boxes"][0]["refresh"], 1);
    let text = body.to_string();
    for leak in ["config", "key_file", "secret.key", "me@example.com", TOKEN] {
        assert!(!text.contains(leak), "{leak} leaked: {text}");
    }
}

#[tokio::test]
async fn capture_then_list_shows_note() {
    let server = Server::start();
    let (status, body) = server.capture("buy milk", None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["box"], "notes");
    assert_eq!(body["item"]["title"], "buy milk");
    assert_eq!(server.titles("notes").await, ["buy milk"]);
    assert!(server.titles("today").await.is_empty());
}

#[tokio::test]
async fn capture_with_same_key_twice_creates_one_note() {
    let server = Server::start();
    let (s1, first) = server.capture("pay rent", Some("k-1")).await;
    let (s2, second) = server.capture("pay rent", Some("k-1")).await;
    assert_eq!((s1, s2), (200, 200));
    assert_eq!(first, second);
    assert_eq!(server.titles("notes").await, ["pay rent"]);
}

#[tokio::test]
async fn same_key_other_body_is_key_reused() {
    let server = Server::start();
    let (status, _) = server.capture("one", Some("k-2")).await;
    assert_eq!(status, 200);
    let (status, body) = server.capture("two", Some("k-2")).await;
    assert_eq!(status, 422);
    assert_eq!(body["error"]["code"], "KEY_REUSED");
    assert_eq!(server.titles("notes").await, ["one"]);
}

#[tokio::test]
async fn drop_between_local_boxes_moves_once() {
    let server = Server::start();
    let (_, captured) = server.capture("move me", None).await;
    let id = captured["item"]["id"].as_str().unwrap().to_owned();
    let drop = json!({"item": captured["item"], "from": {"plugin": "local", "id": id}});
    let send_drop = || {
        server
            .post("/profiles/pano/boxes/today/drops")
            .header("Idempotency-Key", "k-3")
            .json(&drop)
    };
    let (s1, first) = send(send_drop()).await;
    let (s2, second) = send(send_drop()).await;
    assert_eq!((s1, s2), (200, 200), "{first} {second}");
    assert_eq!(first, second);
    assert!(server.titles("notes").await.is_empty());
    assert_eq!(server.titles("today").await, ["move me"]);
}

#[tokio::test]
async fn unknown_profile_is_404() {
    let server = Server::start();
    let (status, body) = send(server.get("/profiles/yok")).await;
    assert_eq!(status, 404);
    assert_eq!(body["error"]["code"], "NOT_FOUND");
    let (status, body) = send(server.get("/profiles/pano/boxes/yok/items")).await;
    assert_eq!((status, &body["error"]["code"]), (404, &json!("NOT_FOUND")));
    let (status, body) = send(server.get("/nowhere")).await;
    assert_eq!((status, &body["error"]["code"]), (404, &json!("NOT_FOUND")));
}

#[tokio::test]
async fn malformed_and_oversized_bodies_use_the_error_shape() {
    let server = Server::start();
    let bad = server
        .post("/profiles/pano/captures")
        .header("Content-Type", "application/json")
        .body("{not json");
    let (status, body) = send(bad).await;
    assert_eq!((status, &body["error"]["code"]), (422, &json!("INVALID")));
    let big = server
        .post("/profiles/pano/captures")
        .header("Content-Type", "application/json")
        .body(vec![b'a'; 1024 * 1024 + 1]);
    let (status, body) = send(big).await;
    assert_eq!((status, &body["error"]["code"]), (422, &json!("INVALID")));
}

#[test]
fn server_without_serve_section_exits_with_error() {
    let dir = tempfile::tempdir().unwrap();
    prepare(dir.path(), "");
    assert!(exit_with_error(dir.path()).contains("[serve]"));
}

#[test]
fn corrupt_idempotency_file_stops_the_server() {
    let dir = tempfile::tempdir().unwrap();
    prepare(dir.path(), SERVE);
    std::fs::write(dir.path().join("state/idempotency.json"), "{broken").unwrap();
    assert!(exit_with_error(dir.path()).contains("idempotency.json"));
}

#[tokio::test]
async fn failed_request_releases_its_key_for_a_retry() {
    let server = Server::start();
    for _ in 0..2 {
        let req = server
            .post("/profiles/yok/captures")
            .header("Idempotency-Key", "k-4")
            .json(&json!({"text": "x", "at": "2026-11-13T10:00:00+03:00", "lang": "en"}));
        let (status, body) = send(req).await;
        assert_eq!(
            (status, &body["error"]["code"]),
            (404, &json!("NOT_FOUND")),
            "{body}"
        );
    }
}

#[tokio::test]
async fn wrong_method_and_bad_path_use_the_error_shape() {
    let server = Server::start();
    let (status, body) = send(server.post("/profiles")).await;
    assert_eq!((status, &body["error"]["code"]), (404, &json!("NOT_FOUND")));
    let (status, body) = send(server.get("/profiles/%FF")).await;
    assert_eq!((status, &body["error"]["code"]), (404, &json!("NOT_FOUND")));
}

#[tokio::test]
async fn unknown_route_without_token_is_401() {
    let server = Server::start();
    let url = server.base.replace("/api/v1", "/nowhere");
    let (status, body) = send(server.client.get(url)).await;
    assert_eq!(
        (status, &body["error"]["code"]),
        (401, &json!("UNAUTHORIZED"))
    );
}

#[test]
fn second_server_on_a_busy_port_leaves_the_key_store_alone() {
    let first = Server::start();
    let addr = first
        .base
        .trim_start_matches("http://")
        .trim_end_matches("/api/v1");
    let dir = tempfile::tempdir().unwrap();
    prepare(dir.path(), &SERVE.replace("127.0.0.1:0", addr));
    let store = dir.path().join("state/idempotency.json");
    let pending = r#"{"version":1,"keys":{"k":{"body":"00","state":"in_progress","at":"2026-11-13T10:00:00+03:00"}}}"#;
    std::fs::write(&store, pending).unwrap();
    exit_with_error(dir.path());
    assert_eq!(std::fs::read_to_string(&store).unwrap(), pending);
}

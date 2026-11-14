use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use chrono::{DateTime, Duration, FixedOffset, Utc};
use mockito::{Matcher, Mock, Server, ServerGuard};
use seenboard::queue::{Queue, Queued};
use seenboard_protocol::Item;
use serde_json::{Value, json};

const TOKEN: &str = "0123456789abcdef0123456789abcdef0123";

struct Env {
    dir: tempfile::TempDir,
}

fn at(s: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(s).unwrap()
}

fn item(title: &str, due: Option<&str>) -> Value {
    serde_json::to_value(Item {
        id: "7".into(),
        title: title.into(),
        subtitle: None,
        url: None,
        due: due.map(at),
        overdue: false,
        source: None,
        description: None,
        labels: vec![],
        from: None,
        links: vec![],
    })
    .unwrap()
}

fn profile_mocks(server: &mut ServerGuard, capture_into: Value) -> Mock {
    server
        .mock("GET", "/api/v1/profiles/p")
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "name": "p", "capture_into": capture_into, "capture_refs": {},
                "capture_label_aliases": {}, "capture_words": {}, "boxes": []
            })
            .to_string(),
        )
        .create();
    server
        .mock("GET", "/api/v1/profiles")
        .with_header("content-type", "application/json")
        .with_body(r#"{"profiles":["p"],"default":"p"}"#)
        .create()
}

fn profiles(server: &mut ServerGuard) -> Mock {
    profile_mocks(server, json!("notes"))
}

fn profiles_without_capture_into(server: &mut ServerGuard) -> Mock {
    profile_mocks(server, Value::Null)
}

impl Env {
    fn new(url: &str) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        let token = root.join("token");
        std::fs::write(&token, TOKEN).unwrap();
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(
            root.join("config/config.toml"),
            format!(
                "[server]\nurl = \"{url}\"\ntoken_file = \"{}\"\n",
                token.display()
            ),
        )
        .unwrap();
        let notify = root.join("bin/notify-send");
        std::fs::write(
            &notify,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do printf '%s\\t' \"$a\"; done >> '{0}'\necho >> '{0}'\n",
                root.join("notify.log").display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&notify, std::fs::Permissions::from_mode(0o755)).unwrap();
        Env { dir }
    }

    fn state_dir(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    fn queue(&self) -> Queue {
        Queue::new(self.state_dir().join("capture-queue.jsonl"))
    }

    fn queued(&self) -> Vec<Queued> {
        let queue = self.queue();
        let taken = queue.take().unwrap();
        let items = taken.items.clone();
        queue.commit(taken, &[]).unwrap();
        items
    }

    fn notifications(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("notify.log")).unwrap_or_default()
    }

    fn run(&self, args: &[&str]) -> Output {
        let root = self.dir.path();
        let path = format!(
            "{}:{}",
            root.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Command::new(env!("CARGO_BIN_EXE_seenboard"))
            .args(args)
            .env("PATH", path)
            .env_remove("SEENBOARD_LANG")
            .env_remove("SEENBOARD_PROFILE")
            .env_remove("LANG")
            .env_remove("LC_MESSAGES")
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .env("SEENBOARD_CONFIG_DIR", root.join("config"))
            .env("SEENBOARD_DATA_DIR", root.join("data"))
            .env("SEENBOARD_STATE_DIR", self.state_dir())
            .output()
            .unwrap()
    }
}

fn code(out: &Output) -> i32 {
    out.status.code().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn capture_ok(server: &mut ServerGuard, key: Matcher, body: Value, title: &str) -> mockito::Mock {
    server
        .mock("POST", "/api/v1/profiles/p/captures")
        .match_header("idempotency-key", key)
        .match_body(Matcher::PartialJson(body))
        .with_header("content-type", "application/json")
        .with_body(
            json!({"box": "notes", "item": item(title, Some("2026-11-14T14:30:00+03:00"))})
                .to_string(),
        )
        .expect(1)
        .create()
}

#[test]
fn remote_capture_sends_entry_key() {
    let mut server = Server::new();
    let env = Env::new(&server.url());
    let _p = profiles(&mut server);
    let old = Queued {
        text: "eski".into(),
        at: at("2026-11-11T10:00:00.123456789+03:00"),
        lang: Some("en".into()),
        id: None,
    };
    env.queue().push_entry(&old).unwrap();
    let old_key = old.key();
    let for_old = capture_ok(
        &mut server,
        Matcher::Exact(old_key),
        json!({"text": "eski", "lang": "en"}),
        "eski",
    );
    let for_new = capture_ok(
        &mut server,
        Matcher::Regex("^.{8,255}$".into()),
        json!({"text": "log at 14:30", "lang": "en"}),
        "log at",
    );
    let out = env.run(&["capture", "log at 14:30"]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    for_old.assert();
    for_new.assert();
    assert!(env.queued().is_empty());
    let log = env.notifications();
    assert!(log.contains("added\t"), "{log}");
    assert!(log.contains("log at · 11:30"), "{log}");
    assert!(!log.contains(TOKEN), "{log}");
}

#[test]
fn remote_invalid_capture_is_dropped_from_queue_and_notified() {
    let mut server = Server::new();
    let env = Env::new(&server.url());
    let _p = profiles(&mut server);
    let refused = server
        .mock("POST", "/api/v1/profiles/p/captures")
        .with_status(422)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"code":"INVALID","message":"title is empty"}}"#)
        .expect(1)
        .create();
    let out = env.run(&["capture", "@ali"]);
    assert_eq!(code(&out), 1);
    refused.assert();
    assert!(env.queued().is_empty());
    let log = env.notifications();
    assert!(
        log.contains("-u\tcritical\t--\tSeenboard: not added\t@ali\ntitle is empty\t"),
        "{log}"
    );
}

#[test]
fn unreachable_server_keeps_note_in_queue() {
    let dead = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", listener.local_addr().unwrap())
    };
    let env = Env::new(&dead);
    let out = env.run(&["capture", "süt al"]);
    assert_eq!(code(&out), 1);
    let queued = env.queued();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].text, "süt al");
    let id = queued[0].id.clone().unwrap();
    assert!(!id.is_empty() && id.len() <= 255);
    let log = env.notifications();
    assert!(log.contains("Seenboard: queued\tsüt al\t"), "{log}");
    assert!(text(&out.stderr).contains(&dead), "{}", text(&out.stderr));
    assert!(!log.contains(TOKEN));

    let mut server = Server::new();
    let env2 = Env::new(&server.url());
    env2.queue().push_entry(&queued[0]).unwrap();
    let _p = profiles(&mut server);
    let sent = capture_ok(
        &mut server,
        Matcher::Exact(id),
        json!({"text": "süt al"}),
        "süt al",
    );
    // the queued note goes out under its stored key, then the new one
    let _new = capture_ok(
        &mut server,
        Matcher::Any,
        json!({"text": "ikinci"}),
        "ikinci",
    );
    let out = env2.run(&["capture", "ikinci"]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    sent.assert();
    assert!(env2.queued().is_empty());
}

#[test]
fn rejected_token_keeps_note_queued_and_names_the_server() {
    let mut server = Server::new();
    let env = Env::new(&server.url());
    let _denied = server
        .mock("GET", "/api/v1/profiles")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"code":"UNAUTHORIZED","message":"no"}}"#)
        .create();
    let out = env.run(&["capture", "not"]);
    assert_eq!(code(&out), 1);
    assert_eq!(env.queued().len(), 1);
    let log = env.notifications();
    assert!(log.contains("Seenboard: queued\tnot\n"), "{log}");
    assert!(log.contains("refused the token"), "{log}");
    assert!(log.contains(&server.host_with_port()), "{log}");
    assert!(!log.contains(TOKEN), "{log}");
}

#[test]
fn remote_remind_prints_due_reminders() {
    let mut server = Server::new();
    let env = Env::new(&server.url());
    let _p = profiles(&mut server);
    let now = Utc::now().fixed_offset();
    let when = now - Duration::minutes(1);
    let reminders = server
        .mock("GET", "/api/v1/profiles/p/reminders")
        .match_query(Matcher::Any)
        .match_request(move |req| {
            let query = req.path_and_query().split_once("since=").map(|(_, v)| v);
            let Some(raw) = query.map(|v| v.split('&').next().unwrap_or_default()) else {
                return false;
            };
            let decoded = raw.replace("%3A", ":").replace("%2B", "+");
            DateTime::parse_from_rfc3339(&decoded)
                .is_ok_and(|since| (since - (now - Duration::hours(12))).abs() <= Duration::seconds(60))
        })
        .with_header("content-type", "application/json")
        .with_body(
            json!({"reminders": [
                {"box": "notes", "at": when.to_rfc3339(), "item": item("toplantı", None)},
                {"box": "notes", "at": (now + Duration::hours(3)).to_rfc3339(), "item": item("sonra", None)},
            ]})
            .to_string(),
        )
        .expect(3)
        .create();
    let out = env.run(&["remind"]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    let time = when.format("%H:%M");
    assert_eq!(text(&out.stdout), format!("{time}  toplantı\n"));
    assert!(env.notifications().is_empty());

    assert_eq!(code(&env.run(&["remind", "--send"])), 0);
    assert_eq!(code(&env.run(&["remind", "--send"])), 0);
    let log = env.notifications();
    assert_eq!(log.matches("⏰ toplantı").count(), 1, "{log}");
    assert!(state_has_reminder(&env.state_dir()));
    reminders.assert();
}

fn refuse(server: &mut ServerGuard, text: &str, status: usize, body: &str, hits: usize) -> Mock {
    server
        .mock("POST", "/api/v1/profiles/p/captures")
        .match_body(Matcher::PartialJson(json!({ "text": text })))
        .with_status(status)
        .with_header("content-type", "application/json")
        .with_body(body)
        .expect(hits)
        .create()
}

fn seeded(text: &str, minute: u32) -> Queued {
    Queued {
        text: text.into(),
        at: at(&format!("2026-11-11T10:{minute:02}:00+03:00")),
        lang: Some("en".into()),
        id: Some(format!("{text}-id")),
    }
}

const INVALID: &str = r#"{"error":{"code":"INVALID","message":"title is empty"}}"#;
const BUSY: &str = r#"{"error":{"code":"IN_PROGRESS","message":"still running"}}"#;

#[test]
fn drain_drops_only_invalid_notes_and_keeps_the_rest_queued() {
    let mut server = Server::new();
    let env = Env::new(&server.url());
    let _p = profiles(&mut server);
    for (i, name) in ["a", "b", "c"].into_iter().enumerate() {
        env.queue().push_entry(&seeded(name, i as u32)).unwrap();
    }
    let a = refuse(&mut server, "a", 422, INVALID, 1);
    let b = refuse(&mut server, "b", 409, BUSY, 1);
    let c = capture_ok(&mut server, Matcher::Any, json!({"text": "c"}), "c");
    let own = capture_ok(&mut server, Matcher::Any, json!({"text": "yeni"}), "yeni");
    let out = env.run(&["capture", "yeni"]);
    assert_eq!(code(&out), 0, "{}", text(&out.stderr));
    for mock in [&a, &b] {
        mock.assert();
    }
    c.assert();
    own.assert();
    assert_eq!(env.queued(), vec![seeded("b", 1)]);
    let log = env.notifications();
    assert!(
        log.contains("Seenboard: not added\ta\ntitle is empty\t"),
        "{log}"
    );
}

#[test]
fn a_server_fault_on_the_first_note_keeps_every_note_in_order() {
    let mut server = Server::new();
    let env = Env::new(&server.url());
    let _p = profiles(&mut server);
    for (i, name) in ["x", "y"].into_iter().enumerate() {
        env.queue().push_entry(&seeded(name, i as u32)).unwrap();
    }
    let x = refuse(
        &mut server,
        "x",
        500,
        r#"{"error":{"code":"INTERNAL","message":"boom"}}"#,
        1,
    );
    let y = refuse(&mut server, "y", 200, "{}", 0);
    let z = refuse(&mut server, "z", 200, "{}", 0);
    let out = env.run(&["capture", "z"]);
    assert_eq!(code(&out), 1);
    x.assert();
    y.assert();
    z.assert();
    let queued = env.queued();
    let texts: Vec<_> = queued.iter().map(|q| q.text.as_str()).collect();
    assert_eq!(texts, ["x", "y", "z"]);
    assert_eq!(queued[0], seeded("x", 0));
    assert_eq!(queued[1], seeded("y", 1));
    let log = env.notifications();
    assert!(log.contains("Seenboard: queued\tz\t"), "{log}");
}

#[test]
fn profile_without_capture_into_keeps_every_note_queued() {
    let mut server = Server::new();
    let env = Env::new(&server.url());
    let _p = profiles_without_capture_into(&mut server);
    env.queue().push_entry(&seeded("a", 0)).unwrap();
    let never = refuse(&mut server, "a", 422, INVALID, 0);
    let out = env.run(&["capture", "yeni"]);
    assert_eq!(code(&out), 1);
    never.assert();
    assert!(
        text(&out.stderr).contains("no capture_into"),
        "{}",
        text(&out.stderr)
    );
    let queued = env.queued();
    let texts: Vec<_> = queued.iter().map(|q| q.text.as_str()).collect();
    assert_eq!(texts, ["a", "yeni"]);
    assert_eq!(queued[0], seeded("a", 0));
    let log = env.notifications();
    assert!(
        log.contains("Seenboard: queued\tyeni\n") && log.contains("no capture_into"),
        "{log}"
    );
}

#[test]
fn shown_time_follows_the_clients_offset() {
    let mut server = Server::new();
    let env = Env::new(&server.url());
    let _p = profiles(&mut server);
    let _m = capture_ok(&mut server, Matcher::Any, json!({"text": "x"}), "x");
    assert_eq!(code(&env.run(&["capture", "x"])), 0);
    let log = env.notifications();
    assert!(log.contains("added\tx · 11:30\t"), "{log}");
}

fn state_has_reminder(state_dir: &Path) -> bool {
    std::fs::read_to_string(state_dir.join("reminded.json"))
        .is_ok_and(|raw| raw.contains("notes:7@"))
}

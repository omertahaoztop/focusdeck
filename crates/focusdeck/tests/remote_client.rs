use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use chrono::{DateTime, Duration, FixedOffset, Local};
use focusdeck::queue::{Queue, Queued};
use focusdeck_protocol::Item;
use mockito::{Matcher, Server, ServerGuard};
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

fn profiles(server: &mut ServerGuard) -> mockito::Mock {
    server
        .mock("GET", "/api/v1/profiles")
        .with_header("content-type", "application/json")
        .with_body(r#"{"profiles":["p"],"default":"p"}"#)
        .create()
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
        Command::new(env!("CARGO_BIN_EXE_focusdeck"))
            .args(args)
            .env("PATH", path)
            .env_remove("FOCUSDECK_LANG")
            .env_remove("FOCUSDECK_PROFILE")
            .env_remove("LANG")
            .env_remove("LC_MESSAGES")
            .env("LC_ALL", "C")
            .env("FOCUSDECK_CONFIG_DIR", root.join("config"))
            .env("FOCUSDECK_DATA_DIR", root.join("data"))
            .env("FOCUSDECK_STATE_DIR", self.state_dir())
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
    assert!(log.contains("log at · 14:30"), "{log}");
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
        log.contains("-u\tcritical\t--\tfocusdeck: not added\ttitle is empty\t"),
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
    assert!(log.contains("focusdeck: queued\tsüt al\t"), "{log}");
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
    let _p = profiles(&mut server);
    let _denied = server
        .mock("POST", "/api/v1/profiles/p/captures")
        .with_status(401)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":{"code":"UNAUTHORIZED","message":"no"}}"#)
        .create();
    let out = env.run(&["capture", "not"]);
    assert_eq!(code(&out), 1);
    assert_eq!(env.queued().len(), 1);
    let log = env.notifications();
    assert!(log.contains("focusdeck: queued"), "{log}");
    assert!(log.contains(&server.host_with_port()), "{log}");
}

#[test]
fn remote_remind_prints_due_reminders() {
    let mut server = Server::new();
    let env = Env::new(&server.url());
    let _p = profiles(&mut server);
    let now = Local::now().fixed_offset();
    let when = now - Duration::minutes(1);
    let reminders = server
        .mock("GET", "/api/v1/profiles/p/reminders")
        .match_query(Matcher::Regex("since=.+".into()))
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

fn state_has_reminder(state_dir: &Path) -> bool {
    std::fs::read_to_string(state_dir.join("reminded.json"))
        .is_ok_and(|raw| raw.contains("notes:7@"))
}

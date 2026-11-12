use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use focusdeck::host::{HostError, PluginHost};
use focusdeck::worker::{self, BoxCmd, BoxEvent};
use focusdeck_protocol::{Item, ListParams, ListResult, View};
use serde_json::{Value, json};
use tokio::sync::mpsc;

fn fake_plugin() -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT
        .get_or_init(|| {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
            let status = Command::new(cargo)
                .args(["build", "-p", "fake-plugin"])
                .current_dir(&workspace)
                .status()
                .expect("cargo build başlatılamadı");
            assert!(status.success(), "fake-plugin derlenemedi");
            let target = match std::env::var_os("CARGO_TARGET_DIR") {
                Some(dir) => workspace.join(dir),
                None => workspace.join("target"),
            };
            target.join("debug/fake-plugin")
        })
        .clone()
}

struct Env {
    dir: tempfile::TempDir,
}

impl Env {
    fn new(mode: &str) -> Env {
        let env = Env {
            dir: tempfile::tempdir().unwrap(),
        };
        env.set_mode(mode);
        env
    }

    fn set_mode(&self, mode: &str) {
        std::fs::write(self.dir.path().join("mode"), mode).unwrap();
    }

    // a wrapper script gives each test its own environment without touching the shared process env
    fn exe(&self) -> PathBuf {
        let script = self.dir.path().join("run.sh");
        let body = format!(
            "#!/bin/sh\nFAKE_MODE_FILE='{}' FAKE_LOG='{}' exec '{}'\n",
            self.dir.path().join("mode").display(),
            self.fake_log().display(),
            fake_plugin().display()
        );
        std::fs::write(&script, body).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    fn fake_log(&self) -> PathBuf {
        self.dir.path().join("fake.log")
    }

    fn log(&self) -> PathBuf {
        self.dir.path().join("logs/sozlu.log")
    }

    fn host(&self) -> PluginHost {
        PluginHost::new(
            "sozlu".into(),
            self.exe(),
            json!({"token": "x"}),
            self.log(),
        )
        .with_timeout(Duration::from_millis(500))
    }

    fn configure_lines(&self) -> usize {
        std::fs::read_to_string(self.fake_log())
            .map(|text| text.lines().filter(|l| l.starts_with("configure")).count())
            .unwrap_or(0)
    }
}

fn summary() -> ListParams {
    ListParams {
        view: View::Summary,
    }
}

#[tokio::test]
async fn lists_items() {
    let env = Env::new("ok");
    let mut host = env.host();
    let result: ListResult = host.call("list", &summary()).await.unwrap();
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].title, "birinci");
}

#[tokio::test]
async fn null_result_is_none() {
    let env = Env::new("ok");
    let mut host = env.host();
    let result: Option<Item> = host
        .call(
            "action",
            &json!({"item": "1", "action": "done", "args": null}),
        )
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn timeout_is_reported() {
    let env = Env::new("hang");
    let mut host = env.host();
    let err = host
        .call::<_, ListResult>("list", &summary())
        .await
        .unwrap_err();
    assert!(matches!(err, HostError::Timeout), "{err:?}");
}

#[tokio::test]
async fn timeout_does_not_leak_stale_answer_into_next_call() {
    let env = Env::new("hang");
    let mut host = env.host();
    assert!(matches!(
        host.call::<_, ListResult>("list", &summary()).await,
        Err(HostError::Timeout)
    ));
    env.set_mode("ok");
    let result: ListResult = host.call("list", &summary()).await.unwrap();
    assert_eq!(result.items.len(), 2);
    assert_eq!(env.configure_lines(), 2);
}

#[tokio::test]
async fn crash_restarts_on_next_call() {
    let env = Env::new("crash");
    let mut host = env.host();
    let err = host
        .call::<_, ListResult>("list", &summary())
        .await
        .unwrap_err();
    assert!(matches!(err, HostError::Crashed), "{err:?}");
    env.set_mode("ok");
    let result: ListResult = host.call("list", &summary()).await.unwrap();
    assert_eq!(result.items.len(), 2);
    assert_eq!(env.configure_lines(), 2);
}

#[tokio::test]
async fn garbage_line_is_error_and_restarts() {
    let env = Env::new("garbage");
    let mut host = env.host();
    let err = host
        .call::<_, ListResult>("list", &summary())
        .await
        .unwrap_err();
    assert!(
        matches!(&err, HostError::BadLine(line) if line == "merhaba"),
        "{err:?}"
    );
    env.set_mode("ok");
    let result: ListResult = host.call("list", &summary()).await.unwrap();
    assert_eq!(result.items.len(), 2);
    assert_eq!(env.configure_lines(), 2);
}

#[tokio::test]
async fn plugin_error_keeps_process() {
    let env = Env::new("ok");
    let mut host = env.host();
    let err = host
        .call::<_, Value>("bilinmeyen", &json!({}))
        .await
        .unwrap_err();
    assert!(
        matches!(&err, HostError::Plugin(m) if m.contains("bilinmeyen")),
        "{err:?}"
    );
    let result: ListResult = host.call("list", &summary()).await.unwrap();
    assert_eq!(result.items.len(), 2);
    assert_eq!(env.configure_lines(), 1);
}

#[tokio::test]
async fn old_protocol_rejected() {
    let env = Env::new("oldproto");
    let mut host = env.host();
    let err = host.describe().await.unwrap_err();
    assert!(
        matches!(
            err,
            HostError::Protocol {
                expected: 1,
                got: 0
            }
        ),
        "{err:?}"
    );
    assert_eq!(env.configure_lines(), 0);
}

#[tokio::test]
async fn describe_configures_once_and_caches() {
    let env = Env::new("ok");
    let mut host = env.host();
    let first = host.describe().await.unwrap();
    assert_eq!(first.name, "fake");
    let second = host.describe().await.unwrap();
    assert_eq!(first, second);
    assert_eq!(env.configure_lines(), 1);
    let logged = std::fs::read_to_string(env.fake_log()).unwrap();
    assert!(logged.contains(r#"{"token":"x"}"#), "{logged}");
}

#[tokio::test]
async fn missing_executable_is_spawn_error() {
    let env = Env::new("ok");
    let mut host = PluginHost::new(
        "yok".into(),
        env.dir.path().join("yok"),
        json!({}),
        env.log(),
    );
    let err = host.describe().await.unwrap_err();
    assert!(matches!(err, HostError::Spawn(_)), "{err:?}");
}

#[tokio::test]
async fn stderr_goes_to_log() {
    let env = Env::new("stderr");
    let mut host = env.host();
    let _: ListResult = host.call("list", &summary()).await.unwrap();
    let mut found = false;
    for _ in 0..100 {
        let text = std::fs::read_to_string(env.log()).unwrap_or_default();
        if text.lines().any(|l| l == "[sozlu] gizli-değil") {
            found = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(found, "log satırı yazılmadı");
}

async fn next_event(rx: &mut mpsc::Receiver<BoxEvent>) -> BoxEvent {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("olay gelmedi")
        .expect("kanal kapandı")
}

#[tokio::test]
async fn worker_emits_listed() {
    let env = Env::new("ok");
    let (cmd_tx, cmd_rx) = mpsc::channel(4);
    let (ev_tx, mut ev_rx) = mpsc::channel(16);
    let handle = worker::spawn(3, env.host(), cmd_rx, ev_tx);
    cmd_tx.send(BoxCmd::Refresh(View::Summary)).await.unwrap();
    match next_event(&mut ev_rx).await {
        BoxEvent::Described { idx, describe } => {
            assert_eq!(idx, 3);
            assert_eq!(describe.name, "fake");
        }
        other => panic!("Described bekleniyordu: {other:?}"),
    }
    match next_event(&mut ev_rx).await {
        BoxEvent::Listed { idx, view, result } => {
            assert_eq!(idx, 3);
            assert_eq!(view, View::Summary);
            assert_eq!(result.unwrap().len(), 2);
        }
        other => panic!("Listed bekleniyordu: {other:?}"),
    }
    drop(cmd_tx);
    handle.await.unwrap();
}

#[tokio::test]
async fn worker_survives_plugin_error() {
    let env = Env::new("crash");
    let (cmd_tx, cmd_rx) = mpsc::channel(4);
    let (ev_tx, mut ev_rx) = mpsc::channel(16);
    let handle = worker::spawn(0, env.host(), cmd_rx, ev_tx);
    cmd_tx.send(BoxCmd::Refresh(View::Summary)).await.unwrap();
    assert!(matches!(
        next_event(&mut ev_rx).await,
        BoxEvent::Described { .. }
    ));
    match next_event(&mut ev_rx).await {
        BoxEvent::Listed { result, .. } => assert_eq!(result.unwrap_err(), "plugin kapandı"),
        other => panic!("Listed bekleniyordu: {other:?}"),
    }
    env.set_mode("ok");
    cmd_tx.send(BoxCmd::Refresh(View::Summary)).await.unwrap();
    match next_event(&mut ev_rx).await {
        BoxEvent::Listed { result, .. } => assert_eq!(result.unwrap().len(), 2),
        other => panic!("Listed bekleniyordu: {other:?}"),
    }
    drop(cmd_tx);
    handle.await.unwrap();
}

#[tokio::test]
async fn worker_runs_action_and_drop() {
    let env = Env::new("ok");
    let (cmd_tx, cmd_rx) = mpsc::channel(4);
    let (ev_tx, mut ev_rx) = mpsc::channel(16);
    let handle = worker::spawn(1, env.host(), cmd_rx, ev_tx);
    assert!(matches!(
        next_event(&mut ev_rx).await,
        BoxEvent::Described { .. }
    ));
    cmd_tx
        .send(BoxCmd::Action {
            item: "1".into(),
            action: "done".into(),
            args: Value::Null,
        })
        .await
        .unwrap();
    match next_event(&mut ev_rx).await {
        BoxEvent::Finished { idx, what, result } => {
            assert_eq!((idx, what.as_str()), (1, "done"));
            assert!(result.is_ok());
        }
        other => panic!("Finished bekleniyordu: {other:?}"),
    }
    let item: Item = serde_json::from_value(json!({"id": "7", "title": "t"})).unwrap();
    cmd_tx
        .send(BoxCmd::Drop { item, from: None })
        .await
        .unwrap();
    match next_event(&mut ev_rx).await {
        BoxEvent::Finished { what, result, .. } => {
            assert_eq!(what, "drop");
            assert!(result.is_ok());
        }
        other => panic!("Finished bekleniyordu: {other:?}"),
    }
    drop(cmd_tx);
    handle.await.unwrap();
}

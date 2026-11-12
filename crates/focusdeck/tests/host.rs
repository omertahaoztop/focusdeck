use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use focusdeck::host::{HostError, PluginHost};
use focusdeck::install::sha256_hex;
use focusdeck::paths::Paths;
use focusdeck::profile::BoxSpec;
use focusdeck::ui::run::build_host;
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
        self.write_script(&script);
        script
    }

    // A child writes the file: a write handle opened here leaks into the forks of other tests and fails the exec with ETXTBSY.
    fn write_script(&self, path: &Path) {
        let body = format!(
            "#!/bin/sh\nFAKE_MODE_FILE='{}' FAKE_LOG='{}' FAKE_LANG_LOG='{}' FAKE_START_LOG='{}' FAKE_PID_LOG='{}' exec '{}'\n",
            self.dir.path().join("mode").display(),
            self.fake_log().display(),
            self.lang_log().display(),
            self.start_log().display(),
            self.pid_log().display(),
            fake_plugin().display()
        );
        let status = Command::new("sh")
            .args(["-c", "printf %s \"$1\" > \"$2\" && chmod 755 \"$2\"", "sh"])
            .arg(body)
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn fake_log(&self) -> PathBuf {
        self.dir.path().join("fake.log")
    }

    fn lang_log(&self) -> PathBuf {
        self.dir.path().join("lang.log")
    }

    fn start_log(&self) -> PathBuf {
        self.dir.path().join("starts.log")
    }

    fn pid_log(&self) -> PathBuf {
        self.dir.path().join("pid")
    }

    fn starts(&self) -> usize {
        std::fs::read_to_string(self.start_log())
            .map(|text| text.lines().count())
            .unwrap_or(0)
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
        .with_timeout(Duration::from_secs(10))
    }

    fn hanging_host(&self) -> PluginHost {
        self.host().with_timeout(Duration::from_millis(500))
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
    let mut host = env.hanging_host();
    let err = host
        .call::<_, ListResult>("list", &summary())
        .await
        .unwrap_err();
    assert!(matches!(err, HostError::Timeout), "{err:?}");
}

#[tokio::test]
async fn timeout_does_not_leak_stale_answer_into_next_call() {
    let env = Env::new("hang");
    let mut host = env.hanging_host();
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

#[tokio::test]
async fn host_passes_requested_language() {
    let env = Env::new("ok");
    let mut host = env.host().with_lang(Some("tr-TR".into()));
    let _: ListResult = host.call("list", &summary()).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(env.lang_log()).unwrap().trim(),
        "tr-TR"
    );
}

#[tokio::test]
async fn protocol_violations_are_reported() {
    // None means the plugin exits without replying
    let cases: [(&str, Option<String>); 7] = [
        ("raw-non-utf8", Some("output is not UTF-8".into())),
        ("raw-non-json", Some("not json".into())),
        ("raw-long-non-json", Some("x".repeat(200))),
        (
            "raw-wrong-id",
            Some(r#"{"id":102,"result":{"items":[]}}"#.into()),
        ),
        (
            "raw-oversize",
            Some("plugin sent a line longer than 16 MiB".into()),
        ),
        (
            "raw-bad-result",
            Some("invalid result: invalid type: integer `5`, expected a sequence".into()),
        ),
        ("raw-exit", None),
    ];
    for (mode, expected) in cases {
        let env = Env::new(mode);
        let mut host = env.host().with_max_line(1024);
        let err = host
            .call::<_, ListResult>("list", &summary())
            .await
            .unwrap_err();
        match expected {
            Some(line) => assert!(
                matches!(&err, HostError::BadLine(got) if *got == line),
                "{mode}: {err:?}"
            ),
            None => assert!(matches!(err, HostError::Crashed), "{mode}: {err:?}"),
        }
        if mode == "raw-oversize" {
            let pid = std::fs::read_to_string(env.pid_log()).unwrap();
            let proc = Path::new("/proc").join(pid.trim());
            for _ in 0..40 {
                if !proc.exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            assert!(!proc.exists(), "the child outlived the over-long line");
        }
        env.set_mode("ok");
        let result: ListResult = host.call("list", &summary()).await.unwrap();
        assert_eq!(result.items.len(), 2, "{mode}");
        assert_eq!(env.starts(), 2, "{mode}");
    }
}

#[tokio::test]
async fn oversized_stderr_line_is_cut_in_the_log() {
    let env = Env::new("raw-stderr-long");
    let mut host = env.host();
    let _: ListResult = host.call("list", &summary()).await.unwrap();
    let cut = format!("[sozlu] {}", "y".repeat(64 * 1024));
    let mut lines = Vec::new();
    for _ in 0..100 {
        let text = std::fs::read_to_string(env.log()).unwrap_or_default();
        lines = text.lines().map(str::to_owned).collect();
        if lines.last().is_some_and(|l| l == "[sozlu] after") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(lines, [cut, "[sozlu] after".to_owned()]);
}

#[tokio::test]
async fn carriage_return_is_stripped_from_stderr_log_lines() {
    let env = Env::new("raw-stderr-cr");
    let mut host = env.host();
    let _: ListResult = host.call("list", &summary()).await.unwrap();
    let mut text = String::new();
    for _ in 0..100 {
        text = std::fs::read_to_string(env.log()).unwrap_or_default();
        if text.ends_with('\n') {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(text, "[sozlu] cr-line\n");
}

#[tokio::test]
async fn failed_describe_reports_once_per_command() {
    let env = Env::new("oldproto");
    let (cmd_tx, cmd_rx) = mpsc::channel(4);
    let (ev_tx, mut ev_rx) = mpsc::channel(16);
    let handle = worker::spawn(0, env.host(), cmd_rx, ev_tx);
    for starts in 1..=2 {
        cmd_tx.send(BoxCmd::Refresh(View::Summary)).await.unwrap();
        match next_event(&mut ev_rx).await {
            BoxEvent::Listed { result, .. } => {
                assert_eq!(result.unwrap_err(), "protocol mismatch: core 1, plugin 0");
            }
            other => panic!("Listed bekleniyordu: {other:?}"),
        }
        assert_eq!(env.starts(), starts);
    }
    cmd_tx
        .send(BoxCmd::Drop {
            item: serde_json::from_value(json!({"id": "7", "title": "t"})).unwrap(),
            from: None,
            capture: true,
        })
        .await
        .unwrap();
    match next_event(&mut ev_rx).await {
        BoxEvent::Finished { what, result, .. } => {
            assert_eq!(what, "capture");
            assert_eq!(result.unwrap_err(), "protocol mismatch: core 1, plugin 0");
        }
        other => panic!("Finished bekleniyordu: {other:?}"),
    }
    assert_eq!(env.starts(), 3);
    drop(cmd_tx);
    handle.await.unwrap();
}

#[tokio::test]
async fn describe_recovers_on_a_later_command() {
    let env = Env::new("oldproto");
    let (cmd_tx, cmd_rx) = mpsc::channel(4);
    let (ev_tx, mut ev_rx) = mpsc::channel(16);
    let handle = worker::spawn(2, env.host(), cmd_rx, ev_tx);
    cmd_tx.send(BoxCmd::Refresh(View::Summary)).await.unwrap();
    assert!(matches!(
        next_event(&mut ev_rx).await,
        BoxEvent::Listed { result: Err(_), .. }
    ));
    env.set_mode("ok");
    cmd_tx.send(BoxCmd::Refresh(View::Summary)).await.unwrap();
    match next_event(&mut ev_rx).await {
        BoxEvent::Described { idx, describe } => {
            assert_eq!((idx, describe.name.as_str()), (2, "fake"));
        }
        other => panic!("Described bekleniyordu: {other:?}"),
    }
    match next_event(&mut ev_rx).await {
        BoxEvent::Listed { result, .. } => assert_eq!(result.unwrap().len(), 2),
        other => panic!("Listed bekleniyordu: {other:?}"),
    }
    assert_eq!(env.starts(), 2);
    drop(cmd_tx);
    handle.await.unwrap();
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
        BoxEvent::Listed { result, .. } => assert_eq!(result.unwrap_err(), "plugin exited"),
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
    for (capture, name) in [(false, "drop"), (true, "capture")] {
        let item: Item = serde_json::from_value(json!({"id": "7", "title": "t"})).unwrap();
        let cmd = BoxCmd::Drop {
            item,
            from: None,
            capture,
        };
        cmd_tx.send(cmd).await.unwrap();
        match next_event(&mut ev_rx).await {
            BoxEvent::Finished { what, result, .. } => {
                assert_eq!(what, name);
                assert!(result.is_ok());
            }
            other => panic!("Finished bekleniyordu: {other:?}"),
        }
    }
    drop(cmd_tx);
    handle.await.unwrap();
}

fn installed_plugin(env: &Env, manifest: Option<&str>) -> (Paths, BoxSpec, String) {
    let paths = Paths {
        config_dir: env.dir.path().join("config"),
        data_dir: env.dir.path().join("data"),
        state_dir: env.dir.path().join("state"),
    };
    let plugins = paths.plugin_dir();
    std::fs::create_dir_all(&plugins).unwrap();
    env.write_script(&plugins.join("fake"));
    let hash = sha256_hex(&std::fs::read(plugins.join("fake")).unwrap());
    if let Some(sha) = manifest {
        let sha = if sha.is_empty() { &hash } else { sha };
        std::fs::write(
            plugins.join("fake.toml"),
            format!("version = \"0.1.0\"\nsha256 = \"{sha}\"\nregistry = \"r\"\n"),
        )
        .unwrap();
    }
    let spec: BoxSpec =
        toml::from_str("id = \"a\"\ntitle = \"A\"\nplugin = \"fake\"\nrow = 0\ncol = 0\n").unwrap();
    (paths, spec, hash)
}

#[tokio::test]
async fn changed_binary_does_not_start() {
    let env = Env::new("ok");
    let (paths, spec, _) = installed_plugin(&env, Some(&"0".repeat(64)));
    let err = build_host(&paths, env.dir.path(), &spec).err().unwrap();
    assert_eq!(
        err,
        "fake no longer matches its manifest; run \"focusdeck plugin install fake\" again"
    );
    let (paths, spec, _) = installed_plugin(&env, Some(""));
    let mut host = build_host(&paths, env.dir.path(), &spec).ok().unwrap();
    assert_eq!(host.describe().await.unwrap().name, "fake");
}

#[tokio::test]
async fn plugin_without_manifest_still_starts() {
    let env = Env::new("ok");
    let (paths, spec, _) = installed_plugin(&env, None);
    let mut host = build_host(&paths, env.dir.path(), &spec).ok().unwrap();
    assert_eq!(host.describe().await.unwrap().name, "fake");
}

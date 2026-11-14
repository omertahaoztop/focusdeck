use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

use chrono::{DateTime, Duration, FixedOffset, Local};
use seenboard::queue::{Queue, Queued};
use seenboard::remind::{self, State};
use seenboard_protocol::{Item, Reminder};

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

fn at(s: &str) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339(s).unwrap()
}

fn item(id: &str, title: &str) -> Item {
    Item {
        id: id.into(),
        title: title.into(),
        subtitle: None,
        url: None,
        due: None,
        overdue: false,
        source: None,
        description: None,
        labels: vec![],
        from: None,
        links: vec![],
    }
}

fn reminder(id: &str, when: &str) -> Reminder {
    Reminder {
        at: at(when),
        item: item(id, "toplantı"),
    }
}

#[test]
fn due_returns_reminders_whose_time_came() {
    let now = at("2026-11-12T10:00:00+03:00");
    let reminders = [reminder("5", "2026-11-12T09:59:00+03:00")];
    let got = remind::due("sozlu", &reminders, now, &State::default());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0, "sozlu:5@2026-11-12T09:59:00+03:00");
}

#[test]
fn due_skips_future_stale_and_already_shown() {
    let now = at("2026-11-12T10:00:00+03:00");
    let reminders = [
        reminder("1", "2026-11-12T10:01:00+03:00"),
        reminder("2", "2026-11-11T21:59:00+03:00"),
        reminder("3", "2026-11-12T09:00:00+03:00"),
    ];
    let mut state = State::default();
    state.0.insert(
        "sozlu:3@2026-11-12T09:00:00+03:00".into(),
        at("2026-11-12T09:00:00+03:00"),
    );
    assert!(remind::due("sozlu", &reminders, now, &state).is_empty());
}

#[test]
fn due_includes_exactly_twelve_hours_old() {
    let now = at("2026-11-12T10:00:00+03:00");
    let reminders = [reminder("1", "2026-11-11T22:00:00+03:00")];
    assert_eq!(
        remind::due("b", &reminders, now, &State::default()).len(),
        1
    );
}

#[test]
fn prune_drops_entries_older_than_seven_days() {
    let now = at("2026-11-12T10:00:00+03:00");
    let mut state = State::default();
    state.0.insert("eski".into(), now - Duration::days(8));
    state.0.insert("yeni".into(), now - Duration::days(6));
    remind::prune(&mut state, now);
    assert_eq!(state.0.keys().collect::<Vec<_>>(), ["yeni"]);
}

#[test]
fn state_round_trips_and_missing_file_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a/b/reminded.json");
    assert_eq!(State::load(&path), State::default());
    let mut state = State::default();
    state.0.insert("k".into(), at("2026-11-12T10:00:00+03:00"));
    state.save(&path).unwrap();
    assert_eq!(State::load(&path), state);
}

struct Env {
    dir: tempfile::TempDir,
}

struct Ran {
    out: Output,
}

impl Ran {
    fn code(&self) -> i32 {
        self.out.status.code().unwrap()
    }

    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.out.stdout).into_owned()
    }

    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.out.stderr).into_owned()
    }
}

impl Env {
    fn new(profile_body: &str) -> Env {
        let env = Env::without_profiles();
        let root = env.dir.path();
        std::fs::create_dir_all(root.join("config/profiles")).unwrap();
        std::fs::write(root.join("config/profiles/p.toml"), profile_body).unwrap();
        env
    }

    fn without_profiles() -> Env {
        let env = Env {
            dir: tempfile::tempdir().unwrap(),
        };
        let root = env.dir.path();
        std::fs::create_dir_all(root.join("data/plugins")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        env.script(
            &root.join("bin/notify-send"),
            &format!(
                "for a in \"$@\"; do printf '%s\\t' \"$a\"; done >> '{0}'\necho >> '{0}'\n[ ! -e '{1}' ]",
                env.notify_log().display(),
                root.join("notify-fail").display()
            ),
        );
        env.set_mode("ok");
        env.install_plugin("vikunja");
        env
    }

    fn standard() -> Env {
        Env::new(STANDARD_PROFILE)
    }

    fn script(&self, path: &Path, body: &str) {
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn install_plugin(&self, name: &str) {
        let root = self.dir.path();
        self.script(
            &root.join("data/plugins").join(name),
            &format!(
                "FAKE_MODE_FILE='{}' FAKE_LOG='{}' FAKE_REMINDERS_FILE='{}' exec '{}'",
                root.join("mode").display(),
                self.fake_log().display(),
                root.join("reminders.json").display(),
                fake_plugin().display()
            ),
        );
    }

    fn set_mode(&self, mode: &str) {
        std::fs::write(self.dir.path().join("mode"), mode).unwrap();
    }

    fn set_reminders(&self, reminders: &[Reminder]) {
        std::fs::write(
            self.dir.path().join("reminders.json"),
            serde_json::to_string(reminders).unwrap(),
        )
        .unwrap();
    }

    fn fake_log(&self) -> PathBuf {
        self.dir.path().join("fake.log")
    }

    fn notify_log(&self) -> PathBuf {
        self.dir.path().join("notify.log")
    }

    fn state_dir(&self) -> PathBuf {
        self.dir.path().join("state")
    }

    fn queue(&self) -> Queue {
        Queue::new(self.state_dir().join("capture-queue.jsonl"))
    }

    fn write_config(&self, body: &str) {
        std::fs::write(self.dir.path().join("config/config.toml"), body).unwrap();
    }

    fn drops(&self) -> Vec<Item> {
        std::fs::read_to_string(self.fake_log())
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.strip_prefix("drop "))
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn notifications(&self) -> Vec<Vec<String>> {
        std::fs::read_to_string(self.notify_log())
            .unwrap_or_default()
            .lines()
            .map(|l| {
                l.split('\t')
                    .filter(|a| !a.is_empty())
                    .map(String::from)
                    .collect()
            })
            .collect()
    }

    fn run(&self, args: &[&str]) -> Ran {
        self.run_in(args, "C")
    }

    fn run_in(&self, args: &[&str], locale: &str) -> Ran {
        self.run_with_profile(args, locale, Some("p"))
    }

    fn run_with_profile(&self, args: &[&str], locale: &str, profile: Option<&str>) -> Ran {
        let out = self.command(args, locale, profile).output().unwrap();
        Ran { out }
    }

    fn command(&self, args: &[&str], locale: &str, profile: Option<&str>) -> Command {
        let root = self.dir.path();
        let path = format!(
            "{}:{}",
            root.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new(env!("CARGO_BIN_EXE_seenboard"));
        match profile {
            Some(name) => command.env("SEENBOARD_PROFILE", name),
            None => command.env_remove("SEENBOARD_PROFILE"),
        };
        command
            .args(args)
            .env("PATH", path)
            .env_remove("SEENBOARD_LANG")
            .env_remove("LANG")
            .env_remove("LC_MESSAGES")
            .env("LC_ALL", locale)
            .env("SEENBOARD_CONFIG_DIR", root.join("config"))
            .env("SEENBOARD_DATA_DIR", root.join("data"))
            .env("SEENBOARD_STATE_DIR", self.state_dir());
        command
    }

    fn queued(&self) -> Vec<Queued> {
        let queue = self.queue();
        let taken = queue.take().unwrap();
        let items = taken.items.clone();
        queue.commit(taken, &[]).unwrap();
        items
    }

    fn wait_for_fake_log(&self, needle: &str) {
        for _ in 0..200 {
            if std::fs::read_to_string(self.fake_log()).is_ok_and(|log| log.contains(needle)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("the fake plugin never logged {needle:?}");
    }

    fn spawn_capture(&self, text: &str) -> std::process::Child {
        self.command(&["capture", text], "C", Some("p"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap()
    }
}

fn signal(child: &std::process::Child, name: &str) {
    let status = Command::new("kill")
        .arg(format!("-{name}"))
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(status.success());
}

const STANDARD_PROFILE: &str = r#"
name = "p"
capture_into = "sozlu"

[[box]]
id = "sozlu"
title = "Sözlü"
plugin = "vikunja"
row = 0
col = 0
"#;

#[test]
fn capture_with_time_drops_and_notifies() {
    let env = Env::standard();
    let ran = env.run(&["capture", "log at 14:30"]);
    assert_eq!(ran.code(), 0, "{}", ran.stderr());
    let drops = env.drops();
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].title, "log at");
    let due = drops[0].due.unwrap();
    assert_eq!(due.format("%H:%M:%S").to_string(), "14:30:00");
    assert_eq!(env.notifications(), [["--", "added", "log at · 14:30"]]);
    assert!(env.queued().is_empty());
}

#[test]
fn capture_without_time_shows_only_the_title() {
    let env = Env::standard();
    assert_eq!(env.run(&["capture", "süt", "al"]).code(), 0);
    assert_eq!(env.notifications(), [["--", "added", "süt al"]]);
}

#[test]
fn parse_error_is_critical_and_queues_nothing() {
    let env = Env::standard();
    let ran = env.run(&["capture", "@ali 14:30"]);
    assert_eq!(ran.code(), 1);
    assert_eq!(
        env.notifications(),
        [[
            "-u",
            "critical",
            "--",
            "Seenboard: not added",
            "title is empty"
        ]]
    );
    assert!(env.drops().is_empty());
    assert!(env.queued().is_empty());
}

#[test]
fn crashing_plugin_queues_the_note() {
    let env = Env::standard();
    env.set_mode("crash");
    let ran = env.run(&["capture", "log at"]);
    assert_eq!(ran.code(), 1);
    let queued = env.queued();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].text, "log at");
    assert_eq!(queued[0].lang.as_deref(), Some("en"));
    let notes = env.notifications();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0][..4], ["-u", "critical", "--", "Seenboard: queued"]);
}

#[test]
fn missing_plugin_binary_queues_the_note() {
    let env = Env::standard();
    std::fs::remove_file(env.dir.path().join("data/plugins/vikunja")).unwrap();
    assert_eq!(env.run(&["capture", "log at"]).code(), 1);
    assert_eq!(env.queued().len(), 1);
}

#[test]
fn unusable_state_dir_still_delivers_the_note() {
    let env = Env::standard();
    std::fs::write(env.state_dir(), "").unwrap();
    let ran = env.run(&["capture", "log at"]);
    assert_eq!(ran.code(), 0, "{}", ran.stderr());
    assert_eq!(env.drops().len(), 1);
    assert_eq!(env.notifications(), [["--", "added", "log at"]]);
}

#[test]
fn unusable_state_dir_and_a_crashing_plugin_report_the_loss() {
    let env = Env::standard();
    std::fs::write(env.state_dir(), "").unwrap();
    env.set_mode("crash");
    assert_eq!(env.run(&["capture", "log at"]).code(), 1);
    let notes = env.notifications();
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0][..4],
        ["-u", "critical", "--", "Seenboard: note may be lost"]
    );
}

#[test]
fn successful_capture_flushes_the_queue_first() {
    let env = Env::standard();
    let q = env.queue();
    q.push("eski bir", at("2026-11-11T10:00:00+03:00")).unwrap();
    q.push("eski iki 09:30", at("2026-11-11T10:01:00+03:00"))
        .unwrap();
    let ran = env.run(&["capture", "yeni"]);
    assert_eq!(ran.code(), 0, "{}", ran.stderr());
    let titles: Vec<_> = env.drops().into_iter().map(|i| i.title).collect();
    assert_eq!(titles, ["eski bir", "eski iki", "yeni"]);
    assert!(!env.state_dir().join("capture-queue.jsonl").exists());
    let raw = std::fs::read_to_string(env.notify_log()).unwrap();
    assert!(
        raw.contains("added\tyeni\n2 notes sent from the queue\t"),
        "{raw}"
    );
}

#[test]
fn queued_note_keeps_its_original_time() {
    let env = Env::standard();
    env.queue()
        .push("eski 30min", at("2026-11-11T10:00:00+03:00"))
        .unwrap();
    assert_eq!(env.run(&["capture", "yeni"]).code(), 0);
    let drops = env.drops();
    assert_eq!(drops[0].due, Some(at("2026-11-11T10:30:00+03:00")));
}

#[test]
fn queued_note_parses_in_its_own_language() {
    let env = Env::standard();
    env.queue()
        .push_entry(&Queued {
            text: "ara yarın 10:00".into(),
            at: at("2026-11-11T10:00:00+03:00"),
            lang: Some("tr".into()),
            id: None,
        })
        .unwrap();
    let ran = env.run(&["capture", "yeni"]);
    assert_eq!(ran.code(), 0, "{}", ran.stderr());
    let drops = env.drops();
    assert_eq!(drops[0].title, "ara");
    assert_eq!(drops[0].due, Some(at("2026-11-12T10:00:00+03:00")));
}

#[test]
fn failed_flush_keeps_queue_and_new_note() {
    let env = Env::standard();
    env.queue()
        .push("eski", at("2026-11-11T10:00:00+03:00"))
        .unwrap();
    env.set_mode("crash");
    assert_eq!(env.run(&["capture", "yeni"]).code(), 1);
    let texts: Vec<_> = env
        .queued()
        .into_iter()
        .map(|Queued { text, .. }| text)
        .collect();
    assert_eq!(texts, ["eski", "yeni"]);
}

#[test]
fn unparsable_queued_note_is_not_counted_as_sent() {
    let env = Env::standard();
    let q = env.queue();
    for (text, when) in [
        ("@ali 14:30", "2026-11-11T10:00:00+03:00"),
        ("eski", "2026-11-11T10:01:00+03:00"),
    ] {
        q.push_entry(&Queued {
            text: text.into(),
            at: at(when),
            lang: Some("en".into()),
            id: None,
        })
        .unwrap();
    }
    assert_eq!(env.run(&["capture", "yeni"]).code(), 0);
    let titles: Vec<_> = env.drops().into_iter().map(|i| i.title).collect();
    assert_eq!(titles, ["eski", "yeni"]);
    let raw = std::fs::read_to_string(env.notify_log()).unwrap();
    assert!(
        raw.contains("added\tyeni\n1 note sent from the queue\t"),
        "{raw}"
    );
}

#[test]
fn partial_flush_requeues_only_the_unsent_notes() {
    let env = Env::standard();
    let q = env.queue();
    q.push("bir", at("2026-11-11T10:00:00+03:00")).unwrap();
    q.push("iki", at("2026-11-11T10:01:00+03:00")).unwrap();
    env.set_mode("crash-on:iki");
    assert_eq!(env.run(&["capture", "yeni"]).code(), 1);
    let sent: Vec<_> = env.drops().into_iter().map(|i| i.title).collect();
    assert_eq!(sent, ["bir"]);
    let left: Vec<_> = env.queued().into_iter().map(|q| q.text).collect();
    assert_eq!(left, ["iki", "yeni"]);
}

#[test]
fn sigterm_during_delivery_keeps_every_note() {
    let env = Env::standard();
    env.set_mode("slow-drop");
    let mut child = env.spawn_capture("log at");
    env.wait_for_fake_log("waiting log at");
    signal(&child, "TERM");
    assert_eq!(child.wait().unwrap().code(), Some(1));
    assert_eq!(
        env.notifications(),
        [[
            "-u",
            "critical",
            "--",
            "Seenboard: queued",
            "interrupted before the note was sent"
        ]]
    );
    let raw = std::fs::read_to_string(env.state_dir().join("capture-queue.jsonl")).unwrap();
    assert_eq!(raw.lines().count(), 1, "{raw}");
    assert!(raw.contains("\"text\":\"log at\""), "{raw}");

    env.set_mode("ok");
    assert_eq!(env.run(&["capture", "sonra"]).code(), 0);
    let titles: Vec<_> = env.drops().into_iter().map(|i| i.title).collect();
    assert_eq!(titles, ["log at", "sonra"]);
}

#[test]
fn sigkill_during_delivery_loses_nothing() {
    let env = Env::standard();
    env.set_mode("slow-drop");
    let mut child = env.spawn_capture("log at");
    env.wait_for_fake_log("waiting log at");
    signal(&child, "KILL");
    assert_eq!(child.wait().unwrap().code(), None);

    env.set_mode("ok");
    assert_eq!(env.run(&["capture", "sonra"]).code(), 0);
    let titles: Vec<_> = env.drops().into_iter().map(|i| i.title).collect();
    assert_eq!(titles, ["log at", "sonra"]);
}

#[test]
fn leading_dash_note_is_not_an_option() {
    let env = Env::standard();
    assert_eq!(env.run(&["capture", "--", "-u low not"]).code(), 0);
    assert_eq!(env.notifications(), [["--", "added", "-u low not"]]);
}

#[test]
fn profile_without_capture_into_fails() {
    let env = Env::new("name = \"p\"\n");
    let ran = env.run(&["capture", "x"]);
    assert_eq!(ran.code(), 1);
    assert_eq!(ran.stderr().trim(), "this profile has no capture_into");
    assert!(env.notifications().is_empty());
}

#[test]
fn empty_prompt_answer_is_silent() {
    let env = Env::standard();
    env.write_config("capture_command = [\"echo\", \"\"]\n");
    let ran = env.run(&["capture"]);
    assert_eq!(ran.code(), 0);
    assert!(env.drops().is_empty());
    assert!(env.notifications().is_empty());
}

#[test]
fn prompt_answer_first_line_is_captured() {
    let env = Env::standard();
    env.write_config("capture_command = [\"printf\", \"  prompttan not  \\nikinci\\n\"]\n");
    assert_eq!(env.run(&["capture"]).code(), 0);
    let titles: Vec<_> = env.drops().into_iter().map(|i| i.title).collect();
    assert_eq!(titles, ["prompttan not"]);
}

#[test]
fn remind_lists_without_notifying_or_saving_state() {
    let env = Env::standard();
    let now = Local::now().fixed_offset();
    env.set_reminders(&[Reminder {
        at: now - Duration::minutes(1),
        item: item("5", "toplantı"),
    }]);
    let ran = env.run(&["remind"]);
    assert_eq!(ran.code(), 0, "{}", ran.stderr());
    let time = (now - Duration::minutes(1)).format("%H:%M").to_string();
    assert_eq!(ran.stdout(), format!("{time}  toplantı\n"));
    assert!(env.notifications().is_empty());
    assert!(!env.state_dir().join("reminded.json").exists());
}

#[test]
fn remind_send_notifies_once() {
    let env = Env::standard();
    let now = Local::now().fixed_offset();
    let when = now - Duration::minutes(1);
    env.set_reminders(&[
        Reminder {
            at: when,
            item: item("5", "toplantı"),
        },
        Reminder {
            at: now + Duration::hours(1),
            item: item("6", "sonra"),
        },
    ]);
    assert_eq!(env.run(&["remind", "--send"]).code(), 0);
    let time = when.format("%H:%M").to_string();
    assert_eq!(
        env.notifications(),
        [[
            "-u".to_string(),
            "critical".into(),
            "--".into(),
            "⏰ toplantı".into(),
            time
        ]]
    );
    let state = State::load(&env.state_dir().join("reminded.json"));
    assert_eq!(state.0.len(), 1);
    assert!(state.0.keys().next().unwrap().starts_with("sozlu:5@"));

    assert_eq!(env.run(&["remind", "--send"]).code(), 0);
    assert_eq!(env.notifications().len(), 1);
    assert_eq!(env.run(&["remind"]).stdout(), "");
}

#[test]
fn remind_send_retries_after_failed_notification() {
    let env = Env::standard();
    let when = Local::now().fixed_offset() - Duration::minutes(1);
    env.set_reminders(&[Reminder {
        at: when,
        item: item("5", "toplantı"),
    }]);
    std::fs::write(env.dir.path().join("notify-fail"), "").unwrap();
    assert_eq!(env.run(&["remind", "--send"]).code(), 0);
    assert_eq!(env.notifications().len(), 1);
    let state_path = env.state_dir().join("reminded.json");
    assert!(State::load(&state_path).0.is_empty());

    std::fs::remove_file(env.dir.path().join("notify-fail")).unwrap();
    assert_eq!(env.run(&["remind", "--send"]).code(), 0);
    assert_eq!(env.notifications().len(), 2);
    assert_eq!(State::load(&state_path).0.len(), 1);

    assert_eq!(env.run(&["remind", "--send"]).code(), 0);
    assert_eq!(env.notifications().len(), 2);
}

#[test]
fn remind_output_strips_control_characters_from_titles() {
    let env = Env::standard();
    let when = Local::now().fixed_offset() - Duration::minutes(1);
    env.set_reminders(&[Reminder {
        at: when,
        item: item("5", "a\x1b[31mkırmızı\nsatır\r"),
    }]);
    let ran = env.run(&["remind"]);
    assert_eq!(ran.code(), 0, "{}", ran.stderr());
    let out = ran.stdout();
    let line = out.strip_suffix('\n').unwrap();
    assert!(line.ends_with("a?[31mkırmızı?satır?"), "{line:?}");
    assert!(!line.chars().any(char::is_control));
}

#[test]
fn remind_skips_failing_and_uninstalled_boxes() {
    let env = Env::new(
        r#"
name = "p"

[[box]]
id = "a"
title = "A"
plugin = "yok"
row = 0
col = 0

[[box]]
id = "b"
title = "B"
plugin = "vikunja"
row = 0
col = 1
"#,
    );
    let now = Local::now().fixed_offset();
    env.set_reminders(&[Reminder {
        at: now - Duration::minutes(1),
        item: item("5", "toplantı"),
    }]);
    let ran = env.run(&["remind"]);
    assert_eq!(ran.code(), 0, "{}", ran.stderr());
    assert!(ran.stdout().ends_with("  toplantı\n"));
}

#[test]
fn remind_logs_a_failing_box_and_still_shows_the_others() {
    let env = Env::new(
        r#"
name = "p"

[[box]]
id = "a"
title = "A"
plugin = "broken"
row = 0
col = 0

[[box]]
id = "b"
title = "B"
plugin = "vikunja"
row = 0
col = 1
"#,
    );
    let root = env.dir.path();
    env.script(
        &root.join("data/plugins/broken"),
        &format!(
            "FAKE_MODE=fail-reminders FAKE_REMINDERS_FILE='{}' exec '{}'",
            root.join("reminders.json").display(),
            fake_plugin().display()
        ),
    );
    env.set_reminders(&[Reminder {
        at: Local::now().fixed_offset() - Duration::minutes(1),
        item: item("5", "toplantı"),
    }]);
    let ran = env.run(&["remind"]);
    assert_eq!(ran.code(), 0, "{}", ran.stderr());
    assert!(ran.stdout().ends_with("  toplantı\n"), "{}", ran.stdout());
    let log = std::fs::read_to_string(env.state_dir().join("seenboard.log")).unwrap();
    assert!(log.contains("a: could not fetch reminders: "), "{log}");
}

#[test]
fn remind_output_is_sorted_by_time_across_boxes() {
    let env = Env::new(
        r#"
name = "p"

[[box]]
id = "a"
title = "A"
plugin = "late"
row = 0
col = 0

[[box]]
id = "b"
title = "B"
plugin = "vikunja"
row = 0
col = 1
"#,
    );
    let root = env.dir.path();
    let now = Local::now().fixed_offset();
    std::fs::write(
        root.join("late.json"),
        serde_json::to_string(&[Reminder {
            at: now - Duration::minutes(1),
            item: item("1", "geç"),
        }])
        .unwrap(),
    )
    .unwrap();
    env.script(
        &root.join("data/plugins/late"),
        &format!(
            "FAKE_REMINDERS_FILE='{}' exec '{}'",
            root.join("late.json").display(),
            fake_plugin().display()
        ),
    );
    env.set_reminders(&[Reminder {
        at: now - Duration::minutes(30),
        item: item("2", "erken"),
    }]);
    let ran = env.run(&["remind"]);
    let titles: Vec<_> = ran
        .stdout()
        .lines()
        .map(|l| l.split("  ").nth(1).unwrap().to_owned())
        .collect();
    assert_eq!(titles, ["erken", "geç"]);
}

#[test]
fn concurrent_remind_runs_notify_once() {
    let env = Env::standard();
    let root = env.dir.path();
    env.script(
        &root.join("bin/notify-send"),
        &format!(
            "sleep 0.3\nfor a in \"$@\"; do printf '%s\\t' \"$a\"; done >> '{0}'\necho >> '{0}'",
            env.notify_log().display()
        ),
    );
    env.set_reminders(&[Reminder {
        at: Local::now().fixed_offset() - Duration::minutes(1),
        item: item("5", "toplantı"),
    }]);
    let children: Vec<_> = (0..4)
        .map(|_| {
            env.command(&["remind", "--send"], "C", Some("p"))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    assert_eq!(env.notifications().len(), 1);
    assert_eq!(
        State::load(&env.state_dir().join("reminded.json")).0.len(),
        1
    );
}

#[test]
fn turkish_locale_keeps_turkish_output() {
    let env = Env::new("name = \"p\"\n");
    let ran = env.run_in(&["capture", "x"], "tr_TR.UTF-8");
    assert_eq!(ran.code(), 1);
    assert_eq!(ran.stderr().trim(), "bu profilde capture_into yok");
}

#[test]
fn turkish_locale_keeps_turkish_notifications() {
    let env = Env::standard();
    assert_eq!(
        env.run_in(&["capture", "@ali 14:30"], "tr_TR.UTF-8").code(),
        1
    );
    assert_eq!(
        env.notifications(),
        [["-u", "critical", "--", "Seenboard: eklenmedi", "başlık boş"]]
    );
    assert_eq!(env.run_in(&["capture", "yeni"], "tr_TR.UTF-8").code(), 0);
    assert_eq!(env.notifications()[1], ["--", "eklendi", "yeni"]);
}

#[test]
fn config_language_overrides_env() {
    let env = Env::new("name = \"p\"\n");
    env.write_config("language = \"tr\"\n");
    let ran = env.run(&["capture", "x"]);
    assert_eq!(ran.code(), 1);
    assert_eq!(ran.stderr().trim(), "bu profilde capture_into yok");
}

#[test]
fn help_is_localized() {
    let env = Env::standard();
    assert!(env.run(&["--help"]).stdout().contains("Focus board"));
    let tr = env.run_in(&["--help"], "tr_TR.UTF-8").stdout();
    assert!(tr.contains("Odak panosu"), "{tr}");
    assert!(tr.contains("Kullanılacak profil adı"), "{tr}");
}

#[test]
fn errors_carry_a_localized_prefix() {
    let env = Env::standard();
    let en = env.run(&["--profile", "yok", "remind"]);
    assert_eq!(en.code(), 1);
    assert!(en.stderr().starts_with("error: "), "{}", en.stderr());
    let tr = env.run_in(&["--profile", "yok", "remind"], "tr_TR.UTF-8");
    assert!(tr.stderr().starts_with("hata: "), "{}", tr.stderr());
    assert!(tr.stderr().contains("okunamadı"), "{}", tr.stderr());
}

const LOCAL_PROFILE: &str = r#"
name = "p"
capture_into = "notes"

[[box]]
id = "notes"
title = "Notes"
plugin = "local"
row = 0
col = 0
"#;

fn local_notes(env: &Env) -> serde_json::Value {
    let raw = std::fs::read_to_string(env.dir.path().join("data/local.json")).unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn plugin_list_names_local_as_built_in() {
    let env = Env::standard();
    assert_eq!(env.run(&["plugin", "list"]).stdout(), "local  (built-in)\n");
    let tr = env.run_in(&["plugin", "list"], "tr_TR.UTF-8");
    assert_eq!(tr.stdout(), "local  (yerleşik)\n");
}

#[test]
fn plugin_install_local_is_refused_in_turkish() {
    let env = Env::standard();
    let ran = env.run_in(&["plugin", "install", "local"], "tr_TR.UTF-8");
    assert_eq!(ran.code(), 1);
    assert!(
        ran.stderr()
            .contains("local Seenboard ile birlikte gelir, kurulacak bir şey yok"),
        "{}",
        ran.stderr()
    );
    let ran = env.run_in(&["plugin", "remove", "local"], "tr_TR.UTF-8");
    assert_eq!(ran.code(), 1);
    assert!(
        ran.stderr()
            .contains("local Seenboard ile birlikte gelir, kaldırılamaz"),
        "{}",
        ran.stderr()
    );
}

#[test]
fn capture_into_a_local_box_needs_no_plugin() {
    let env = Env::new(LOCAL_PROFILE);
    std::fs::remove_file(env.dir.path().join("data/plugins/vikunja")).unwrap();
    let ran = env.run(&["capture", "süt al #ev"]);
    assert_eq!(ran.code(), 0, "{}", ran.stderr());
    let notes = local_notes(&env);
    let items = notes["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["title"], "süt al");
    assert_eq!(items[0]["labels"], serde_json::json!(["ev"]));
    assert_eq!(items[0]["list"], "notes");
}

#[test]
fn remind_sends_due_local_notes() {
    let env = Env::new(LOCAL_PROFILE);
    let due = Local::now().fixed_offset() - Duration::minutes(5);
    let note = serde_json::json!({
        "version": 1,
        "items": [{
            "id": "n1",
            "list": "notes",
            "title": "ilaç al",
            "due": due.to_rfc3339(),
            "position": 1.0,
            "created": due.to_rfc3339(),
        }],
    });
    std::fs::write(env.dir.path().join("data/local.json"), note.to_string()).unwrap();
    let ran = env.run(&["remind", "--send"]);
    assert_eq!(ran.code(), 0, "{}", ran.stderr());
    let sent = env.notifications();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].contains(&"⏰ ilaç al".to_string()), "{sent:?}");
}

#[test]
fn serve_local_is_hidden_from_help() {
    let env = Env::standard();
    let help = env.run(&["--help"]).stdout();
    assert!(help.contains("Focus board"), "{help}");
    assert!(!help.contains("serve-local"), "{help}");
}

#[test]
fn first_capture_creates_the_starter_profile() {
    let env = Env::without_profiles();
    let ran = env.run_with_profile(&["capture", "ilk not"], "tr_TR.UTF-8", None);
    assert_eq!(ran.code(), 0, "{}", ran.stderr());
    let root = env.dir.path();
    let profile = std::fs::read_to_string(root.join("config/profiles/notes.toml")).unwrap();
    assert!(profile.contains("Notlar"), "{profile}");
    let notes = local_notes(&env);
    assert_eq!(notes["items"][0]["title"], "ilk not");
    assert!(!root.join("config/config.toml").exists());
}

#[test]
fn two_profiles_without_a_choice_still_error() {
    let env = Env::without_profiles();
    let profiles = env.dir.path().join("config/profiles");
    std::fs::create_dir_all(&profiles).unwrap();
    for name in ["is", "ev"] {
        std::fs::write(profiles.join(format!("{name}.toml")), STANDARD_PROFILE).unwrap();
    }
    let ran = env.run_with_profile(&["capture", "x"], "C", None);
    assert_eq!(ran.code(), 1);
    assert!(
        ran.stderr().contains("no profile selected"),
        "{}",
        ran.stderr()
    );
    assert!(!profiles.join("notes.toml").exists());
}

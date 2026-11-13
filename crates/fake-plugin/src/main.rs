//! Test plugin driven by FAKE_MODE_FILE or FAKE_MODE (raw-* modes break the protocol); writes FAKE_LOG, FAKE_LANG_LOG, FAKE_START_LOG, FAKE_PID_LOG; reminders from FAKE_REMINDERS_FILE, list items from FAKE_ITEMS_FILE.

use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::time::Duration;

use focusdeck_protocol::{
    ActionParams, Describe, DropParams, Item, ListParams, PROTOCOL_VERSION, Plugin, Reminder,
    RemindersParams, serve,
};
use serde_json::{Value, json};

struct Fake;

fn mode() -> String {
    if let Ok(path) = std::env::var("FAKE_MODE_FILE")
        && let Ok(text) = std::fs::read_to_string(path)
    {
        return text.trim().to_string();
    }
    std::env::var("FAKE_MODE").unwrap_or_else(|_| "ok".into())
}

fn log_line(line: &str) {
    if let Ok(path) = std::env::var("FAKE_LOG")
        && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path)
    {
        let _ = writeln!(file, "{line}");
    }
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

impl Plugin for Fake {
    fn describe(&self) -> Describe {
        Describe {
            name: "fake".into(),
            version: "0.1.0".into(),
            protocol: if mode() == "oldproto" {
                0
            } else {
                PROTOCOL_VERSION
            },
            actions: vec!["done".into()],
            accepts_drop: true,
            reminders: std::env::var_os("FAKE_REMINDERS_FILE").is_some(),
        }
    }

    fn configure(&mut self, config: Value) -> Result<(), String> {
        if let Ok(path) = std::env::var("FAKE_LOG") {
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| e.to_string())?;
            writeln!(file, "configure {config}").map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn list(&mut self, _p: ListParams) -> Result<Vec<Item>, String> {
        log_line("list");
        match mode().as_str() {
            "empty" => Ok(vec![]),
            _ if std::env::var_os("FAKE_ITEMS_FILE").is_some() => {
                let path = std::env::var("FAKE_ITEMS_FILE").unwrap_or_default();
                let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
                serde_json::from_str(&text).map_err(|e| e.to_string())
            }
            "hang" => {
                std::thread::sleep(Duration::from_secs(60));
                Ok(vec![])
            }
            "crash" => std::process::exit(3),
            "garbage" => {
                let mut out = io::stdout();
                let _ = writeln!(out, "merhaba");
                let _ = out.flush();
                std::process::exit(0);
            }
            "stderr" => {
                eprintln!("gizli-değil");
                Ok(vec![item("1", "birinci")])
            }
            _ => Ok(vec![item("1", "birinci"), item("2", "ikinci")]),
        }
    }

    fn action(&mut self, p: ActionParams) -> Result<Option<Item>, String> {
        if mode() == "slow-action" {
            log_line("action-start");
            std::thread::sleep(Duration::from_millis(300));
            log_line("action-end");
        }
        log_line(&format!("action {} {}", p.item, p.action));
        Ok(None)
    }

    fn reminders(&mut self, _p: RemindersParams) -> Result<Vec<Reminder>, String> {
        if mode() == "fail-reminders" {
            return Err("reminders are unavailable".into());
        }
        let Ok(path) = std::env::var("FAKE_REMINDERS_FILE") else {
            return Ok(vec![]);
        };
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| e.to_string()),
            Err(_) => Ok(vec![]),
        }
    }

    fn drop_item(&mut self, p: DropParams) -> Result<Item, String> {
        let mode = mode();
        if mode == "crash" || mode.strip_prefix("crash-on:") == Some(p.item.title.as_str()) {
            std::process::exit(3);
        }
        if mode == "slow-drop" {
            if let Ok(path) = std::env::var("FAKE_LOG")
                && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path)
            {
                let _ = writeln!(file, "waiting {}", p.item.title);
            }
            std::thread::sleep(Duration::from_secs(20));
        }
        if let Ok(path) = std::env::var("FAKE_LOG")
            && let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path)
        {
            let _ = writeln!(
                file,
                "drop {}",
                serde_json::to_string(&p.item).unwrap_or_default()
            );
        }
        Ok(Item {
            id: "yeni".into(),
            ..p.item
        })
    }
}

fn main() -> io::Result<()> {
    if let Ok(path) = std::env::var("FAKE_LANG_LOG") {
        let lang = std::env::var("FOCUSDECK_LANG").unwrap_or_else(|_| "-".into());
        std::fs::write(path, lang)?;
    }
    if let Ok(path) = std::env::var("FAKE_START_LOG") {
        writeln!(
            OpenOptions::new().create(true).append(true).open(path)?,
            "start"
        )?;
    }
    let mode = mode();
    if mode.starts_with("raw-") {
        return raw(&mode);
    }
    serve(Fake, io::stdin().lock(), io::stdout())
}

// raw-* modes: answers describe and configure like serve does, then breaks the protocol on the first other request
fn raw(mode: &str) -> io::Result<()> {
    let mut out = io::stdout();
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line?).map_err(io::Error::other)?;
        let id = request["id"].as_u64().unwrap_or(0);
        let reply = match request["method"].as_str() {
            Some("describe") => json!({"id": id, "result": Fake.describe()}),
            Some("configure") => json!({"id": id, "result": null}),
            _ => return misbehave(mode, id),
        };
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    Ok(())
}

fn misbehave(mode: &str, id: u64) -> io::Result<()> {
    let mut out = io::stdout();
    match mode {
        "raw-non-utf8" => out.write_all(b"\xff\xfe\n")?,
        "raw-non-json" => writeln!(out, "not json")?,
        "raw-long-non-json" => writeln!(out, "{}", "x".repeat(300))?,
        "raw-wrong-id" => writeln!(out, "{}", json!({"id": id + 99, "result": {"items": []}}))?,
        "raw-oversize" => {
            if let Ok(path) = std::env::var("FAKE_PID_LOG") {
                std::fs::write(path, std::process::id().to_string())?;
            }
            writeln!(out, "{}", "x".repeat(4096))?;
            out.flush()?;
            // stays alive so only the host can end it
            std::thread::sleep(Duration::from_secs(60));
        }
        "raw-stderr-cr" => {
            write!(io::stderr(), "cr-line\r\n")?;
            writeln!(out, "{}", json!({"id": id, "result": {"items": []}}))?;
        }
        "raw-bad-result" => writeln!(out, "{}", json!({"id": id, "result": {"items": 5}}))?,
        "raw-stderr-long" => {
            let mut err = io::stderr();
            writeln!(err, "{}", "y".repeat(100 * 1024))?;
            writeln!(err, "after")?;
            writeln!(out, "{}", json!({"id": id, "result": {"items": []}}))?;
        }
        _ => {}
    }
    out.flush()
}

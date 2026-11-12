//! Test plugin whose behaviour comes from FAKE_MODE_FILE or FAKE_MODE; configure and drop calls are appended to FAKE_LOG, reminders come from FAKE_REMINDERS_FILE.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::time::Duration;

use focusdeck_protocol::{
    ActionParams, Describe, DropParams, Item, ListParams, PROTOCOL_VERSION, Plugin, Reminder,
    RemindersParams, serve,
};
use serde_json::Value;

struct Fake;

fn mode() -> String {
    if let Ok(path) = std::env::var("FAKE_MODE_FILE")
        && let Ok(text) = std::fs::read_to_string(path)
    {
        return text.trim().to_string();
    }
    std::env::var("FAKE_MODE").unwrap_or_else(|_| "ok".into())
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
        match mode().as_str() {
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

    fn action(&mut self, _p: ActionParams) -> Result<Option<Item>, String> {
        Ok(None)
    }

    fn reminders(&mut self, _p: RemindersParams) -> Result<Vec<Reminder>, String> {
        let Ok(path) = std::env::var("FAKE_REMINDERS_FILE") else {
            return Ok(vec![]);
        };
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| e.to_string()),
            Err(_) => Ok(vec![]),
        }
    }

    fn drop_item(&mut self, p: DropParams) -> Result<Item, String> {
        if mode() == "crash" {
            std::process::exit(3);
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
    serve(Fake, io::stdin().lock(), io::stdout())
}

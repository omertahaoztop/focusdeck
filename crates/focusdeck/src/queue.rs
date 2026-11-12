use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Queued {
    pub text: String,
    pub at: DateTime<FixedOffset>,
}

pub struct Queue {
    path: PathBuf,
}

impl Queue {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn push(&self, text: &str, at: DateTime<FixedOffset>) -> io::Result<()> {
        self.append(&[Queued {
            text: text.to_owned(),
            at,
        }])
    }

    pub fn take_all(&self) -> io::Result<Vec<Queued>> {
        // Rename first so a push racing with the read lands in a fresh file.
        let taken = self.path.with_extension("jsonl.taken");
        match fs::rename(&self.path, &taken) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        }
        let raw = fs::read_to_string(&taken)?;
        fs::remove_file(&taken)?;
        // A line cut short by a crash must not block the rest of the queue.
        Ok(raw
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect())
    }

    pub fn put_back(&self, items: &[Queued]) -> io::Result<()> {
        self.append(items)
    }

    fn append(&self, items: &[Queued]) -> io::Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut buf = String::new();
        for item in items {
            buf.push_str(&serde_json::to_string(item).map_err(io::Error::other)?);
            buf.push('\n');
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(buf.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    fn queue() -> (tempfile::TempDir, Queue) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("capture-queue.jsonl");
        (dir, Queue::new(path))
    }

    #[test]
    fn push_twice_then_take_all_in_order_and_file_removed() {
        let (dir, q) = queue();
        q.push("bir", at("2026-11-12T10:00:00+03:00")).unwrap();
        q.push("iki", at("2026-11-12T10:01:00+03:00")).unwrap();
        let got = q.take_all().unwrap();
        assert_eq!(
            got,
            vec![
                Queued {
                    text: "bir".into(),
                    at: at("2026-11-12T10:00:00+03:00")
                },
                Queued {
                    text: "iki".into(),
                    at: at("2026-11-12T10:01:00+03:00")
                },
            ]
        );
        assert!(!dir.path().join("capture-queue.jsonl").exists());
        assert!(q.take_all().unwrap().is_empty());
    }

    #[test]
    fn line_format_is_text_then_at() {
        let (dir, q) = queue();
        q.push("log at", at("2026-11-12T10:00:00+03:00")).unwrap();
        let raw = fs::read_to_string(dir.path().join("capture-queue.jsonl")).unwrap();
        assert_eq!(
            raw,
            "{\"text\":\"log at\",\"at\":\"2026-11-12T10:00:00+03:00\"}\n"
        );
    }

    #[test]
    fn put_back_writes_one_record() {
        let (dir, q) = queue();
        let item = Queued {
            text: "geri".into(),
            at: at("2026-11-12T10:00:00+03:00"),
        };
        q.put_back(std::slice::from_ref(&item)).unwrap();
        let raw = fs::read_to_string(dir.path().join("capture-queue.jsonl")).unwrap();
        assert_eq!(raw.lines().count(), 1);
        assert_eq!(q.take_all().unwrap(), vec![item]);
    }

    #[test]
    fn empty_file_gives_empty_vec() {
        let (dir, q) = queue();
        fs::write(dir.path().join("capture-queue.jsonl"), "").unwrap();
        assert!(q.take_all().unwrap().is_empty());
    }

    #[test]
    fn broken_line_is_skipped() {
        let (dir, q) = queue();
        fs::write(
            dir.path().join("capture-queue.jsonl"),
            "{\"text\":\"ok\",\"at\":\"2026-11-12T10:00:00+03:00\"}\n{\"text\":\"kes\n",
        )
        .unwrap();
        let got = q.take_all().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "ok");
    }

    #[test]
    fn push_creates_missing_state_dir() {
        let dir = tempfile::tempdir().unwrap();
        let q = Queue::new(dir.path().join("state/capture-queue.jsonl"));
        q.push("x", at("2026-11-12T10:00:00+03:00")).unwrap();
        assert_eq!(q.take_all().unwrap().len(), 1);
    }
}

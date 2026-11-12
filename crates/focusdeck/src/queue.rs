use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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
        self.take_all_with(|path| fs::remove_file(path))
    }

    fn take_all_with(&self, remove: impl Fn(&Path) -> io::Result<()>) -> io::Result<Vec<Queued>> {
        let _lock = match self.lock() {
            Ok(lock) => lock,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        // Leftovers of an interrupted drain come first (oldest first), then the live file.
        let mut asides = self.leftovers()?;
        let live = self.aside_name();
        match fs::rename(&self.path, &live) {
            Ok(()) => asides.push(live),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }

        // Files are deleted only after every one was read, so a read error loses nothing.
        let mut read = Vec::new();
        for aside in &asides {
            read.push(decode(&fs::read(aside)?).collect::<Vec<_>>());
        }
        for (done, aside) in asides.iter().enumerate() {
            if let Err(e) = remove(aside) {
                // Entries of already deleted asides lost their file; asides still on disk keep theirs.
                let restored: Vec<Queued> = read.drain(..done).flatten().collect();
                self.append_locked(&restored)?;
                return Err(e);
            }
        }
        Ok(read.into_iter().flatten().collect())
    }

    pub fn put_back(&self, items: &[Queued]) -> io::Result<()> {
        self.append(items)
    }

    fn lock(&self) -> io::Result<File> {
        let mut name = self.path.clone().into_os_string();
        name.push(".lock");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(PathBuf::from(name))?;
        file.lock()?;
        Ok(file)
    }

    fn file_name(&self) -> String {
        self.path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }

    fn aside_name(&self) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(format!(".taken.{}", unique_suffix()));
        PathBuf::from(name)
    }

    fn leftovers(&self) -> io::Result<Vec<PathBuf>> {
        let dir = match self.path.parent() {
            Some(d) if !d.as_os_str().is_empty() => d,
            _ => Path::new("."),
        };
        let prefix = format!("{}.taken.", self.file_name());
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut found = Vec::new();
        for entry in entries {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                found.push(entry.path());
            }
        }
        found.sort();
        Ok(found)
    }

    fn append(&self, items: &[Queued]) -> io::Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let _lock = self.lock()?;
        self.append_locked(items)
    }

    fn append_locked(&self, items: &[Queued]) -> io::Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let mut buf = String::new();
        for item in items {
            buf.push_str(&serde_json::to_string(item).map_err(io::Error::other)?);
            buf.push('\n');
        }
        let mut file = OpenOptions::new()
            .read(true)
            .create(true)
            .append(true)
            .open(&self.path)?;
        // a crash-truncated last line must not swallow the next record
        let len = file.metadata()?.len();
        if len > 0 {
            let mut last = [0u8; 1];
            file.read_exact_at(&mut last, len - 1)?;
            if last[0] != b'\n' {
                buf.insert(0, '\n');
            }
        }
        file.write_all(buf.as_bytes())
    }
}

fn unique_suffix() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:020}-{}-{n:06}", std::process::id())
}

// Per-line decoding: a crash-truncated line, even one cut inside a multi-byte char, is dropped alone.
fn decode(raw: &[u8]) -> impl Iterator<Item = Queued> {
    raw.split(|&b| b == b'\n')
        .filter_map(|line| serde_json::from_slice(line).ok())
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

    #[test]
    fn truncated_multibyte_line_keeps_valid_lines() {
        let (dir, q) = queue();
        let mut raw = b"{\"text\":\"ok\",\"at\":\"2026-11-12T10:00:00+03:00\"}\n".to_vec();
        let broken = "{\"text\":\"koş".as_bytes();
        raw.extend_from_slice(&broken[..broken.len() - 1]);
        assert!(std::str::from_utf8(&raw).is_err());
        fs::write(dir.path().join("capture-queue.jsonl"), raw).unwrap();
        let got = q.take_all().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "ok");
    }

    #[test]
    fn stale_aside_files_are_recovered_oldest_first() {
        let (dir, q) = queue();
        let line = |t: &str| format!("{{\"text\":\"{t}\",\"at\":\"2026-11-12T10:00:00+03:00\"}}\n");
        fs::write(
            dir.path()
                .join("capture-queue.jsonl.taken.00000000000000000002-1-000000"),
            line("yeni-kalan"),
        )
        .unwrap();
        fs::write(
            dir.path()
                .join("capture-queue.jsonl.taken.00000000000000000001-1-000000"),
            line("eski-kalan"),
        )
        .unwrap();
        q.push("canli", at("2026-11-12T10:00:00+03:00")).unwrap();
        let texts: Vec<_> = q.take_all().unwrap().into_iter().map(|i| i.text).collect();
        assert_eq!(texts, ["eski-kalan", "yeni-kalan", "canli"]);
        let rest: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(rest, ["capture-queue.jsonl.lock"]);
    }

    #[test]
    fn stale_aside_without_live_file_is_recovered() {
        let (dir, q) = queue();
        fs::write(
            dir.path()
                .join("capture-queue.jsonl.taken.00000000000000000001-1-000000"),
            "{\"text\":\"kaldi\",\"at\":\"2026-11-12T10:00:00+03:00\"}\n",
        )
        .unwrap();
        let got = q.take_all().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "kaldi");
        assert!(q.take_all().unwrap().is_empty());
    }

    #[test]
    fn push_after_truncated_line_keeps_new_note() {
        let (dir, q) = queue();
        let broken = "{\"text\":\"koş".as_bytes();
        fs::write(
            dir.path().join("capture-queue.jsonl"),
            &broken[..broken.len() - 1],
        )
        .unwrap();
        q.push("yeni", at("2026-11-12T10:00:00+03:00")).unwrap();
        let got = q.take_all().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].text, "yeni");
    }

    #[test]
    fn failed_second_aside_removal_loses_nothing() {
        let (dir, q) = queue();
        let line = |t: &str| format!("{{\"text\":\"{t}\",\"at\":\"2026-11-12T10:00:00+03:00\"}}\n");
        let aside = |n: u8| {
            dir.path()
                .join(format!("capture-queue.jsonl.taken.0000000000000000000{n}-1-000000"))
        };
        fs::write(aside(1), line("a")).unwrap();
        fs::write(aside(2), line("b")).unwrap();
        q.push("canli", at("2026-11-12T10:00:00+03:00")).unwrap();
        let bad = aside(2);
        let err = q
            .take_all_with(|p| {
                if p == bad {
                    Err(io::Error::other("kilitli"))
                } else {
                    fs::remove_file(p)
                }
            })
            .unwrap_err();
        assert_eq!(err.to_string(), "kilitli");
        let mut texts: Vec<_> = q.take_all().unwrap().into_iter().map(|i| i.text).collect();
        texts.sort();
        assert_eq!(texts, ["a", "b", "canli"]);
    }

    #[test]
    fn concurrent_drains_neither_lose_nor_duplicate() {
        let (_dir, q) = queue();
        for i in 0..50 {
            q.push(&format!("n{i}"), at("2026-11-12T10:00:00+03:00"))
                .unwrap();
        }
        let q = std::sync::Arc::new(q);
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let q = q.clone();
                std::thread::spawn(move || q.take_all().unwrap())
            })
            .collect();
        let mut all: Vec<String> = handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .map(|i| i.text)
            .collect();
        all.sort();
        let mut want: Vec<String> = (0..50).map(|i| format!("n{i}")).collect();
        want.sort();
        assert_eq!(all, want);
    }
}

use std::fs::{self, File};
use std::io::{self, Read, Write};
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
}

pub struct Queue {
    path: PathBuf,
}

// The aside files stay locked for as long as this lives, which marks them as in flight.
#[derive(Default)]
pub struct Taken {
    pub items: Vec<Queued>,
    asides: Vec<(PathBuf, File)>,
}

impl Queue {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn push(&self, text: &str, at: DateTime<FixedOffset>) -> io::Result<()> {
        self.push_entry(&Queued {
            text: text.to_owned(),
            at,
            lang: None,
        })
    }

    pub fn push_entry(&self, entry: &Queued) -> io::Result<()> {
        let _lock = self.lock()?;
        self.append_locked(std::slice::from_ref(entry))
    }

    pub fn take(&self) -> io::Result<Taken> {
        let _lock = self.lock()?;
        // Leftovers of dead runs come first (oldest first), then the live file.
        let mut asides = Vec::new();
        for path in self.leftovers()? {
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            match file.try_lock() {
                Ok(()) => asides.push((path, file)),
                Err(fs::TryLockError::WouldBlock) => {}
                Err(fs::TryLockError::Error(e)) => return Err(e),
            }
        }
        let live = self.aside_name();
        match fs::rename(&self.path, &live) {
            Ok(()) => {
                let file = File::open(&live)?;
                file.lock()?;
                asides.push((live, file));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }

        // Nothing is deleted here, so a read error or a crash loses nothing.
        let mut items = Vec::new();
        for (_, file) in &asides {
            let mut raw = Vec::new();
            (&*file).read_to_end(&mut raw)?;
            items.extend(decode(&raw));
        }
        Ok(Taken { items, asides })
    }

    pub fn commit(&self, taken: Taken, unsent: &[Queued]) -> io::Result<()> {
        if unsent.is_empty() && taken.asides.is_empty() {
            return Ok(());
        }
        let _lock = self.lock()?;
        self.append_locked(unsent)?;
        for (path, _file) in &taken.asides {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    fn lock(&self) -> io::Result<File> {
        let mut name = self.path.clone().into_os_string();
        name.push(".lock");
        let file = crate::private::append(Path::new(&name))?;
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

    fn append_locked(&self, items: &[Queued]) -> io::Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let mut buf = String::new();
        for item in items {
            buf.push_str(&serde_json::to_string(item).map_err(io::Error::other)?);
            buf.push('\n');
        }
        let mut file = crate::private::append(&self.path)?;
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
    use std::os::unix::fs::PermissionsExt;

    const FILE: &str = "capture-queue.jsonl";

    fn at(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    fn queue() -> (tempfile::TempDir, Queue) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE);
        (dir, Queue::new(path))
    }

    fn entry(text: &str) -> Queued {
        Queued {
            text: text.into(),
            at: at("2026-11-12T10:00:00+03:00"),
            lang: None,
        }
    }

    fn line(text: &str) -> String {
        format!("{{\"text\":\"{text}\",\"at\":\"2026-11-12T10:00:00+03:00\"}}\n")
    }

    fn aside(dir: &tempfile::TempDir, n: u8) -> PathBuf {
        dir.path()
            .join(format!("{FILE}.taken.0000000000000000000{n}-1-000000"))
    }

    fn texts(items: &[Queued]) -> Vec<&str> {
        items.iter().map(|i| i.text.as_str()).collect()
    }

    fn drain(q: &Queue) -> Vec<Queued> {
        let taken = q.take().unwrap();
        let items = taken.items.clone();
        q.commit(taken, &[]).unwrap();
        items
    }

    fn files(dir: &tempfile::TempDir) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn push_twice_then_take_in_order_and_files_removed() {
        let (dir, q) = queue();
        q.push("bir", at("2026-11-12T10:00:00+03:00")).unwrap();
        q.push("iki", at("2026-11-12T10:01:00+03:00")).unwrap();
        let taken = q.take().unwrap();
        assert_eq!(
            taken.items,
            vec![
                entry("bir"),
                Queued {
                    text: "iki".into(),
                    at: at("2026-11-12T10:01:00+03:00"),
                    lang: None,
                },
            ]
        );
        assert!(!dir.path().join(FILE).exists());
        q.commit(taken, &[]).unwrap();
        assert_eq!(files(&dir), [format!("{FILE}.lock")]);
        assert!(q.take().unwrap().items.is_empty());
    }

    #[test]
    fn line_format_is_text_then_at() {
        let (dir, q) = queue();
        q.push("log at", at("2026-11-12T10:00:00+03:00")).unwrap();
        let raw = fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert_eq!(
            raw,
            "{\"text\":\"log at\",\"at\":\"2026-11-12T10:00:00+03:00\"}\n"
        );
    }

    #[test]
    fn queue_files_are_owner_only() {
        let (dir, q) = queue();
        q.push("gizli", at("2026-11-12T10:00:00+03:00")).unwrap();
        let mode = |p: PathBuf| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(dir.path().join(FILE)), 0o600);
        assert_eq!(mode(dir.path().join(format!("{FILE}.lock"))), 0o600);
        let taken = q.take().unwrap();
        assert_eq!(mode(taken.asides[0].0.clone()), 0o600);
    }

    #[test]
    fn old_entries_without_lang_still_load() {
        let (dir, q) = queue();
        fs::write(dir.path().join(FILE), line("a")).unwrap();
        let got = drain(&q);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].lang, None);

        let tr = Queued {
            text: "b".into(),
            at: at("2026-11-12T10:00:00+03:00"),
            lang: Some("tr".into()),
        };
        q.push_entry(&tr).unwrap();
        let raw = fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert!(raw.contains("\"lang\":\"tr\""), "{raw}");
        assert_eq!(drain(&q), vec![tr]);

        q.push("c", at("2026-11-12T10:00:00+03:00")).unwrap();
        let raw = fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert!(!raw.contains("lang"), "{raw}");
    }

    #[test]
    fn push_entry_writes_one_record() {
        let (dir, q) = queue();
        q.push_entry(&entry("geri")).unwrap();
        let raw = fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert_eq!(raw.lines().count(), 1);
        assert_eq!(drain(&q), vec![entry("geri")]);
    }

    #[test]
    fn empty_file_gives_empty_vec() {
        let (dir, q) = queue();
        fs::write(dir.path().join(FILE), "").unwrap();
        assert!(drain(&q).is_empty());
    }

    #[test]
    fn broken_line_is_skipped() {
        let (dir, q) = queue();
        fs::write(
            dir.path().join(FILE),
            format!("{}{{\"text\":\"kes\n", line("ok")),
        )
        .unwrap();
        assert_eq!(texts(&drain(&q)), ["ok"]);
    }

    #[test]
    fn push_creates_missing_state_dir() {
        let dir = tempfile::tempdir().unwrap();
        let q = Queue::new(dir.path().join("state").join(FILE));
        q.push("x", at("2026-11-12T10:00:00+03:00")).unwrap();
        assert_eq!(drain(&q).len(), 1);
    }

    #[test]
    fn truncated_multibyte_line_keeps_valid_lines() {
        let (dir, q) = queue();
        let mut raw = line("ok").into_bytes();
        let broken = "{\"text\":\"koş".as_bytes();
        raw.extend_from_slice(&broken[..broken.len() - 1]);
        assert!(std::str::from_utf8(&raw).is_err());
        fs::write(dir.path().join(FILE), raw).unwrap();
        assert_eq!(texts(&drain(&q)), ["ok"]);
    }

    #[test]
    fn stale_aside_files_are_recovered_oldest_first() {
        let (dir, q) = queue();
        fs::write(aside(&dir, 2), line("yeni-kalan")).unwrap();
        fs::write(aside(&dir, 1), line("eski-kalan")).unwrap();
        q.push("canlı", at("2026-11-12T10:00:00+03:00")).unwrap();
        assert_eq!(texts(&drain(&q)), ["eski-kalan", "yeni-kalan", "canlı"]);
        assert_eq!(files(&dir), [format!("{FILE}.lock")]);
    }

    #[test]
    fn stale_aside_without_live_file_is_recovered() {
        let (dir, q) = queue();
        fs::write(aside(&dir, 1), line("kaldı")).unwrap();
        assert_eq!(texts(&drain(&q)), ["kaldı"]);
        assert!(drain(&q).is_empty());
    }

    #[test]
    fn push_after_truncated_line_keeps_new_note() {
        let (dir, q) = queue();
        let broken = "{\"text\":\"koş".as_bytes();
        fs::write(dir.path().join(FILE), &broken[..broken.len() - 1]).unwrap();
        q.push("yeni", at("2026-11-12T10:00:00+03:00")).unwrap();
        assert_eq!(texts(&drain(&q)), ["yeni"]);
    }

    #[test]
    fn read_error_loses_nothing() {
        let (dir, q) = queue();
        // reading a directory fails after it was opened and locked like a file
        fs::create_dir(aside(&dir, 1)).unwrap();
        q.push("canlı", at("2026-11-12T10:00:00+03:00")).unwrap();
        q.take().err().expect("reading a directory fails");
        fs::remove_dir(aside(&dir, 1)).unwrap();
        assert_eq!(texts(&drain(&q)), ["canlı"]);
    }

    #[test]
    fn in_flight_aside_is_not_taken_twice() {
        let (_dir, first) = queue();
        first.push("a", at("2026-11-12T10:00:00+03:00")).unwrap();
        let held = first.take().unwrap();
        assert_eq!(texts(&held.items), ["a"]);

        let second = Queue::new(first.path.clone());
        second.push("b", at("2026-11-12T10:00:00+03:00")).unwrap();
        let other = second.take().unwrap();
        assert_eq!(texts(&other.items), ["b"]);
        second.commit(other, &[]).unwrap();
        first.commit(held, &[]).unwrap();
        assert!(drain(&first).is_empty());
    }

    #[test]
    fn aside_of_a_dead_run_is_taken_again() {
        let (_dir, q) = queue();
        q.push("a", at("2026-11-12T10:00:00+03:00")).unwrap();
        let dead = q.take().unwrap();
        // a killed process releases its locks the same way
        drop(dead);
        q.push("b", at("2026-11-12T10:00:00+03:00")).unwrap();
        assert_eq!(texts(&drain(&q)), ["a", "b"]);
    }

    #[test]
    fn commit_keeps_unsent_and_removes_asides() {
        let (dir, q) = queue();
        for t in ["a", "b", "c"] {
            q.push(t, at("2026-11-12T10:00:00+03:00")).unwrap();
        }
        let taken = q.take().unwrap();
        let unsent: Vec<Queued> = taken.items[1..].to_vec();
        q.commit(taken, &unsent).unwrap();
        assert_eq!(files(&dir), [FILE.to_owned(), format!("{FILE}.lock")]);
        assert_eq!(texts(&drain(&q)), ["b", "c"]);
    }

    #[test]
    fn dropping_taken_loses_nothing() {
        let (dir, q) = queue();
        q.push("a", at("2026-11-12T10:00:00+03:00")).unwrap();
        q.push("b", at("2026-11-12T10:00:00+03:00")).unwrap();
        drop(q.take().unwrap());
        assert_eq!(files(&dir).len(), 2, "{:?}", files(&dir));
        assert_eq!(texts(&drain(&q)), ["a", "b"]);
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
                std::thread::spawn(move || drain(&q))
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

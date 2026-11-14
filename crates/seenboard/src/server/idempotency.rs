//! Persistent `Idempotency-Key` claims so a retried request never creates a second note.

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::{DateTime, Duration, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::fsio::write_atomic;
use crate::i18n::{msg, t, t_args};

const RETENTION_DAYS: i64 = 90;

#[derive(Debug, Clone, PartialEq)]
pub enum Claim {
    New,
    Replay(Value),
    InProgress,
    Reused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum State {
    InProgress,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Entry {
    body: String,
    state: State,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    response: Option<Value>,
    at: String,
}

#[derive(Deserialize)]
struct FileFormat {
    version: u32,
    keys: BTreeMap<String, Entry>,
}

#[derive(Serialize)]
struct FileRef<'a> {
    version: u32,
    keys: &'a BTreeMap<String, Entry>,
}

pub struct IdempotencyStore {
    path: PathBuf,
    keys: Mutex<BTreeMap<String, Entry>>,
}

fn hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(64), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn persist(path: &std::path::Path, keys: &BTreeMap<String, Entry>) -> io::Result<()> {
    let bytes =
        serde_json::to_vec_pretty(&FileRef { version: 1, keys }).map_err(io::Error::other)?;
    write_atomic(path, &bytes)
}

impl IdempotencyStore {
    pub fn open(path: PathBuf, now: DateTime<FixedOffset>) -> io::Result<(Self, usize)> {
        let mut keys = match std::fs::read(&path) {
            Ok(raw) => {
                let file: FileFormat = serde_json::from_slice(&raw)
                    .map_err(|e| invalid(format!("{}: {e}", path.display())))?;
                if file.version != 1 {
                    return Err(invalid(t_args(
                        msg::SERVER_STORE_VERSION,
                        &[
                            ("path", path.display().to_string().as_str().into()),
                            ("version", file.version.to_string().as_str().into()),
                        ],
                    )));
                }
                file.keys
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(e),
        };
        let cutoff = now - Duration::days(RETENTION_DAYS);
        let before = keys.len();
        let mut dropped_in_progress = 0;
        let mut expired = Vec::new();
        for (key, entry) in &keys {
            let at = DateTime::parse_from_rfc3339(&entry.at).map_err(|e| {
                invalid(t_args(
                    msg::SERVER_STORE_ENTRY_TIME,
                    &[
                        ("path", path.display().to_string().as_str().into()),
                        ("error", e.to_string().as_str().into()),
                    ],
                ))
            })?;
            if entry.state == State::InProgress {
                dropped_in_progress += 1;
                expired.push(key.clone());
            } else if at < cutoff {
                expired.push(key.clone());
            }
        }
        for key in expired {
            keys.remove(&key);
        }
        if keys.len() != before {
            persist(&path, &keys)?;
        }
        Ok((
            Self {
                path,
                keys: Mutex::new(keys),
            },
            dropped_in_progress,
        ))
    }

    pub fn claim(
        &self,
        key: &str,
        body_sha256: [u8; 32],
        now: DateTime<FixedOffset>,
    ) -> io::Result<Claim> {
        let body = hex(&body_sha256);
        let mut keys = self.keys.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = keys.get(key) {
            return Ok(if entry.body != body {
                Claim::Reused
            } else if entry.state == State::InProgress {
                Claim::InProgress
            } else {
                Claim::Replay(entry.response.clone().unwrap_or(Value::Null))
            });
        }
        let cutoff = now - Duration::days(RETENTION_DAYS);
        keys.retain(|_, entry| {
            entry.state == State::InProgress
                || DateTime::parse_from_rfc3339(&entry.at).is_ok_and(|at| at >= cutoff)
        });
        keys.insert(
            key.to_owned(),
            Entry {
                body,
                state: State::InProgress,
                response: None,
                at: now.to_rfc3339(),
            },
        );
        if let Err(e) = persist(&self.path, &keys) {
            keys.remove(key);
            return Err(e);
        }
        Ok(Claim::New)
    }

    /// Memory keeps the answer even when the write fails, so a retry replays it instead of waiting.
    pub fn complete(&self, key: &str, response: &Value) -> io::Result<()> {
        let mut keys = self.keys.lock().unwrap_or_else(|e| e.into_inner());
        let entry = keys.get_mut(key).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, t(msg::SERVER_KEY_NOT_CLAIMED))
        })?;
        entry.state = State::Done;
        entry.response = Some(response.clone());
        persist(&self.path, &keys)
    }

    pub fn release(&self, key: &str) -> io::Result<()> {
        let mut keys = self.keys.lock().unwrap_or_else(|e| e.into_inner());
        if keys.remove(key).is_none() {
            return Ok(());
        }
        persist(&self.path, &keys)
    }
}

pub fn body_hash(body: &[u8]) -> [u8; 32] {
    Sha256::digest(body).into()
}

/// Binds a key to where it was sent, so one key and body on another box or profile is `Reused`.
pub fn request_hash(target: &[&str], body: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in target {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    hasher.update(body);
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-13T10:00:00+03:00").unwrap()
    }

    fn open(dir: &tempfile::TempDir) -> (IdempotencyStore, usize) {
        IdempotencyStore::open(dir.path().join("idempotency.json"), now()).unwrap()
    }

    #[test]
    fn same_key_same_body_replays() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(&dir);
        let body = body_hash(b"a");
        assert_eq!(store.claim("k", body, now()).unwrap(), Claim::New);
        store.complete("k", &json!({"ok": 1})).unwrap();
        assert_eq!(
            store.claim("k", body, now()).unwrap(),
            Claim::Replay(json!({"ok": 1}))
        );
        let (reopened, dropped) = open(&dir);
        assert_eq!(dropped, 0);
        assert_eq!(
            reopened.claim("k", body, now()).unwrap(),
            Claim::Replay(json!({"ok": 1}))
        );
    }

    #[test]
    fn same_key_other_body_is_reused() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(&dir);
        assert_eq!(
            store.claim("k", body_hash(b"a"), now()).unwrap(),
            Claim::New
        );
        assert_eq!(
            store.claim("k", body_hash(b"b"), now()).unwrap(),
            Claim::Reused
        );
        store.complete("k", &json!({})).unwrap();
        assert_eq!(
            store.claim("k", body_hash(b"b"), now()).unwrap(),
            Claim::Reused
        );
    }

    #[test]
    fn claim_while_in_progress() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(&dir);
        let body = body_hash(b"a");
        assert_eq!(store.claim("k", body, now()).unwrap(), Claim::New);
        assert_eq!(store.claim("k", body, now()).unwrap(), Claim::InProgress);
    }

    #[test]
    fn release_allows_retry() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(&dir);
        let body = body_hash(b"a");
        assert_eq!(store.claim("k", body, now()).unwrap(), Claim::New);
        store.release("k").unwrap();
        assert_eq!(store.claim("k", body, now()).unwrap(), Claim::New);
        let (reopened, dropped) = open(&dir);
        assert_eq!(dropped, 1);
        assert_eq!(reopened.claim("k", body, now()).unwrap(), Claim::New);
    }

    #[test]
    fn reopen_drops_in_progress_and_counts_them() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(&dir);
        let body = body_hash(b"a");
        store.claim("pending", body, now()).unwrap();
        store.claim("finished", body, now()).unwrap();
        store.complete("finished", &json!({"n": 2})).unwrap();
        drop(store);
        let (reopened, dropped) = open(&dir);
        assert_eq!(dropped, 1);
        assert_eq!(reopened.claim("pending", body, now()).unwrap(), Claim::New);
        assert_eq!(
            reopened.claim("finished", body, now()).unwrap(),
            Claim::Replay(json!({"n": 2}))
        );
        let (_, again) = open(&dir);
        assert_eq!(again, 1, "the re-claimed key is in progress again");
    }

    #[test]
    fn entries_older_than_90_days_are_pruned_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(&dir);
        let body = body_hash(b"a");
        let old = now() - Duration::days(91);
        let fresh = now() - Duration::days(89);
        store.claim("old", body, old).unwrap();
        store.complete("old", &json!(1)).unwrap();
        store.claim("fresh", body, fresh).unwrap();
        store.complete("fresh", &json!(2)).unwrap();
        drop(store);
        let (reopened, dropped) = open(&dir);
        assert_eq!(dropped, 0);
        assert_eq!(reopened.claim("old", body, now()).unwrap(), Claim::New);
        assert_eq!(
            reopened.claim("fresh", body, now()).unwrap(),
            Claim::Replay(json!(2))
        );
    }

    #[test]
    fn a_new_claim_prunes_finished_entries_older_than_90_days() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(&dir);
        let body = body_hash(b"a");
        store
            .claim("old", body, now() - Duration::days(91))
            .unwrap();
        store.complete("old", &json!(1)).unwrap();
        store
            .claim("stuck", body, now() - Duration::days(91))
            .unwrap();
        store
            .claim("fresh", body, now() - Duration::days(89))
            .unwrap();
        store.complete("fresh", &json!(2)).unwrap();
        store.claim("new", body, now()).unwrap();
        let file: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("idempotency.json")).unwrap())
                .unwrap();
        let mut kept: Vec<&String> = file["keys"].as_object().unwrap().keys().collect();
        kept.sort();
        assert_eq!(kept, ["fresh", "new", "stuck"]);
    }

    #[test]
    fn eight_threads_claiming_one_key_get_one_new() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(&dir);
        let store = Arc::new(store);
        let body = body_hash(b"a");
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let store = Arc::clone(&store);
                std::thread::spawn(move || store.claim("k", body, now()).unwrap())
            })
            .collect();
        let results: Vec<Claim> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.iter().filter(|c| **c == Claim::New).count(), 1);
        assert_eq!(
            results.iter().filter(|c| **c == Claim::InProgress).count(),
            7
        );
    }

    const A_HEX: &str = "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb";

    #[test]
    fn file_format_is_pinned() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(&dir);
        let path = dir.path().join("idempotency.json");
        let read = || -> Value { serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap() };
        store.claim("k", body_hash(b"a"), now()).unwrap();
        assert_eq!(
            read(),
            json!({"version": 1, "keys": {"k": {
                "body": A_HEX, "state": "in_progress", "at": "2026-11-13T10:00:00+03:00"
            }}})
        );
        store.complete("k", &json!({"ok": 1})).unwrap();
        assert_eq!(
            read(),
            json!({"version": 1, "keys": {"k": {
                "body": A_HEX, "state": "done", "response": {"ok": 1},
                "at": "2026-11-13T10:00:00+03:00"
            }}})
        );
    }

    #[test]
    fn opens_a_hand_written_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("idempotency.json");
        let raw = format!(
            r#"{{"version":1,"keys":{{"k":{{"body":"{A_HEX}","state":"done","response":{{"ok":1}},"at":"2026-11-13T09:00:00+03:00"}}}}}}"#
        );
        std::fs::write(&path, raw).unwrap();
        let (store, dropped) = IdempotencyStore::open(path, now()).unwrap();
        assert_eq!(dropped, 0);
        assert_eq!(
            store.claim("k", body_hash(b"a"), now()).unwrap(),
            Claim::Replay(json!({"ok": 1}))
        );
    }

    #[test]
    fn hash_bytes_are_zero_padded_lowercase_hex() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = open(&dir);
        let mut hash = [1u8; 32];
        hash[31] = 0xab;
        store.claim("k", hash, now()).unwrap();
        let file: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("idempotency.json")).unwrap())
                .unwrap();
        assert_eq!(
            file["keys"]["k"]["body"],
            json!(format!("{}ab", "01".repeat(31)))
        );
    }

    #[test]
    fn corrupt_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("idempotency.json");
        std::fs::write(&path, b"{not json").unwrap();
        assert!(IdempotencyStore::open(path.clone(), now()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{not json");
    }

    #[test]
    fn failed_disk_write_rolls_back_a_claim_but_keeps_an_answer() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("state");
        std::fs::create_dir(&sub).unwrap();
        let (store, _) = IdempotencyStore::open(sub.join("idempotency.json"), now()).unwrap();
        let body = body_hash(b"a");
        std::fs::remove_dir_all(&sub).unwrap();
        assert!(store.claim("k", body, now()).is_err());
        std::fs::create_dir(&sub).unwrap();
        assert_eq!(store.claim("k", body, now()).unwrap(), Claim::New);
        assert_eq!(store.claim("r", body, now()).unwrap(), Claim::New);
        std::fs::remove_dir_all(&sub).unwrap();
        assert!(store.complete("k", &json!(1)).is_err());
        assert!(store.release("r").is_err());
        std::fs::create_dir(&sub).unwrap();
        assert_eq!(
            store.claim("k", body, now()).unwrap(),
            Claim::Replay(json!(1))
        );
        assert_eq!(store.claim("r", body, now()).unwrap(), Claim::New);
        let (reopened, _) = IdempotencyStore::open(sub.join("idempotency.json"), now()).unwrap();
        assert_eq!(
            reopened.claim("k", body, now()).unwrap(),
            Claim::Replay(json!(1)),
            "the next successful write carries the answer to disk"
        );
    }
}

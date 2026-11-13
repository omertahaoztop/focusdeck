//! The `local` box's note file: one JSON document guarded by a lock file.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, FixedOffset};
use focusdeck_i18n::Localizer;
use focusdeck_protocol::{Link, Source};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::fsio;
use crate::i18n::{self, msg};
use crate::paths::Paths;

const FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub list: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<DateTime<FixedOffset>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    pub position: f64,
    pub created: DateTime<FixedOffset>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_at: Option<DateTime<FixedOffset>>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct FileData {
    version: u32,
    items: Vec<Record>,
    #[serde(flatten)]
    extra: serde_json::Map<String, Value>,
}

#[derive(Debug)]
pub enum StoreError {
    Corrupt { path: PathBuf, error: String },
    Newer { path: PathBuf, version: u32 },
    Io { path: PathBuf, error: String },
}

impl StoreError {
    pub fn localized(&self, l: &Localizer) -> String {
        match self {
            StoreError::Corrupt { path, error } => l.t_args(
                msg::LOCAL_FILE_CORRUPT,
                &[
                    ("path", path.display().to_string().as_str().into()),
                    ("error", error.as_str().into()),
                ],
            ),
            StoreError::Newer { path, version } => l.t_args(
                msg::LOCAL_FILE_NEWER,
                &[
                    ("path", path.display().to_string().as_str().into()),
                    ("version", version.to_string().as_str().into()),
                ],
            ),
            StoreError::Io { path, error } => l.t_args(
                msg::LOCAL_FILE_IO,
                &[
                    ("path", path.display().to_string().as_str().into()),
                    ("error", error.as_str().into()),
                ],
            ),
        }
    }
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.localized(i18n::current()))
    }
}

impl std::error::Error for StoreError {}

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(path: PathBuf) -> Store {
        Store { path }
    }

    pub fn from_paths(paths: &Paths) -> Store {
        Store::new(paths.data_dir.join("local.json"))
    }

    pub fn read(&self) -> Result<Vec<Record>, StoreError> {
        if !self.path.parent().is_some_and(Path::exists) {
            return Ok(Vec::new());
        }
        let _lock = self.lock_file(true)?;
        Ok(self.load()?.items)
    }

    pub fn update<T>(
        &self,
        f: impl FnOnce(&mut Vec<Record>) -> Result<T, String>,
    ) -> Result<Result<T, String>, StoreError> {
        let _lock = self.lock_file(false)?;
        let mut data = self.load()?;
        match f(&mut data.items) {
            Ok(value) => {
                self.write(&data)?;
                Ok(Ok(value))
            }
            Err(message) => Ok(Err(message)),
        }
    }

    fn sibling(&self, suffix: &str) -> PathBuf {
        let mut name = self.path.as_os_str().to_owned();
        name.push(suffix);
        PathBuf::from(name)
    }

    fn io_error(path: &Path, error: io::Error) -> StoreError {
        StoreError::Io {
            path: path.to_path_buf(),
            error: error.to_string(),
        }
    }

    fn lock_file(&self, shared: bool) -> Result<File, StoreError> {
        let lock_path = self.sibling(".lock");
        let open = || -> io::Result<File> {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent)?;
            }
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .read(true)
                .open(&lock_path)?;
            if shared {
                file.lock_shared()?;
            } else {
                file.lock()?;
            }
            Ok(file)
        };
        open().map_err(|error| Self::io_error(&lock_path, error))
    }

    fn load(&self) -> Result<FileData, StoreError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(FileData {
                    version: FORMAT_VERSION,
                    items: Vec::new(),
                    extra: serde_json::Map::new(),
                });
            }
            Err(error) => return Err(Self::io_error(&self.path, error)),
        };
        let data: FileData =
            serde_json::from_slice(&bytes).map_err(|error| StoreError::Corrupt {
                path: self.path.clone(),
                error: error.to_string(),
            })?;
        if data.version > FORMAT_VERSION {
            return Err(StoreError::Newer {
                path: self.path.clone(),
                version: data.version,
            });
        }
        Ok(data)
    }

    fn write(&self, data: &FileData) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec_pretty(data)
            .map_err(|error| Self::io_error(&self.path, error.into()))?;
        fsio::write_atomic(&self.path, &bytes).map_err(|error| Self::io_error(&self.path, error))
    }
}

pub fn new_id(existing: &[Record]) -> String {
    new_id_with(existing, fsio::unique_id)
}

fn new_id_with(existing: &[Record], mut candidate: impl FnMut() -> String) -> String {
    loop {
        let id = candidate();
        if existing.iter().all(|r| r.id != id) {
            return id;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::Arc;

    use chrono::DateTime;
    use serde_json::json;

    fn record(id: &str, title: &str) -> Record {
        Record {
            id: id.into(),
            list: "inbox".into(),
            title: title.into(),
            description: Some("details".into()),
            due: None,
            labels: vec!["home".into()],
            from: None,
            links: vec![],
            source: None,
            position: 1.0,
            created: DateTime::parse_from_rfc3339("2026-01-02T03:04:05+03:00").unwrap(),
            done_at: None,
            extra: serde_json::Map::new(),
        }
    }

    fn store_in(dir: &tempfile::TempDir) -> Store {
        Store::new(dir.path().join("local.json"))
    }

    #[test]
    fn missing_file_reads_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        assert_eq!(store.read().unwrap(), vec![]);
        assert!(!dir.path().join("local.json").exists());
    }

    #[test]
    fn missing_dir_reads_empty_and_stays_missing() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        let store = Store::new(data.join("local.json"));
        assert_eq!(store.read().unwrap(), vec![]);
        assert!(!data.exists());
    }

    #[test]
    fn update_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        let first = record("a", "first");
        let mut second = record("b", "second");
        second.description = None;
        second.labels.clear();
        let (a, b) = (first.clone(), second.clone());
        store
            .update(|items| {
                items.push(a);
                items.push(b);
                Ok(())
            })
            .unwrap()
            .unwrap();
        assert_eq!(store.read().unwrap(), vec![first, second]);
        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("local.json")).unwrap()).unwrap();
        assert_eq!(raw["version"], 1);
        assert_eq!(raw["items"].as_array().unwrap().len(), 2);
        assert!(raw["items"][1].get("description").is_none());
    }

    #[test]
    fn unknown_fields_survive_a_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local.json");
        let item = |id: &str, extra: bool| {
            let mut value = json!({
                "id": id, "list": "inbox", "title": id,
                "position": 1.0, "created": "2026-01-02T03:04:05+03:00",
            });
            if extra {
                value["pin"] = json!(true);
            }
            value
        };
        let doc =
            json!({"version": 1, "color": "red", "items": [item("a", true), item("b", false)]});
        std::fs::write(&path, doc.to_string()).unwrap();
        Store::new(path.clone())
            .update(|items| {
                items[1].title = "changed".into();
                Ok(())
            })
            .unwrap()
            .unwrap();
        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(raw["color"], "red");
        assert_eq!(raw["items"][0]["pin"], true);
        assert_eq!(raw["items"][1]["title"], "changed");
    }

    #[test]
    fn corrupt_file_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local.json");
        let bytes = br#"{"version":1,"items":["#;
        std::fs::write(&path, bytes).unwrap();
        let store = Store::new(path.clone());
        assert!(matches!(store.read(), Err(StoreError::Corrupt { .. })));
        assert!(matches!(
            store.update(|_| Ok(())),
            Err(StoreError::Corrupt { .. })
        ));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(!dir.path().join("local.json.tmp").exists());
    }

    #[test]
    fn newer_version_is_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local.json");
        let bytes = br#"{"version":2,"items":[]}"#;
        std::fs::write(&path, bytes).unwrap();
        let store = Store::new(path.clone());
        assert!(matches!(
            store.read(),
            Err(StoreError::Newer { version: 2, .. })
        ));
        assert!(matches!(
            store.update(|_| Ok(())),
            Err(StoreError::Newer { version: 2, .. })
        ));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert!(!dir.path().join("local.json.tmp").exists());
    }

    #[test]
    fn closure_error_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local.json");
        let store = Store::new(path.clone());
        store
            .update(|items| {
                items.push(record("a", "first"));
                Ok(())
            })
            .unwrap()
            .unwrap();
        let before = std::fs::read(&path).unwrap();
        let outcome = store
            .update(|items| {
                items.clear();
                Err::<(), String>("no".into())
            })
            .unwrap();
        assert_eq!(outcome, Err("no".to_string()));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(store.read().unwrap().len(), 1);
    }

    #[test]
    fn concurrent_updates_keep_every_record() {
        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join("local.json"));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let path = Arc::clone(&path);
                std::thread::spawn(move || {
                    let store = Store::new((*path).clone());
                    for _ in 0..25 {
                        store
                            .update(|items| {
                                let id = new_id(items);
                                items.push(record(&id, "n"));
                                Ok(())
                            })
                            .unwrap()
                            .unwrap();
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
        let items = Store::new((*path).clone()).read().unwrap();
        assert_eq!(items.len(), 200);
        let ids: HashSet<_> = items.iter().map(|r| r.id.clone()).collect();
        assert_eq!(ids.len(), 200);
    }

    #[test]
    fn new_id_is_unique_against_existing() {
        let mut items: Vec<Record> = Vec::new();
        for _ in 0..1000 {
            let id = new_id(&items);
            assert!(items.iter().all(|r| r.id != id));
            items.push(record(&id, "n"));
        }
    }

    #[test]
    fn new_id_skips_a_colliding_candidate() {
        let taken = [record("dup", "taken")];
        let mut candidates = ["dup", "fresh"].into_iter();
        let id = new_id_with(&taken, || candidates.next().unwrap().to_owned());
        assert_eq!(id, "fresh");
        assert_eq!(candidates.next(), None);
    }

    #[cfg(unix)]
    #[test]
    fn file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(&dir);
        store
            .update(|items| {
                items.push(record("a", "first"));
                Ok(())
            })
            .unwrap()
            .unwrap();
        let mode = std::fs::metadata(dir.path().join("local.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn errors_are_localized() {
        let tr = crate::i18n::localizer(Some("tr"));
        let error = StoreError::Newer {
            path: "x/local.json".into(),
            version: 2,
        };
        assert_eq!(
            error.localized(&tr),
            "x/local.json daha yeni bir focusdeck ile yazılmış (biçim 2); focusdeck'i güncelle"
        );
        assert!(error.to_string().contains("newer focusdeck"));
    }
}

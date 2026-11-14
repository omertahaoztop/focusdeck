use crate::config::Config;
use crate::duration;
use crate::i18n::{self, msg};
use crate::paths::Paths;
use seenboard_i18n::Localizer;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_REFRESH: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Profile {
    pub name: String,
    pub capture_into: Option<String>,
    #[serde(default)]
    pub capture_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub capture_label_aliases: BTreeMap<String, String>,
    #[serde(default, rename = "box")]
    pub boxes: Vec<BoxSpec>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BoxSpec {
    pub id: String,
    pub title: String,
    pub plugin: String,
    pub row: u16,
    pub col: u16,
    #[serde(default = "one")]
    pub width: u16,
    #[serde(default = "one")]
    pub height: u16,
    #[serde(default = "default_refresh", deserialize_with = "refresh_from_str")]
    pub refresh: Duration,
    pub plan_into: Option<String>,
    #[serde(default)]
    pub config: toml::Table,
}

fn one() -> u16 {
    1
}

fn default_refresh() -> Duration {
    DEFAULT_REFRESH
}

fn refresh_from_str<'de, D: Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    let s = String::deserialize(d)?;
    duration::parse(&s).map_err(serde::de::Error::custom)
}

#[derive(Debug)]
pub enum ProfileError {
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    DuplicateBoxId(String),
    UnknownCaptureInto(String),
    PlanIntoSelf(String),
    UnknownPlanInto(String),
    MixedServers {
        plugin: String,
        first: String,
        second: String,
    },
    EmptyBox(String),
    OverlappingCells {
        first: String,
        second: String,
        row: u32,
        col: u32,
    },
    NoProfileSelected,
    InvalidName(String),
    Create {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl ProfileError {
    pub fn localized(&self, l: &Localizer) -> String {
        match self {
            ProfileError::Read { path, source } => l.t_args(
                msg::PROFILE_READ_FAILED,
                &[
                    ("path", path.display().to_string().as_str().into()),
                    ("error", source.to_string().as_str().into()),
                ],
            ),
            ProfileError::Parse { path, source } => l.t_args(
                msg::PROFILE_PARSE_FAILED,
                &[
                    ("path", path.display().to_string().as_str().into()),
                    ("error", source.to_string().as_str().into()),
                ],
            ),
            ProfileError::DuplicateBoxId(id) => {
                l.t_args(msg::PROFILE_DUPLICATE_BOX_ID, &[("id", id.as_str().into())])
            }
            ProfileError::UnknownCaptureInto(id) => l.t_args(
                msg::PROFILE_UNKNOWN_CAPTURE_INTO,
                &[("id", id.as_str().into())],
            ),
            ProfileError::PlanIntoSelf(id) => {
                l.t_args(msg::PROFILE_PLAN_INTO_SELF, &[("id", id.as_str().into())])
            }
            ProfileError::UnknownPlanInto(id) => l.t_args(
                msg::PROFILE_UNKNOWN_PLAN_INTO,
                &[("id", id.as_str().into())],
            ),
            ProfileError::MixedServers {
                plugin,
                first,
                second,
            } => l.t_args(
                msg::PROFILE_MIXED_SERVERS,
                &[
                    ("plugin", plugin.as_str().into()),
                    ("first", first.as_str().into()),
                    ("second", second.as_str().into()),
                ],
            ),
            ProfileError::EmptyBox(id) => {
                l.t_args(msg::PROFILE_EMPTY_BOX, &[("id", id.as_str().into())])
            }
            ProfileError::OverlappingCells {
                first,
                second,
                row,
                col,
            } => l.t_args(
                msg::PROFILE_OVERLAPPING_CELLS,
                &[
                    ("first", first.as_str().into()),
                    ("second", second.as_str().into()),
                    ("row", row.to_string().as_str().into()),
                    ("col", col.to_string().as_str().into()),
                ],
            ),
            ProfileError::NoProfileSelected => l.t(msg::PROFILE_NO_PROFILE),
            ProfileError::InvalidName(name) => l.t_args(
                msg::PROFILE_INVALID_NAME,
                &[("name", format!("{name:?}").as_str().into())],
            ),
            ProfileError::Create { path, source } => l.t_args(
                msg::PROFILE_CREATE_FAILED,
                &[
                    ("path", path.display().to_string().as_str().into()),
                    ("error", source.to_string().as_str().into()),
                ],
            ),
        }
    }
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.localized(i18n::current()))
    }
}

impl std::error::Error for ProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ProfileError::Read { source, .. } => Some(source),
            ProfileError::Parse { source, .. } => Some(source),
            ProfileError::Create { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl Profile {
    pub fn box_index(&self, id: &str) -> Option<usize> {
        self.boxes.iter().position(|b| b.id == id)
    }

    fn validate(&self) -> Result<(), ProfileError> {
        for (i, b) in self.boxes.iter().enumerate() {
            if self.boxes[..i].iter().any(|prev| prev.id == b.id) {
                return Err(ProfileError::DuplicateBoxId(b.id.clone()));
            }
            if b.width == 0 || b.height == 0 {
                return Err(ProfileError::EmptyBox(b.id.clone()));
            }
            if b.plan_into.as_deref() == Some(b.id.as_str()) {
                return Err(ProfileError::PlanIntoSelf(b.id.clone()));
            }
            if let Some(target) = &b.plan_into
                && self.box_index(target).is_none()
            {
                return Err(ProfileError::UnknownPlanInto(target.clone()));
            }
        }
        if let Some(target) = &self.capture_into
            && self.box_index(target).is_none()
        {
            return Err(ProfileError::UnknownCaptureInto(target.clone()));
        }
        // task ids are server-local, so one plugin must not span servers
        for (j, second) in self.boxes.iter().enumerate() {
            for first in &self.boxes[..j] {
                if first.plugin == second.plugin && server_of(first) != server_of(second) {
                    return Err(ProfileError::MixedServers {
                        plugin: second.plugin.clone(),
                        first: first.id.clone(),
                        second: second.id.clone(),
                    });
                }
            }
        }
        for (j, second) in self.boxes.iter().enumerate() {
            for first in &self.boxes[..j] {
                if let Some((row, col)) = first_shared_cell(first, second) {
                    return Err(ProfileError::OverlappingCells {
                        first: first.id.clone(),
                        second: second.id.clone(),
                        row,
                        col,
                    });
                }
            }
        }
        Ok(())
    }
}

fn server_of(b: &BoxSpec) -> Option<&str> {
    b.config
        .get("url")
        .and_then(toml::Value::as_str)
        .map(|u| u.trim_end_matches('/'))
}

fn first_shared_cell(a: &BoxSpec, b: &BoxSpec) -> Option<(u32, u32)> {
    let rows = |x: &BoxSpec| u32::from(x.row)..u32::from(x.row) + u32::from(x.height);
    let cols = |x: &BoxSpec| u32::from(x.col)..u32::from(x.col) + u32::from(x.width);
    let (a_rows, a_cols) = (rows(a), cols(a));
    rows(b).find_map(|r| {
        cols(b)
            .find(|c| a_rows.contains(&r) && a_cols.contains(c))
            .map(|c| (r, c))
    })
}

pub fn load(path: &Path) -> Result<Profile, ProfileError> {
    let text = std::fs::read_to_string(path).map_err(|source| ProfileError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let profile: Profile = toml::from_str(&text).map_err(|source| ProfileError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    profile.validate()?;
    Ok(profile)
}

/// Seçilen ad dosya yoluna girdiği için dizin ayırıcı ve baştaki nokta reddedilir.
pub fn select_name(
    cli: Option<&str>,
    env: Option<&str>,
    cfg: &Config,
) -> Result<String, ProfileError> {
    let name = [cli, env, cfg.default_profile.as_deref()]
        .into_iter()
        .flatten()
        .find(|s| !s.is_empty())
        .ok_or(ProfileError::NoProfileSelected)?;
    if name.starts_with('.') || name.contains(['/', '\\', '\0']) {
        return Err(ProfileError::InvalidName(name.to_string()));
    }
    Ok(name.to_string())
}

pub fn resolve_name(
    paths: &Paths,
    cli: Option<&str>,
    env: Option<&str>,
    cfg: &Config,
) -> Result<String, ProfileError> {
    match select_name(cli, env, cfg) {
        Err(ProfileError::NoProfileSelected) => {}
        selected => return selected,
    }
    let mut names = existing_names(&paths.config_dir.join("profiles"))?;
    match names.len() {
        0 => create_starter(paths),
        1 => Ok(names.remove(0)),
        _ => Err(ProfileError::NoProfileSelected),
    }
}

fn existing_names(dir: &Path) -> Result<Vec<String>, ProfileError> {
    let read_error = |source| ProfileError::Read {
        path: dir.to_path_buf(),
        source,
    };
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(read_error(e)),
    };
    let mut names = Vec::new();
    for entry in entries {
        let path = entry.map_err(read_error)?.path();
        if path.extension().is_some_and(|ext| ext == "toml")
            && path.is_file()
            && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && !stem.starts_with('.')
        {
            names.push(stem.to_string());
        }
    }
    Ok(names)
}

fn create_starter(paths: &Paths) -> Result<String, ProfileError> {
    create_starter_with(paths, |from, to| std::fs::hard_link(from, to))
}

fn create_starter_with(
    paths: &Paths,
    link: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<String, ProfileError> {
    const NAME: &str = "notes";
    let path = paths.profile_path(NAME);
    let create_error = |source| ProfileError::Create {
        path: path.clone(),
        source,
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(create_error)?;
    // a dot-prefixed name without the .toml extension stays invisible to the profile scan
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = dir.join(format!(
        ".{NAME}.toml.{}.{}.tmp",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let content = starter_profile(&i18n::t(msg::STARTER_TITLE));
    let published = write_synced(&tmp, &content).and_then(|()| match link(&tmp, &path) {
        // filesystems without hard links get a direct exclusive write instead
        Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => write_synced(&path, &content),
        linked => linked,
    });
    if let Err(e) = std::fs::remove_file(&tmp)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        eprintln!(
            "{}",
            i18n::t_args(
                msg::STARTER_TMP_LEFT,
                &[
                    ("path", tmp.display().to_string().as_str().into()),
                    ("error", e.to_string().as_str().into()),
                ],
            )
        );
    }
    match published {
        Ok(()) => {}
        // another Seenboard published a complete file first
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && path.is_file() => {}
        Err(e) => return Err(create_error(e)),
    }
    Ok(NAME.to_string())
}

/// Creates `path` exclusively; a failed write removes the partial file again.
fn write_synced(path: &Path, content: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let written = file
        .write_all(content.as_bytes())
        .and_then(|()| file.sync_all());
    if written.is_err() {
        let _ = std::fs::remove_file(path);
    }
    written
}

pub fn starter_profile(title: &str) -> String {
    let title = toml::Value::String(title.to_string());
    format!(
        "name = {title}\ncapture_into = \"notes\"\n\n[[box]]\nid = \"notes\"\ntitle = {title}\nplugin = \"local\"\nrow = 0\ncol = 0\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::time::Duration;

    const EXAMPLE: &str = r#"
name = "iş"
capture_into = "sozlu"
capture_refs = { R = "https://redmine.example.com/issues/{}" }
capture_label_aliases = { tel = "telefon", yuz = "yuz-yuze", "yüz" = "yuz-yuze" }

[[box]]
id = "sozlu"
title = "Sözlü işler"
plugin = "vikunja"
row = 0
col = 0
refresh = "1m"
config = { url = "https://vikunja.example.com", token_file = "~/.config/seenboard/secrets/vikunja", project = "Home" }

[[box]]
id = "redmine"
title = "Redmine planım"
plugin = "redmine"
row = 0
col = 1
plan_into = "sozlu"
config = { url = "https://redmine.example.com", key_file = "~/.config/seenboard/secrets/redmine" }
"#;

    fn load_str(toml: &str) -> Result<Profile, ProfileError> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.toml");
        std::fs::write(&path, toml).unwrap();
        load(&path)
    }

    fn two_boxes(second: &str) -> String {
        format!(
            "name = \"x\"\n[[box]]\nid = \"sozlu\"\ntitle = \"S\"\nplugin = \"p\"\nrow = 0\ncol = 0\nwidth = 2\n{second}"
        )
    }

    #[test]
    fn loads_example_profile() {
        let p = load_str(EXAMPLE).unwrap();
        assert_eq!(p.name, "iş");
        assert_eq!(p.capture_into.as_deref(), Some("sozlu"));
        assert_eq!(p.capture_refs["R"], "https://redmine.example.com/issues/{}");
        assert_eq!(p.capture_label_aliases["yüz"], "yuz-yuze");
        assert_eq!(p.capture_label_aliases.len(), 3);
        assert_eq!(p.boxes.len(), 2);
        assert_eq!(p.boxes[0].refresh, Duration::from_secs(60));
        assert_eq!(p.boxes[1].refresh, Duration::from_secs(120));
        for b in &p.boxes {
            assert_eq!((b.width, b.height), (1, 1));
        }
        assert_eq!(p.boxes[0].title, "Sözlü işler");
        assert_eq!(p.boxes[1].plan_into.as_deref(), Some("sozlu"));
        assert_eq!(p.boxes[0].config["project"].as_str(), Some("Home"));
        assert_eq!(p.box_index("redmine"), Some(1));
        assert_eq!(p.box_index("yok"), None);
    }

    #[test]
    fn rejects_duplicate_box_id() {
        let t = "name = \"x\"\n[[box]]\nid = \"sozlu\"\ntitle = \"a\"\nplugin = \"p\"\nrow = 0\ncol = 0\n[[box]]\nid = \"sozlu\"\ntitle = \"b\"\nplugin = \"p\"\nrow = 1\ncol = 0\n";
        assert_eq!(
            load_str(t).unwrap_err().to_string(),
            "duplicate box id: sozlu"
        );
    }

    #[test]
    fn rejects_unknown_capture_into() {
        let t = "name = \"x\"\ncapture_into = \"yok\"\n[[box]]\nid = \"a\"\ntitle = \"a\"\nplugin = \"p\"\nrow = 0\ncol = 0\n";
        assert_eq!(
            load_str(t).unwrap_err().to_string(),
            "capture_into points to an unknown box: yok"
        );
    }

    #[test]
    fn rejects_unknown_plan_into() {
        let t = "name = \"x\"\n[[box]]\nid = \"redmine\"\ntitle = \"a\"\nplugin = \"p\"\nrow = 0\ncol = 0\nplan_into = \"yok\"\n";
        assert_eq!(
            load_str(t).unwrap_err().to_string(),
            "plan_into points to an unknown box: yok"
        );
    }

    #[test]
    fn rejects_plan_into_self() {
        let t = "name = \"x\"\n[[box]]\nid = \"redmine\"\ntitle = \"a\"\nplugin = \"p\"\nrow = 0\ncol = 0\nplan_into = \"redmine\"\n";
        assert_eq!(
            load_str(t).unwrap_err().to_string(),
            "box redmine cannot use itself as plan_into"
        );
    }

    #[test]
    fn rejects_overlapping_cells() {
        let t =
            two_boxes("[[box]]\nid = \"fikir\"\ntitle = \"F\"\nplugin = \"p\"\nrow = 0\ncol = 1\n");
        assert_eq!(
            load_str(&t).unwrap_err().to_string(),
            "boxes sozlu and fikir share a cell: row 0 column 1"
        );
    }

    fn url_box(id: &str, col: u16, url: Option<&str>) -> String {
        let cfg = url.map_or(String::new(), |u| format!("config = {{ url = \"{u}\" }}\n"));
        format!(
            "[[box]]\nid = \"{id}\"\ntitle = \"T\"\nplugin = \"vikunja\"\nrow = 0\ncol = {col}\n{cfg}"
        )
    }

    fn url_profile(a: Option<&str>, b: Option<&str>) -> String {
        format!("name = \"x\"\n{}{}", url_box("a", 0, a), url_box("b", 1, b))
    }

    #[test]
    fn rejects_same_plugin_on_different_servers() {
        for (a, b) in [
            (Some("https://one.test"), Some("https://two.test")),
            (Some("https://one.test"), None),
            (None, Some("https://one.test")),
        ] {
            assert_eq!(
                load_str(&url_profile(a, b)).unwrap_err().to_string(),
                "vikunja boxes point to different servers: a, b",
                "{a:?} {b:?}"
            );
        }
    }

    #[test]
    fn same_plugin_on_one_server_is_fine() {
        for (a, b) in [
            (Some("https://one.test"), Some("https://one.test/")),
            (Some("https://one.test/"), Some("https://one.test")),
            (None, None),
        ] {
            assert!(load_str(&url_profile(a, b)).is_ok(), "{a:?} {b:?}");
        }
    }

    #[test]
    fn adjacent_cells_are_fine() {
        let t =
            two_boxes("[[box]]\nid = \"fikir\"\ntitle = \"F\"\nplugin = \"p\"\nrow = 0\ncol = 2\n");
        assert!(load_str(&t).is_ok());
    }

    #[test]
    fn rejects_zero_size_box() {
        let t = "name = \"x\"\n[[box]]\nid = \"a\"\ntitle = \"a\"\nplugin = \"p\"\nrow = 0\ncol = 0\nheight = 0\n";
        assert_eq!(
            load_str(t).unwrap_err().to_string(),
            "width and height of box a must be at least 1"
        );
    }

    #[test]
    fn rejects_bad_refresh() {
        let t = "name = \"x\"\n[[box]]\nid = \"a\"\ntitle = \"a\"\nplugin = \"p\"\nrow = 0\ncol = 0\nrefresh = \"2\"\n";
        assert!(matches!(
            load_str(t).unwrap_err(),
            ProfileError::Parse { .. }
        ));
    }

    #[test]
    fn missing_file_is_read_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = load(&dir.path().join("yok.toml")).unwrap_err();
        assert!(matches!(err, ProfileError::Read { .. }));
        assert!(err.to_string().contains("yok.toml"));
    }

    fn cfg(default: Option<&str>) -> Config {
        Config {
            default_profile: default.map(str::to_string),
            ..Config::default()
        }
    }

    #[test]
    fn select_name_order() {
        assert_eq!(
            select_name(Some("ev"), Some("is"), &cfg(Some("x"))).unwrap(),
            "ev"
        );
        assert_eq!(select_name(None, Some("is"), &cfg(None)).unwrap(), "is");
        assert_eq!(select_name(None, None, &cfg(Some("is"))).unwrap(), "is");
        assert_eq!(
            select_name(None, None, &cfg(None)).unwrap_err().to_string(),
            "no profile selected: pass --profile or set default_profile in config.toml"
        );
    }

    fn paths_in(dir: &tempfile::TempDir) -> Paths {
        Paths {
            config_dir: dir.path().join("config"),
            data_dir: dir.path().join("data"),
            state_dir: dir.path().join("state"),
        }
    }

    fn profile_files(paths: &Paths) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(paths.config_dir.join("profiles"))
            .map(|rd| {
                rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn write_profile(paths: &Paths, file: &str) {
        let dir = paths.config_dir.join("profiles");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(file), "name = \"x\"\n").unwrap();
    }

    #[test]
    fn resolve_name_explicit_name_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(&dir);
        assert_eq!(
            resolve_name(&paths, Some("ev"), None, &cfg(None)).unwrap(),
            "ev"
        );
        assert!(profile_files(&paths).is_empty());
        assert!(!paths.config_dir.exists());
        assert!(matches!(
            resolve_name(&paths, Some("a/b"), None, &cfg(None)).unwrap_err(),
            ProfileError::InvalidName(_)
        ));
    }

    #[test]
    fn resolve_name_single_profile_is_used_without_creating_files() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(&dir);
        write_profile(&paths, "is.toml");
        write_profile(&paths, ".gizli.toml");
        write_profile(&paths, "notlar.txt");
        assert_eq!(resolve_name(&paths, None, None, &cfg(None)).unwrap(), "is");
        assert_eq!(
            profile_files(&paths),
            [".gizli.toml", "is.toml", "notlar.txt"]
        );
    }

    #[test]
    fn resolve_name_without_profiles_writes_the_starter() {
        for create_dir in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let paths = paths_in(&dir);
            if create_dir {
                std::fs::create_dir_all(paths.config_dir.join("profiles")).unwrap();
            }
            assert_eq!(
                resolve_name(&paths, None, None, &cfg(None)).unwrap(),
                "notes"
            );
            assert_eq!(profile_files(&paths), ["notes.toml"]);
            let p = load(&paths.profile_path("notes")).unwrap();
            assert_eq!(p.boxes[0].title, "Notes");
            assert!(!paths.config_dir.join("config.toml").exists());
            assert_eq!(
                resolve_name(&paths, None, None, &cfg(None)).unwrap(),
                "notes"
            );
            assert_eq!(profile_files(&paths), ["notes.toml"]);
        }
    }

    #[test]
    fn resolve_name_reuses_a_complete_starter_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(&dir);
        let existing = starter_profile("Benim");
        let path = paths.profile_path("notes");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &existing).unwrap();
        assert_eq!(
            resolve_name(&paths, None, None, &cfg(None)).unwrap(),
            "notes"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), existing);
        assert_eq!(profile_files(&paths), ["notes.toml"]);
    }

    #[test]
    fn concurrent_first_runs_never_see_a_partial_starter() {
        for _ in 0..50 {
            let dir = tempfile::tempdir().unwrap();
            let paths = paths_in(&dir);
            let barrier = std::sync::Barrier::new(8);
            std::thread::scope(|scope| {
                for _ in 0..8 {
                    scope.spawn(|| {
                        barrier.wait();
                        let name = resolve_name(&paths, None, None, &cfg(None)).unwrap();
                        assert_eq!(name, "notes");
                        let p = load(&paths.profile_path(&name)).unwrap();
                        assert_eq!(p.boxes.len(), 1);
                    });
                }
            });
            assert_eq!(profile_files(&paths), ["notes.toml"]);
        }
    }

    #[test]
    fn starter_falls_back_to_a_direct_write_without_hard_links() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(&dir);
        let unsupported = |_: &Path, _: &Path| Err(std::io::ErrorKind::Unsupported.into());
        assert_eq!(create_starter_with(&paths, unsupported).unwrap(), "notes");
        assert_eq!(profile_files(&paths), ["notes.toml"]);
        let p = load(&paths.profile_path("notes")).unwrap();
        assert_eq!(p.boxes.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn failed_starter_write_leaves_no_profile() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(&dir);
        let profiles = paths.config_dir.join("profiles");
        std::fs::create_dir_all(&profiles).unwrap();
        std::fs::set_permissions(&profiles, std::fs::Permissions::from_mode(0o555)).unwrap();
        let writable = std::fs::File::create(profiles.join(".probe")).is_ok();
        let result = resolve_name(&paths, None, None, &cfg(None));
        std::fs::set_permissions(&profiles, std::fs::Permissions::from_mode(0o755)).unwrap();
        if writable {
            return;
        }
        assert!(matches!(result.unwrap_err(), ProfileError::Create { .. }));
        assert!(profile_files(&paths).is_empty());
    }

    #[test]
    fn resolve_name_with_several_profiles_still_needs_a_choice() {
        let dir = tempfile::tempdir().unwrap();
        let paths = paths_in(&dir);
        write_profile(&paths, "is.toml");
        write_profile(&paths, "ev.toml");
        assert!(matches!(
            resolve_name(&paths, None, None, &cfg(None)).unwrap_err(),
            ProfileError::NoProfileSelected
        ));
        assert_eq!(profile_files(&paths), ["ev.toml", "is.toml"]);
    }

    #[test]
    fn starter_profile_loads_as_a_local_notes_box() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.toml");
        std::fs::write(&path, starter_profile("Notlar")).unwrap();
        let p = load(&path).unwrap();
        assert_eq!(p.name, "Notlar");
        assert_eq!(p.capture_into.as_deref(), Some("notes"));
        assert_eq!(p.boxes.len(), 1);
        let b = &p.boxes[0];
        assert_eq!((b.id.as_str(), b.title.as_str()), ("notes", "Notlar"));
        assert_eq!(b.plugin, "local");
        assert_eq!((b.row, b.col), (0, 0));
    }

    #[test]
    fn starter_profile_escapes_the_title() {
        let title = "a \"b\" \\ c\nd";
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.toml");
        std::fs::write(&path, starter_profile(title)).unwrap();
        let p = load(&path).unwrap();
        assert_eq!(p.name, title);
        assert_eq!(p.boxes[0].title, title);
    }

    #[test]
    fn tr_error_texts_unchanged() {
        let tr = i18n::localizer(Some("tr"));
        let cases = [
            (
                ProfileError::DuplicateBoxId("a".into()),
                "aynı kutu kimliği iki kez: a",
            ),
            (
                ProfileError::UnknownCaptureInto("a".into()),
                "capture_into bilinmeyen kutuya bakıyor: a",
            ),
            (
                ProfileError::PlanIntoSelf("a".into()),
                "a kutusu kendine plan_into veremez",
            ),
            (
                ProfileError::UnknownPlanInto("a".into()),
                "plan_into bilinmeyen kutuya bakıyor: a",
            ),
            (
                ProfileError::MixedServers {
                    plugin: "p".into(),
                    first: "a".into(),
                    second: "b".into(),
                },
                "p kutuları farklı sunuculara bakıyor: a, b",
            ),
            (
                ProfileError::EmptyBox("a".into()),
                "a kutusunun width ve height değeri en az 1 olmalı",
            ),
            (
                ProfileError::OverlappingCells {
                    first: "a".into(),
                    second: "b".into(),
                    row: 0,
                    col: 1,
                },
                "a ve b kutuları aynı hücrede: satır 0 sütun 1",
            ),
            (
                ProfileError::NoProfileSelected,
                "profil seçilmedi: --profile ver ya da config.toml'a default_profile yaz",
            ),
            (
                ProfileError::InvalidName("a/b".into()),
                "geçersiz profil adı: \"a/b\"",
            ),
            (
                ProfileError::Create {
                    path: "/p/notes.toml".into(),
                    source: std::io::Error::other("izin yok"),
                },
                "/p/notes.toml oluşturulamadı: izin yok",
            ),
        ];
        for (error, text) in cases {
            assert_eq!(error.localized(&tr), text);
        }
    }

    #[test]
    fn select_name_skips_empty_values() {
        assert_eq!(select_name(None, Some(""), &cfg(Some("is"))).unwrap(), "is");
    }

    #[test]
    fn select_name_rejects_path_like_names() {
        for bad in ["../x", "a/b", "a\\b", ".gizli", "..", "a\0b"] {
            let err = select_name(Some(bad), None, &cfg(None)).unwrap_err();
            assert!(matches!(err, ProfileError::InvalidName(_)), "{bad:?}");
        }
        assert_eq!(select_name(Some("iş"), None, &cfg(None)).unwrap(), "iş");
    }
}

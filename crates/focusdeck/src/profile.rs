use crate::config::Config;
use crate::duration;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

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

#[derive(Debug, Error)]
pub enum ProfileError {
    #[error("{} okunamadı: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{} geçersiz: {source}", path.display())]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("aynı kutu kimliği iki kez: {0}")]
    DuplicateBoxId(String),
    #[error("capture_into bilinmeyen kutuya bakıyor: {0}")]
    UnknownCaptureInto(String),
    #[error("{0} kutusu kendine plan_into veremez")]
    PlanIntoSelf(String),
    #[error("plan_into bilinmeyen kutuya bakıyor: {0}")]
    UnknownPlanInto(String),
    #[error("{plugin} kutuları farklı sunuculara bakıyor: {first}, {second}")]
    MixedServers {
        plugin: String,
        first: String,
        second: String,
    },
    #[error("{0} kutusunun width ve height değeri en az 1 olmalı")]
    EmptyBox(String),
    #[error("{first} ve {second} kutuları aynı hücrede: satır {row} sütun {col}")]
    OverlappingCells {
        first: String,
        second: String,
        row: u32,
        col: u32,
    },
    #[error("profil seçilmedi: --profile ver ya da config.toml'a default_profile yaz")]
    NoProfileSelected,
    #[error("geçersiz profil adı: {0:?}")]
    InvalidName(String),
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::time::Duration;

    const EXAMPLE: &str = r#"
name = "iş"
capture_into = "sozlu"
capture_refs = { R = "https://destek.mono.tr/issues/{}" }
capture_label_aliases = { tel = "telefon", yuz = "yuz-yuze", "yüz" = "yuz-yuze" }

[[box]]
id = "sozlu"
title = "Sözlü işler"
plugin = "vikunja"
row = 0
col = 0
refresh = "1m"
config = { url = "https://vikunja.omertahaoztop.tr", token_file = "~/.config/focusdeck/secrets/vikunja", project = "Mono" }

[[box]]
id = "redmine"
title = "Redmine planım"
plugin = "redmine"
row = 0
col = 1
plan_into = "sozlu"
config = { url = "https://destek.mono.tr", key_file = "~/.config/focusdeck/secrets/redmine" }
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
        assert_eq!(p.capture_refs["R"], "https://destek.mono.tr/issues/{}");
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
        assert_eq!(p.boxes[0].config["project"].as_str(), Some("Mono"));
        assert_eq!(p.box_index("redmine"), Some(1));
        assert_eq!(p.box_index("yok"), None);
    }

    #[test]
    fn rejects_duplicate_box_id() {
        let t = "name = \"x\"\n[[box]]\nid = \"sozlu\"\ntitle = \"a\"\nplugin = \"p\"\nrow = 0\ncol = 0\n[[box]]\nid = \"sozlu\"\ntitle = \"b\"\nplugin = \"p\"\nrow = 1\ncol = 0\n";
        assert_eq!(
            load_str(t).unwrap_err().to_string(),
            "aynı kutu kimliği iki kez: sozlu"
        );
    }

    #[test]
    fn rejects_unknown_capture_into() {
        let t = "name = \"x\"\ncapture_into = \"yok\"\n[[box]]\nid = \"a\"\ntitle = \"a\"\nplugin = \"p\"\nrow = 0\ncol = 0\n";
        assert_eq!(
            load_str(t).unwrap_err().to_string(),
            "capture_into bilinmeyen kutuya bakıyor: yok"
        );
    }

    #[test]
    fn rejects_unknown_plan_into() {
        let t = "name = \"x\"\n[[box]]\nid = \"redmine\"\ntitle = \"a\"\nplugin = \"p\"\nrow = 0\ncol = 0\nplan_into = \"yok\"\n";
        assert_eq!(
            load_str(t).unwrap_err().to_string(),
            "plan_into bilinmeyen kutuya bakıyor: yok"
        );
    }

    #[test]
    fn rejects_plan_into_self() {
        let t = "name = \"x\"\n[[box]]\nid = \"redmine\"\ntitle = \"a\"\nplugin = \"p\"\nrow = 0\ncol = 0\nplan_into = \"redmine\"\n";
        assert_eq!(
            load_str(t).unwrap_err().to_string(),
            "redmine kutusu kendine plan_into veremez"
        );
    }

    #[test]
    fn rejects_overlapping_cells() {
        let t =
            two_boxes("[[box]]\nid = \"fikir\"\ntitle = \"F\"\nplugin = \"p\"\nrow = 0\ncol = 1\n");
        assert_eq!(
            load_str(&t).unwrap_err().to_string(),
            "sozlu ve fikir kutuları aynı hücrede: satır 0 sütun 1"
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
                "vikunja kutuları farklı sunuculara bakıyor: a, b",
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
            "a kutusunun width ve height değeri en az 1 olmalı"
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
            "profil seçilmedi: --profile ver ya da config.toml'a default_profile yaz"
        );
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

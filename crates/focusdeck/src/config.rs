use crate::i18n::{msg, t_args};
use crate::paths::Paths;
use anyhow::Context;
use serde::Deserialize;
use std::io::ErrorKind;

pub const DEFAULT_REGISTRY: &str =
    "https://raw.githubusercontent.com/omertahaoztop/focusdeck-plugins/main/index.toml";

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CaptureWords {
    pub tomorrow: Vec<String>,
    pub minute: Vec<String>,
    pub hour: Vec<String>,
    pub mon: Vec<String>,
    pub tue: Vec<String>,
    pub wed: Vec<String>,
    pub thu: Vec<String>,
    pub fri: Vec<String>,
    pub sat: Vec<String>,
    pub sun: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub language: Option<String>,
    pub capture_words: CaptureWords,
    pub default_profile: Option<String>,
    pub registries: Vec<String>,
    pub capture_command: Option<Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            language: None,
            capture_words: CaptureWords::default(),
            default_profile: None,
            registries: vec![DEFAULT_REGISTRY.to_string()],
            capture_command: None,
        }
    }
}

impl Config {
    pub fn load(paths: &Paths) -> anyhow::Result<Config> {
        let path = paths.config_dir.join("config.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(e).with_context(|| path_text(msg::CONFIG_READ_FAILED, &path)),
        };
        toml::from_str(&text).with_context(|| path_text(msg::CONFIG_INVALID, &path))
    }
}

fn path_text(id: &str, path: &std::path::Path) -> String {
    t_args(id, &[("path", path.display().to_string().as_str().into())])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;

    fn paths(dir: &std::path::Path) -> Paths {
        Paths {
            config_dir: dir.to_path_buf(),
            data_dir: dir.join("data"),
            state_dir: dir.join("state"),
        }
    }

    #[test]
    fn language_and_capture_words_parse() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "language = \"tr\"\n[capture_words]\ntomorrow = [\"tmrw\"]\n",
        )
        .unwrap();
        let cfg = Config::load(&paths(dir.path())).unwrap();
        assert_eq!(cfg.language.as_deref(), Some("tr"));
        assert_eq!(cfg.capture_words.tomorrow, ["tmrw"]);
        assert!(cfg.capture_words.minute.is_empty());

        std::fs::write(
            dir.path().join("config.toml"),
            "[capture_words]\ntomorow = [\"x\"]\n",
        )
        .unwrap();
        assert!(Config::load(&paths(dir.path())).is_err());

        let empty = tempfile::tempdir().unwrap();
        assert_eq!(Config::load(&paths(empty.path())).unwrap().language, None);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load(&paths(dir.path())).unwrap();
        assert_eq!(cfg.default_profile, None);
        assert_eq!(cfg.registries, vec![DEFAULT_REGISTRY.to_string()]);
        assert_eq!(cfg.capture_command, None);
    }

    #[test]
    fn reads_values() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "default_profile = \"is\"\nregistries = [\"https://a/index.toml\", \"https://b/index.toml\"]\ncapture_command = [\"fuzzel\", \"--dmenu\"]\n",
        )
        .unwrap();
        let cfg = Config::load(&paths(dir.path())).unwrap();
        assert_eq!(cfg.default_profile.as_deref(), Some("is"));
        assert_eq!(
            cfg.registries,
            ["https://a/index.toml", "https://b/index.toml"]
        );
        assert_eq!(
            cfg.capture_command,
            Some(vec!["fuzzel".to_string(), "--dmenu".to_string()])
        );
    }

    #[test]
    fn absent_registries_key_keeps_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "default_profile = \"ev\"\n").unwrap();
        let cfg = Config::load(&paths(dir.path())).unwrap();
        assert_eq!(cfg.registries, vec![DEFAULT_REGISTRY.to_string()]);
    }

    #[test]
    fn unknown_top_level_key_names_itself() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "registires = [\"https://a/index.toml\"]\n",
        )
        .unwrap();
        let err = Config::load(&paths(dir.path())).unwrap_err();
        assert!(format!("{err:#}").contains("registires"), "{err:#}");
    }

    #[test]
    fn invalid_toml_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "default_profile = [").unwrap();
        let err = Config::load(&paths(dir.path())).unwrap_err();
        assert!(format!("{err:#}").contains("config.toml"));
    }
}

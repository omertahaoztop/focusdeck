use crate::paths::Paths;
use anyhow::Context;
use serde::Deserialize;
use std::io::ErrorKind;

pub const DEFAULT_REGISTRY: &str =
    "https://raw.githubusercontent.com/omertahaoztop/focusdeck-plugins/main/index.toml";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_profile: Option<String>,
    pub registries: Vec<String>,
    pub capture_command: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_profile: None,
            registries: vec![DEFAULT_REGISTRY.to_string()],
            capture_command: ["rofi", "-dmenu", "-p", "Not"].map(String::from).to_vec(),
        }
    }
}

impl Config {
    pub fn load(paths: &Paths) -> anyhow::Result<Config> {
        let path = paths.config_dir.join("config.toml");
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(e).with_context(|| format!("{} okunamadı", path.display())),
        };
        toml::from_str(&text).with_context(|| format!("{} geçersiz", path.display()))
    }
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
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load(&paths(dir.path())).unwrap();
        assert_eq!(cfg.default_profile, None);
        assert_eq!(cfg.registries, vec![DEFAULT_REGISTRY.to_string()]);
        assert_eq!(cfg.capture_command, ["rofi", "-dmenu", "-p", "Not"]);
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
        assert_eq!(cfg.capture_command, ["fuzzel", "--dmenu"]);
    }

    #[test]
    fn absent_registries_key_keeps_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "default_profile = \"ev\"\n").unwrap();
        let cfg = Config::load(&paths(dir.path())).unwrap();
        assert_eq!(cfg.registries, vec![DEFAULT_REGISTRY.to_string()]);
    }

    #[test]
    fn invalid_toml_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.toml"), "default_profile = [").unwrap();
        let err = Config::load(&paths(dir.path())).unwrap_err();
        assert!(format!("{err:#}").contains("config.toml"));
    }
}

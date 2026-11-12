use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl Paths {
    pub fn from_env() -> Paths {
        Paths::from_lookup(|k| std::env::var_os(k))
    }

    fn from_lookup(get: impl Fn(&str) -> Option<OsString>) -> Paths {
        let resolve = |var: &str, default: Option<PathBuf>| {
            get(var)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    default
                        .unwrap_or_else(|| PathBuf::from("."))
                        .join("focusdeck")
                })
        };
        Paths {
            config_dir: resolve("FOCUSDECK_CONFIG_DIR", dirs::config_dir()),
            data_dir: resolve("FOCUSDECK_DATA_DIR", dirs::data_dir()),
            state_dir: resolve("FOCUSDECK_STATE_DIR", dirs::state_dir()),
        }
    }

    pub fn profile_path(&self, name: &str) -> PathBuf {
        self.config_dir
            .join("profiles")
            .join(format!("{name}.toml"))
    }

    pub fn plugin_dir(&self) -> PathBuf {
        self.data_dir.join("plugins")
    }

    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("focusdeck.log")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;

    fn lookup(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<OsString> {
        move |k| {
            pairs
                .iter()
                .find(|(n, _)| *n == k)
                .map(|(_, v)| OsString::from(v))
        }
    }

    #[test]
    fn env_overrides_win() {
        let p = Paths::from_lookup(lookup(&[
            ("FOCUSDECK_CONFIG_DIR", "/c"),
            ("FOCUSDECK_DATA_DIR", "/d"),
            ("FOCUSDECK_STATE_DIR", "/s"),
        ]));
        assert_eq!(p.config_dir, PathBuf::from("/c"));
        assert_eq!(p.data_dir, PathBuf::from("/d"));
        assert_eq!(p.state_dir, PathBuf::from("/s"));
        assert_eq!(p.profile_path("is"), PathBuf::from("/c/profiles/is.toml"));
        assert_eq!(p.plugin_dir(), PathBuf::from("/d/plugins"));
        assert_eq!(p.log_file(), PathBuf::from("/s/focusdeck.log"));
    }

    #[test]
    fn empty_override_falls_back_to_default() {
        let p = Paths::from_lookup(lookup(&[("FOCUSDECK_CONFIG_DIR", "")]));
        assert!(p.config_dir.ends_with("focusdeck"));
        assert!(!p.config_dir.as_os_str().is_empty());
    }
}

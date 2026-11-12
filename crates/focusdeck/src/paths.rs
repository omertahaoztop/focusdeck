use std::ffi::OsString;
use std::path::PathBuf;

use crate::i18n::{msg, t_args};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl Paths {
    pub fn from_env() -> Result<Paths, String> {
        Paths::from_lookup(
            |k| std::env::var_os(k),
            [dirs::config_dir(), dirs::data_dir(), dirs::state_dir()],
        )
    }

    fn from_lookup(
        get: impl Fn(&str) -> Option<OsString>,
        defaults: [Option<PathBuf>; 3],
    ) -> Result<Paths, String> {
        let resolve = |var: &str, default: Option<PathBuf>| {
            get(var)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .or_else(|| default.map(|d| d.join("focusdeck")))
                .ok_or_else(|| t_args(msg::ERROR_NO_BASE_DIR, &[("var", var.into())]))
        };
        let [config, data, state] = defaults;
        Ok(Paths {
            config_dir: resolve("FOCUSDECK_CONFIG_DIR", config)?,
            data_dir: resolve("FOCUSDECK_DATA_DIR", data)?,
            state_dir: resolve("FOCUSDECK_STATE_DIR", state)?,
        })
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

    fn home() -> [Option<PathBuf>; 3] {
        ["/h/.config", "/h/.local/share", "/h/.local/state"].map(|d| Some(PathBuf::from(d)))
    }

    #[test]
    fn env_overrides_win_without_a_home() {
        let p = Paths::from_lookup(
            lookup(&[
                ("FOCUSDECK_CONFIG_DIR", "/c"),
                ("FOCUSDECK_DATA_DIR", "/d"),
                ("FOCUSDECK_STATE_DIR", "/s"),
            ]),
            [None, None, None],
        )
        .unwrap();
        assert_eq!(p.config_dir, PathBuf::from("/c"));
        assert_eq!(p.data_dir, PathBuf::from("/d"));
        assert_eq!(p.state_dir, PathBuf::from("/s"));
        assert_eq!(p.profile_path("is"), PathBuf::from("/c/profiles/is.toml"));
        assert_eq!(p.plugin_dir(), PathBuf::from("/d/plugins"));
        assert_eq!(p.log_file(), PathBuf::from("/s/focusdeck.log"));
    }

    #[test]
    fn empty_override_falls_back_to_default() {
        let p = Paths::from_lookup(lookup(&[("FOCUSDECK_CONFIG_DIR", "")]), home()).unwrap();
        assert_eq!(p.config_dir, PathBuf::from("/h/.config/focusdeck"));
        assert_eq!(p.data_dir, PathBuf::from("/h/.local/share/focusdeck"));
        assert_eq!(p.state_dir, PathBuf::from("/h/.local/state/focusdeck"));
    }

    #[test]
    fn no_override_and_no_home_is_an_error() {
        let mut defaults = home();
        defaults[2] = None;
        let err = Paths::from_lookup(lookup(&[]), defaults).unwrap_err();
        assert_eq!(
            err,
            "FOCUSDECK_STATE_DIR is not set and no home directory was found"
        );
    }
}

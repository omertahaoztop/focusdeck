use crate::profile::BoxSpec;
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("{} bulunamadı", .0.display())]
    Missing(PathBuf),
    #[error("{} dosyasını başkaları okuyabiliyor, chmod 600 yap", .0.display())]
    Exposed(PathBuf),
    #[error("{} denetlenemedi: {source}", path.display())]
    Unreadable {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Şifre dosyalarının yalnız varlığına ve iznine bakar, içeriğini hiç okumaz.
pub fn plugin_config(spec: &BoxSpec, home: &Path) -> Result<Value, SecretError> {
    let mut value = serde_json::to_value(&spec.config).expect("toml tablosu JSON'a çevrilir");
    check_files(&mut value, home)?;
    Ok(value)
}

fn check_files(value: &mut Value, home: &Path) -> Result<(), SecretError> {
    match value {
        Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                match v {
                    Value::String(s) if key.ends_with("_file") => {
                        let path = expand_home(s, home);
                        check_secret(&path)?;
                        *s = path.to_string_lossy().into_owned();
                    }
                    other => check_files(other, home)?,
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                check_files(item, home)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn expand_home(s: &str, home: &Path) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(s),
    }
}

fn check_secret(path: &Path) -> Result<(), SecretError> {
    let meta = std::fs::metadata(path).map_err(|source| match source.kind() {
        std::io::ErrorKind::NotFound => SecretError::Missing(path.to_path_buf()),
        _ => SecretError::Unreadable {
            path: path.to_path_buf(),
            source,
        },
    })?;
    if meta.permissions().mode() & 0o077 != 0 {
        return Err(SecretError::Exposed(path.to_path_buf()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::BoxSpec;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    fn spec(config: &str) -> BoxSpec {
        let config: toml::Table = config.parse().unwrap();
        BoxSpec {
            id: "b".into(),
            title: "B".into(),
            plugin: "p".into(),
            row: 0,
            col: 0,
            width: 1,
            height: 1,
            refresh: std::time::Duration::from_secs(120),
            plan_into: None,
            config,
        }
    }

    fn write_secret(path: &Path, mode: u32) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "super-secret-value").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn expands_tilde_in_file_keys() {
        let home = tempfile::tempdir().unwrap();
        write_secret(&home.path().join("s/v"), 0o600);
        let spec = spec("url = \"~/not-a-file\"\ntoken_file = \"~/s/v\"\nproject = \"Mono\"");
        let json = plugin_config(&spec, home.path()).unwrap();
        assert_eq!(
            json["token_file"],
            home.path().join("s/v").to_str().unwrap()
        );
        assert_eq!(json["url"], "~/not-a-file");
        assert_eq!(json["project"], "Mono");
    }

    #[test]
    fn rejects_group_readable_secret() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("s/v");
        write_secret(&path, 0o640);
        let err = plugin_config(&spec("token_file = \"~/s/v\""), home.path()).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                "{} dosyasını başkaları okuyabiliyor, chmod 600 yap",
                path.display()
            )
        );
    }

    #[test]
    fn rejects_world_readable_secret() {
        let home = tempfile::tempdir().unwrap();
        write_secret(&home.path().join("v"), 0o604);
        let err = plugin_config(&spec("key_file = \"~/v\""), home.path()).unwrap_err();
        assert!(matches!(err, SecretError::Exposed(_)));
    }

    #[test]
    fn rejects_missing_secret() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("yok");
        let err = plugin_config(&spec("token_file = \"~/yok\""), home.path()).unwrap_err();
        assert_eq!(err.to_string(), format!("{} bulunamadı", path.display()));
    }

    #[test]
    fn accepts_0600_secret() {
        let home = tempfile::tempdir().unwrap();
        write_secret(&home.path().join("s/v"), 0o600);
        let json = plugin_config(&spec("token_file = \"~/s/v\""), home.path()).unwrap();
        assert!(!json.to_string().contains("super-secret-value"));
        assert_eq!(
            json["token_file"],
            home.path().join("s/v").to_str().unwrap()
        );
    }

    #[test]
    fn accepts_0400_secret() {
        let home = tempfile::tempdir().unwrap();
        write_secret(&home.path().join("v"), 0o400);
        assert!(plugin_config(&spec("key_file = \"~/v\""), home.path()).is_ok());
    }

    #[test]
    fn absolute_path_is_checked_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k");
        write_secret(&path, 0o660);
        let cfg = format!("key_file = {:?}", path.to_str().unwrap());
        let err = plugin_config(&spec(&cfg), Path::new("/nonexistent-home")).unwrap_err();
        assert!(matches!(err, SecretError::Exposed(p) if p == path));
    }

    #[test]
    fn symlink_is_judged_by_target_mode() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("real");
        write_secret(&target, 0o644);
        std::os::unix::fs::symlink(&target, home.path().join("link")).unwrap();
        let err = plugin_config(&spec("token_file = \"~/link\""), home.path()).unwrap_err();
        assert!(matches!(err, SecretError::Exposed(_)));
    }

    #[test]
    fn nested_file_keys_are_checked_too() {
        let home = tempfile::tempdir().unwrap();
        let cfg = "[auth]\nkey_file = \"~/yok\"\n";
        let spec = spec(cfg);
        let err = plugin_config(&spec, home.path()).unwrap_err();
        assert!(matches!(err, SecretError::Missing(_)));
    }

    #[test]
    fn non_file_keys_are_never_touched() {
        let home = tempfile::tempdir().unwrap();
        let json =
            plugin_config(&spec("profile = \"~/yok\"\nfiles = \"~/yok\""), home.path()).unwrap();
        assert_eq!(json["profile"], "~/yok");
        assert_eq!(json["files"], "~/yok");
    }

    #[test]
    fn error_text_has_no_file_contents() {
        let home = tempfile::tempdir().unwrap();
        write_secret(&home.path().join("v"), 0o644);
        let err = plugin_config(&spec("token_file = \"~/v\""), home.path()).unwrap_err();
        assert!(!err.to_string().contains("super-secret-value"));
    }
}

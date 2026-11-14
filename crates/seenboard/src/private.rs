//! Files that hold notes or plugin output are created readable by their owner only.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

pub const LOG_LIMIT: u64 = 1024 * 1024;
const OWNER_ONLY: u32 = 0o600;

pub fn append(path: &Path) -> io::Result<File> {
    open(path, OpenOptions::new().read(true).append(true))
}

pub fn log(path: &Path) -> io::Result<File> {
    if fs::metadata(path).is_ok_and(|m| m.len() > LOG_LIMIT) {
        // a log that cannot be moved keeps growing, which beats losing the new lines
        let _ = fs::rename(path, rotated(path));
    }
    append(path)
}

pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    open(path, OpenOptions::new().write(true).truncate(true))?.write_all(bytes)
}

fn rotated(path: &Path) -> PathBuf {
    let mut name = OsString::from(path);
    name.push(".1");
    PathBuf::from(name)
}

fn open(path: &Path, options: &mut OpenOptions) -> io::Result<File> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let file = options.create(true).mode(OWNER_ONLY).open(path)?;
    // the mode above only applies to a new file; one from an older version may be wider
    if file.metadata()?.permissions().mode() & 0o077 != 0 {
        file.set_permissions(fs::Permissions::from_mode(OWNER_ONLY))?;
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn new_file_is_owner_only_and_its_directory_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state/queue.jsonl");
        append(&path).unwrap().write_all(b"a\n").unwrap();
        append(&path).unwrap().write_all(b"b\n").unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(fs::read(&path).unwrap(), b"a\nb\n");
    }

    #[test]
    fn wider_existing_file_is_tightened() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seenboard.log");
        fs::write(&path, "old\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        log(&path).unwrap().write_all(b"new\n").unwrap();
        assert_eq!(mode(&path), 0o600);
        assert_eq!(fs::read(&path).unwrap(), b"old\nnew\n");
    }

    #[test]
    fn log_over_the_limit_moves_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seenboard.log");
        let old = vec![b'x'; usize::try_from(LOG_LIMIT).unwrap() + 1];
        fs::write(&path, &old).unwrap();
        log(&path).unwrap().write_all(b"new\n").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new\n");
        assert_eq!(fs::read(dir.path().join("seenboard.log.1")).unwrap(), old);
        assert_eq!(mode(&path), 0o600);
    }

    #[test]
    fn log_at_the_limit_stays() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("seenboard.log");
        fs::write(&path, vec![b'x'; usize::try_from(LOG_LIMIT).unwrap()]).unwrap();
        drop(log(&path).unwrap());
        assert!(!dir.path().join("seenboard.log.1").exists());
    }

    #[test]
    fn write_replaces_the_content_and_is_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reminded.json");
        write(&path, b"a much longer first version").unwrap();
        write(&path, b"{}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{}");
        assert_eq!(mode(&path), 0o600);
    }
}

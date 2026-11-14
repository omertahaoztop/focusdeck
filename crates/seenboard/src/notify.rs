//! Desktop notifications via notify-send; failures go to the log and are reported as `false`.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn send_logged(log: &Path, title: &str, body: &str, critical: bool) -> bool {
    let mut cmd = Command::new("notify-send");
    if critical {
        cmd.args(["-u", "critical"]);
    }
    // "--" keeps a note that starts with a dash from being read as an option
    cmd.args(["--", title, body])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match cmd.status() {
        Ok(status) if status.success() => true,
        Ok(status) => {
            log_line(log, &format!("notify-send exited with {status}"));
            false
        }
        Err(e) => {
            log_line(log, &format!("could not start notify-send: {e}"));
            false
        }
    }
}

pub fn log_line(log: &Path, message: &str) {
    if let Ok(mut file) = crate::private::log(log) {
        let _ = writeln!(file, "[seenboard] {message}");
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn log_line_writes_an_owner_only_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("state/seenboard.log");
        log_line(&log, "hello");
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "[seenboard] hello\n"
        );
        assert_eq!(
            std::fs::metadata(&log).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

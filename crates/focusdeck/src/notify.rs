//! Desktop notifications via notify-send; failures go to the log and never reach the caller.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::paths::Paths;

pub fn send(title: &str, body: &str, critical: bool) {
    send_logged(&Paths::from_env().log_file(), title, body, critical);
}

pub fn send_logged(log: &Path, title: &str, body: &str, critical: bool) {
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
        Ok(status) if status.success() => {}
        Ok(status) => log_line(log, &format!("notify-send {status} ile çıktı")),
        Err(e) => log_line(log, &format!("notify-send başlatılamadı: {e}")),
    }
}

pub fn log_line(log: &Path, message: &str) {
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
    {
        let _ = writeln!(file, "[focusdeck] {message}");
    }
}

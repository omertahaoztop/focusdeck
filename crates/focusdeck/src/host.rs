//! Runs one plugin process and speaks line-delimited JSON with it.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use focusdeck_i18n::Localizer;
use focusdeck_protocol::{Describe, PROTOCOL_VERSION, Request, Response};

use crate::i18n::{self, msg, t, t_args};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
const BAD_LINE_LIMIT: usize = 200;
const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
const MAX_STDERR_LINE: usize = 64 * 1024;
const LOG_RETRY_STEP: u64 = 64 * 1024;

#[derive(Debug)]
pub enum HostError {
    Spawn(String),
    Timeout,
    Crashed,
    BadLine(String),
    Plugin(String),
    Protocol { expected: u32, got: u32 },
}

impl HostError {
    pub fn localized(&self, l: &Localizer) -> String {
        match self {
            HostError::Spawn(error) => {
                l.t_args(msg::HOST_SPAWN_FAILED, &[("error", error.as_str().into())])
            }
            HostError::Timeout => l.t(msg::HOST_TIMEOUT),
            HostError::Crashed => l.t(msg::HOST_CRASHED),
            HostError::BadLine(line) => {
                l.t_args(msg::HOST_BAD_LINE, &[("line", line.as_str().into())])
            }
            HostError::Plugin(message) => message.clone(),
            HostError::Protocol { expected, got } => l.t_args(
                msg::HOST_PROTOCOL_MISMATCH,
                &[
                    ("expected", expected.to_string().as_str().into()),
                    ("got", got.to_string().as_str().into()),
                ],
            ),
        }
    }
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.localized(i18n::current()))
    }
}

impl std::error::Error for HostError {}

struct Proc {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    max_line: usize,
    next_id: u64,
}

impl Proc {
    async fn kill(mut self) {
        let _ = self.child.kill().await;
    }
}

pub struct PluginHost {
    label: String,
    exe: PathBuf,
    args: Vec<String>,
    config: Value,
    log: PathBuf,
    lang: Option<String>,
    timeout: Duration,
    max_line: usize,
    proc: Option<Proc>,
    describe: Option<Describe>,
}

impl PluginHost {
    pub fn new(label: String, exe: PathBuf, config: Value, log: PathBuf) -> PluginHost {
        PluginHost {
            label,
            exe,
            args: Vec::new(),
            config,
            log,
            lang: None,
            timeout: DEFAULT_TIMEOUT,
            max_line: MAX_LINE_BYTES,
            proc: None,
            describe: None,
        }
    }

    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    pub fn with_lang(mut self, lang: Option<String>) -> Self {
        self.lang = lang;
        self
    }

    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
        self
    }

    pub fn with_max_line(mut self, bytes: usize) -> Self {
        self.max_line = bytes;
        self
    }

    pub async fn describe(&mut self) -> Result<Describe, HostError> {
        self.ensure_started().await?;
        Ok(self.describe.clone().expect("describe is stored on start"))
    }

    pub async fn call<P: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        params: &P,
    ) -> Result<R, HostError> {
        let params = serde_json::to_value(params).map_err(|e| HostError::Plugin(e.to_string()))?;
        self.ensure_started().await?;
        // taken out while the request is in flight: if this future is dropped the child dies with it
        // instead of leaving an unread answer behind for the next request
        let mut proc = self.proc.take().expect("process is running after start");
        match exchange(&mut proc, method, params, self.timeout).await {
            Ok(value) => match serde_json::from_value(value) {
                Ok(result) => {
                    self.proc = Some(proc);
                    Ok(result)
                }
                Err(e) => {
                    proc.kill().await;
                    Err(HostError::BadLine(t_args(
                        msg::HOST_INVALID_RESULT,
                        &[("error", e.to_string().as_str().into())],
                    )))
                }
            },
            Err(HostError::Plugin(message)) => {
                self.proc = Some(proc);
                Err(HostError::Plugin(message))
            }
            Err(e) => {
                proc.kill().await;
                Err(e)
            }
        }
    }

    async fn ensure_started(&mut self) -> Result<(), HostError> {
        if self.proc.is_some() {
            return Ok(());
        }
        let mut proc = self.spawn()?;
        match self.handshake(&mut proc).await {
            Ok(describe) => {
                self.describe = Some(describe);
                self.proc = Some(proc);
                Ok(())
            }
            Err(e) => {
                proc.kill().await;
                Err(e)
            }
        }
    }

    async fn handshake(&self, proc: &mut Proc) -> Result<Describe, HostError> {
        let value = exchange(proc, "describe", Value::Null, self.timeout).await?;
        let describe: Describe = serde_json::from_value(value).map_err(|e| {
            HostError::BadLine(t_args(
                msg::HOST_INVALID_DESCRIBE,
                &[("error", e.to_string().as_str().into())],
            ))
        })?;
        if describe.protocol != PROTOCOL_VERSION {
            return Err(HostError::Protocol {
                expected: PROTOCOL_VERSION,
                got: describe.protocol,
            });
        }
        exchange(proc, "configure", self.config.clone(), self.timeout).await?;
        Ok(describe)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.exe);
        command.args(&self.args);
        match &self.lang {
            Some(lang) => command.env("FOCUSDECK_LANG", lang),
            None => command.env_remove("FOCUSDECK_LANG"),
        };
        command
    }

    fn spawn(&self) -> Result<Proc, HostError> {
        let mut child = self
            .command()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| HostError::Spawn(e.to_string()))?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        tokio::spawn(log_stderr(self.label.clone(), stderr, self.log.clone()));
        Ok(Proc {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            max_line: self.max_line,
            next_id: 0,
        })
    }
}

async fn exchange(
    proc: &mut Proc,
    method: &str,
    params: Value,
    limit: Duration,
) -> Result<Value, HostError> {
    proc.next_id += 1;
    let id = proc.next_id;
    let request = Request {
        id,
        method: method.into(),
        params,
    };
    let mut line = serde_json::to_string(&request).map_err(|e| HostError::Plugin(e.to_string()))?;
    line.push('\n');
    let round_trip = async {
        proc.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|_| HostError::Crashed)?;
        proc.stdin.flush().await.map_err(|_| HostError::Crashed)?;
        let bytes = match read_capped(&mut proc.stdout, proc.max_line, false).await {
            Ok(Some(line)) if line.overflowed => {
                return Err(HostError::BadLine(t(msg::HOST_LINE_TOO_LONG)));
            }
            Ok(Some(line)) => line.bytes,
            Ok(None) | Err(_) => return Err(HostError::Crashed),
        };
        let mut reply =
            String::from_utf8(bytes).map_err(|_| HostError::BadLine(t(msg::HOST_NON_UTF8)))?;
        if reply.ends_with('\r') {
            reply.pop();
        }
        let response: Response =
            serde_json::from_str(&reply).map_err(|_| HostError::BadLine(clip(&reply)))?;
        if response.id != id {
            return Err(HostError::BadLine(clip(&reply)));
        }
        match response.error {
            Some(error) => Err(HostError::Plugin(error.message)),
            None => Ok(response.result.unwrap_or(Value::Null)),
        }
    };
    tokio::time::timeout(limit, round_trip)
        .await
        .map_err(|_| HostError::Timeout)?
}

fn clip(line: &str) -> String {
    line.chars().take(BAD_LINE_LIMIT).collect()
}

struct Capped {
    bytes: Vec<u8>,
    overflowed: bool,
}

// with `drain` the rest of an over-long line is read and dropped; without it the read stops at the first excess byte
async fn read_capped<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
    drain: bool,
) -> std::io::Result<Option<Capped>> {
    let mut bytes = Vec::new();
    let mut overflowed = false;
    loop {
        let buf = reader.fill_buf().await?;
        if buf.is_empty() {
            return Ok((!bytes.is_empty() || overflowed).then_some(Capped { bytes, overflowed }));
        }
        let newline = buf.iter().position(|&b| b == b'\n');
        let chunk = &buf[..newline.unwrap_or(buf.len())];
        let take = chunk.len().min(max - bytes.len());
        bytes.extend_from_slice(&chunk[..take]);
        overflowed |= take < chunk.len();
        let used = chunk.len() + usize::from(newline.is_some());
        reader.consume(used);
        if newline.is_some() || (overflowed && !drain) {
            return Ok(Some(Capped { bytes, overflowed }));
        }
    }
}

// A failed rotation leaves the file over the limit, so the next try waits for a step of growth; a handle that cannot be inspected is dropped instead of reopened on every line.
async fn open_log(path: &Path) -> Option<(tokio::fs::File, u64)> {
    let file = tokio::fs::File::from_std(crate::private::log(path).ok()?);
    let len = file.metadata().await.ok()?.len();
    let limit = if len > crate::private::LOG_LIMIT {
        len + LOG_RETRY_STEP
    } else {
        crate::private::LOG_LIMIT
    };
    Some((file, limit))
}

// The handle is replaced when it is past the limit, cannot be inspected, or is no longer the file at the path because someone rotated or deleted the log.
async fn needs_reopen(file: &tokio::fs::File, path: &Path, limit: u64) -> bool {
    let Ok(open) = file.metadata().await else {
        return true;
    };
    if open.len() > limit {
        return true;
    }
    match tokio::fs::metadata(path).await {
        Ok(named) => (named.dev(), named.ino()) != (open.dev(), open.ino()),
        Err(_) => true,
    }
}

async fn log_stderr<R: AsyncRead + Unpin>(label: String, stderr: R, path: PathBuf) {
    let mut reader = BufReader::new(stderr);
    let mut log = open_log(&path).await;
    while let Ok(Some(line)) = read_capped(&mut reader, MAX_STDERR_LINE, true).await {
        if let Some((f, limit)) = &log
            && needs_reopen(f, &path, *limit).await
        {
            log = open_log(&path).await;
        }
        // keeps draining when the log is unwritable so the child never blocks on a full pipe
        let Some((f, _)) = log.as_mut() else { continue };
        let text = String::from_utf8_lossy(&line.bytes);
        let entry = format!("[{label}] {}\n", text.strip_suffix('\r').unwrap_or(&text));
        if f.write_all(entry.as_bytes()).await.is_err() || f.flush().await.is_err() {
            log = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lang_env(host: &PluginHost) -> Option<Option<String>> {
        host.command()
            .as_std()
            .get_envs()
            .find(|(key, _)| *key == "FOCUSDECK_LANG")
            .map(|(_, value)| value.map(|v| v.to_string_lossy().into_owned()))
    }

    #[test]
    fn tr_error_texts_unchanged() {
        let tr = i18n::localizer(Some("tr"));
        let cases = [
            (HostError::Spawn("x".into()), "plugin başlatılamadı: x"),
            (HostError::Timeout, "plugin 15 sn içinde cevap vermedi"),
            (HostError::Crashed, "plugin kapandı"),
            (
                HostError::BadLine("x".into()),
                "plugin geçersiz satır yazdı: x",
            ),
            (HostError::Plugin("x".into()), "x"),
            (
                HostError::Protocol {
                    expected: 1,
                    got: 2,
                },
                "protokol uyuşmuyor: çekirdek 1, plugin 2",
            ),
        ];
        for (error, text) in cases {
            assert_eq!(error.localized(&tr), text);
        }
    }

    #[test]
    fn language_env_is_set_or_cleared() {
        let host = PluginHost::new("x".into(), "plugin".into(), Value::Null, "log".into());
        assert_eq!(lang_env(&host), Some(None));
        let host = host.with_lang(Some("tr-TR".into()));
        assert_eq!(lang_env(&host), Some(Some("tr-TR".into())));
    }

    #[test]
    fn with_args_reach_the_command() {
        let host = PluginHost::new("x".into(), "plugin".into(), Value::Null, "log".into());
        assert_eq!(host.command().as_std().get_args().count(), 0);
        let host = host.with_args(vec!["serve-local".into()]);
        let command = host.command();
        let args: Vec<_> = command.as_std().get_args().collect();
        assert_eq!(args, ["serve-local"]);
    }

    #[tokio::test]
    async fn a_chatty_writer_rotates_the_log_while_it_runs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("focusdeck.log");
        let line = format!("{}\n", "x".repeat(99));
        let limit = usize::try_from(crate::private::LOG_LIMIT).unwrap();
        let input = line.repeat((limit + limit / 8) / line.len());
        log_stderr("p".into(), std::io::Cursor::new(input), path.clone()).await;
        let entry = "[p] ".len() + line.len();
        assert!(dir.path().join("focusdeck.log.1").exists());
        assert!(std::fs::metadata(&path).unwrap().len() <= (limit + entry) as u64);
    }

    async fn second_line_after(disturb: impl FnOnce(&std::path::Path)) -> String {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("focusdeck.log");
        let (mut tx, rx) = tokio::io::duplex(1024);
        let task = tokio::spawn(log_stderr("p".into(), rx, path.clone()));
        tx.write_all(b"first\n").await.unwrap();
        let first_logged = async {
            while !std::fs::read_to_string(&path).is_ok_and(|s| s.contains("first")) {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        };
        tokio::time::timeout(Duration::from_secs(10), first_logged)
            .await
            .unwrap();
        disturb(&path);
        tx.write_all(b"second\n").await.unwrap();
        drop(tx);
        task.await.unwrap();
        std::fs::read_to_string(&path).unwrap()
    }

    fn fill_past_the_limit(path: &std::path::Path) {
        use std::io::Write as _;
        let filler = vec![b'x'; usize::try_from(crate::private::LOG_LIMIT).unwrap() + 1];
        std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(&filler)
            .unwrap();
    }

    #[tokio::test]
    async fn a_quiet_writer_follows_a_log_another_process_rotated() {
        let log = second_line_after(|path| {
            fill_past_the_limit(path);
            std::fs::rename(path, path.with_extension("log.1")).unwrap();
        })
        .await;
        assert_eq!(log, "[p] second\n");
    }

    #[tokio::test]
    async fn a_quiet_writer_follows_a_log_that_was_deleted() {
        let log = second_line_after(|path| std::fs::remove_file(path).unwrap()).await;
        assert_eq!(log, "[p] second\n");
    }

    #[tokio::test]
    async fn a_log_that_cannot_rotate_still_takes_every_line() {
        let log = second_line_after(|path| {
            std::fs::create_dir(path.with_extension("log.1")).unwrap();
            fill_past_the_limit(path);
        })
        .await;
        assert!(log.starts_with("[p] first\n"));
        assert!(log.ends_with("[p] second\n"));
    }
}

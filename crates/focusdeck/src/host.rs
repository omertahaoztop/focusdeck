//! Runs one plugin process and speaks line-delimited JSON with it.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use focusdeck_protocol::{Describe, PROTOCOL_VERSION, Request, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);
const BAD_LINE_LIMIT: usize = 200;

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("plugin başlatılamadı: {0}")]
    Spawn(String),
    #[error("plugin 15 sn içinde cevap vermedi")]
    Timeout,
    #[error("plugin kapandı")]
    Crashed,
    #[error("plugin geçersiz satır yazdı: {0}")]
    BadLine(String),
    #[error("{0}")]
    Plugin(String),
    #[error("protokol uyuşmuyor: çekirdek {expected}, plugin {got}")]
    Protocol { expected: u32, got: u32 },
}

struct Proc {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
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
    config: Value,
    log: PathBuf,
    lang: Option<String>,
    timeout: Duration,
    proc: Option<Proc>,
    describe: Option<Describe>,
}

impl PluginHost {
    pub fn new(label: String, exe: PathBuf, config: Value, log: PathBuf) -> PluginHost {
        PluginHost {
            label,
            exe,
            config,
            log,
            lang: None,
            timeout: DEFAULT_TIMEOUT,
            proc: None,
            describe: None,
        }
    }

    pub fn with_lang(mut self, lang: Option<String>) -> Self {
        self.lang = lang;
        self
    }

    pub fn with_timeout(mut self, d: Duration) -> Self {
        self.timeout = d;
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
                    Err(HostError::BadLine(format!("geçersiz sonuç: {e}")))
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
        let describe: Describe = serde_json::from_value(value)
            .map_err(|e| HostError::BadLine(format!("geçersiz describe: {e}")))?;
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
            stdout: BufReader::new(stdout).lines(),
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
        let reply = match proc.stdout.next_line().await {
            Ok(Some(reply)) => reply,
            Ok(None) => return Err(HostError::Crashed),
            Err(e) if e.kind() == std::io::ErrorKind::InvalidData => {
                return Err(HostError::BadLine("UTF-8 olmayan çıktı".into()));
            }
            Err(_) => return Err(HostError::Crashed),
        };
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

async fn log_stderr(label: String, stderr: ChildStderr, path: PathBuf) {
    let mut lines = BufReader::new(stderr).lines();
    let mut file = open_log(&path).await;
    while let Ok(Some(line)) = lines.next_line().await {
        // keeps draining when the log is unwritable so the child never blocks on a full pipe
        let Some(f) = file.as_mut() else { continue };
        let entry = format!("[{label}] {line}\n");
        if f.write_all(entry.as_bytes()).await.is_err() || f.flush().await.is_err() {
            file = None;
        }
    }
}

async fn open_log(path: &PathBuf) -> Option<tokio::fs::File> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.ok()?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .ok()
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
    fn language_env_is_set_or_cleared() {
        let host = PluginHost::new("x".into(), "plugin".into(), Value::Null, "log".into());
        assert_eq!(lang_env(&host), Some(None));
        let host = host.with_lang(Some("tr-TR".into()));
        assert_eq!(lang_env(&host), Some(Some("tr-TR".into())));
    }
}

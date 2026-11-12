//! `focusdeck capture`: parses a note and drops it into the capture_into box, queueing it on failure.

use std::path::PathBuf;
use std::process::Stdio;

use chrono::{DateTime, FixedOffset};
use focusdeck_protocol::{DropParams, Item};
use tokio::process::Command;

use crate::capture;
use crate::config::Config;
use crate::host::PluginHost;
use crate::i18n::{self, msg, t, t_args};
use crate::notify;
use crate::paths::Paths;
use crate::profile::Profile;
use crate::queue::{Queue, Queued};
use crate::ui::run::build_host;

// Whatever is still in `items` when this goes away, by return, `?` or panic, lands back in the queue.
struct Unsent<'a> {
    queue: &'a Queue,
    log: PathBuf,
    items: Vec<Queued>,
}

impl Unsent<'_> {
    fn flush(&mut self) -> std::io::Result<()> {
        if self.items.is_empty() {
            return Ok(());
        }
        self.queue.put_back(&self.items)?;
        self.items.clear();
        Ok(())
    }
}

impl Drop for Unsent<'_> {
    fn drop(&mut self) {
        if let Err(e) = self.flush() {
            notify::log_line(&self.log, &format!("could not write the queue: {e}"));
        }
    }
}

pub async fn run(
    paths: &Paths,
    cfg: &Config,
    profile: &Profile,
    text: Option<String>,
    now: DateTime<FixedOffset>,
) -> anyhow::Result<i32> {
    let log = paths.log_file();
    let Some(target) = profile.capture_into.as_deref() else {
        eprintln!("{}", t(msg::CAPTURE_NO_CAPTURE_INTO));
        return Ok(1);
    };
    let text = match text {
        Some(text) => text,
        None => match prompt(&capture_command(cfg)).await {
            Ok(text) if text.is_empty() => return Ok(0),
            Ok(text) => text,
            Err(e) => {
                notify::send_logged(&log, &t(msg::CAPTURE_FAILED_TITLE), &e, true);
                return Ok(1);
            }
        },
    };
    let text = text.trim().to_owned();

    let parsed = match parse(profile, &text, now) {
        Ok(parsed) => parsed,
        Err(e) => {
            notify::send_logged(&log, &t(msg::CAPTURE_FAILED_TITLE), &e, true);
            return Ok(1);
        }
    };
    let shown = match parsed.due {
        Some(due) => format!("{} · {}", parsed.title, due.format("%H:%M")),
        None => parsed.title,
    };

    let queue = Queue::new(paths.state_dir.join("capture-queue.jsonl"));
    let mut pending = match queue.take_all() {
        Ok(taken) => taken,
        Err(e) => {
            notify::log_line(&log, &format!("could not read the queue: {e}"));
            Vec::new()
        }
    };
    let queued_before = pending.len();
    pending.push(Queued { text, at: now });
    let mut unsent = Unsent {
        queue: &queue,
        log: log.clone(),
        items: pending,
    };

    let outcome = deliver(paths, profile, target, &mut unsent, &log).await;
    let flushed = unsent.flush();
    let failed = outcome.is_err() || flushed.is_err();
    match (outcome, flushed) {
        (Ok(()), _) => {
            let body = if queued_before > 0 {
                format!(
                    "{shown}\n{}",
                    t_args(
                        msg::CAPTURE_QUEUE_FLUSHED,
                        &[("count", queued_before.into())]
                    )
                )
            } else {
                shown
            };
            notify::send_logged(&log, &t(msg::CAPTURE_ADDED_TITLE), &body, false);
        }
        (Err(e), Ok(())) => {
            notify::log_line(&log, &format!("{target}: {e}"));
            notify::send_logged(&log, &t(msg::CAPTURE_QUEUED_TITLE), &e, true);
        }
        (Err(e), Err(io)) => {
            notify::log_line(
                &log,
                &format!("{target}: {e}; could not write the queue: {io}"),
            );
            notify::send_logged(&log, &t(msg::CAPTURE_LOST_TITLE), &io.to_string(), true);
        }
    }
    Ok(i32::from(failed))
}

fn parse(
    profile: &Profile,
    text: &str,
    at: DateTime<FixedOffset>,
) -> Result<capture::Capture, String> {
    capture::parse(
        text,
        at,
        &profile.capture_refs,
        &profile.capture_label_aliases,
    )
}

fn capture_command(cfg: &Config) -> Vec<String> {
    cfg.capture_command
        .clone()
        .unwrap_or_else(|| default_prompt(i18n::current()))
}

fn default_prompt(l: &focusdeck_i18n::Localizer) -> Vec<String> {
    ["rofi", "-dmenu", "-p"]
        .map(String::from)
        .into_iter()
        .chain([l.t(msg::CAPTURE_PROMPT)])
        .collect()
}

async fn prompt(command: &[String]) -> Result<String, String> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| t(msg::CAPTURE_COMMAND_EMPTY))?;
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()
        .await
        .map_err(|e| {
            t_args(
                msg::CAPTURE_PROMPT_SPAWN_FAILED,
                &[
                    ("program", program.as_str().into()),
                    ("error", e.to_string().as_str().into()),
                ],
            )
        })?;
    // a cancelled prompt (rofi Esc) prints nothing, which the caller treats as no note
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned())
}

async fn deliver(
    paths: &Paths,
    profile: &Profile,
    target: &str,
    unsent: &mut Unsent<'_>,
    log: &std::path::Path,
) -> Result<(), String> {
    let spec = profile
        .box_index(target)
        .map(|i| &profile.boxes[i])
        .ok_or_else(|| t_args(msg::CAPTURE_NO_BOX, &[("target", target.into())]))?;
    let home = dirs::home_dir().unwrap_or_default();
    let mut host: PluginHost = build_host(paths, &home, spec)?;
    while let Some(entry) = unsent.items.first() {
        match parse(profile, &entry.text, entry.at) {
            Ok(parsed) => {
                let drop = DropParams {
                    item: parsed.into_item(),
                    from: None,
                };
                host.call::<_, Item>("drop", &drop)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Err(e) => notify::log_line(log, &format!("skipped a queued note: {e}")),
        }
        unsent.items.remove(0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n;

    #[test]
    fn queue_flushed_text_follows_the_language() {
        let count = |l: &focusdeck_i18n::Localizer, n: usize| {
            l.t_args(msg::CAPTURE_QUEUE_FLUSHED, &[("count", n.into())])
        };
        let (tr, en) = (i18n::localizer(Some("tr")), i18n::localizer(None));
        assert_eq!(count(&tr, 2), "kuyruktan 2 not gönderildi");
        assert_eq!(count(&en, 1), "1 note sent from the queue");
        assert_eq!(count(&en, 2), "2 notes sent from the queue");
    }

    #[test]
    fn default_prompt_is_localized() {
        assert_eq!(
            default_prompt(&i18n::localizer(Some("tr"))),
            ["rofi", "-dmenu", "-p", "Not"]
        );
        assert_eq!(
            default_prompt(&i18n::localizer(None)),
            ["rofi", "-dmenu", "-p", "Note"]
        );
    }
}

//! `focusdeck capture`: parses a note and drops it into the capture_into box, queueing it on failure.

use std::collections::VecDeque;
use std::process::Stdio;

use chrono::{DateTime, FixedOffset};
use focusdeck_protocol::{DropParams, Item};
use tokio::process::Command;
use tokio::signal::unix::{Signal, SignalKind, signal};

use crate::capture;
use crate::config::Config;
use crate::host::PluginHost;
use crate::i18n::{self, msg, t, t_args};
use crate::notify;
use crate::paths::Paths;
use crate::profile::Profile;
use crate::queue::{Queue, Queued, Taken};
use crate::ui::run::build_host;

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

    let words = capture::Words::new(i18n::current(), &cfg.capture_words);
    let parsed = match parse(profile, &text, now, &words) {
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

    let mut stop = Stop::new()?;
    let queue = Queue::new(paths.state_dir.join("capture-queue.jsonl"));
    let entry = Queued {
        text,
        at: now,
        lang: Some(i18n::current().lang().to_owned()),
    };
    // The note reaches the disk before anything else can go wrong; a kill from here on loses nothing.
    let pushed = queue.push_entry(&entry);
    if let Err(e) = &pushed {
        notify::log_line(&log, &format!("could not write the queue: {e}"));
    }
    let mut taken = match queue.take() {
        Ok(taken) => taken,
        Err(e) => {
            notify::log_line(&log, &format!("could not read the queue: {e}"));
            if pushed.is_ok() {
                let title = t(msg::CAPTURE_QUEUED_TITLE);
                notify::send_logged(&log, &title, &e.to_string(), true);
                return Ok(1);
            }
            // With no queue to fall back on, sending now is the only way the note survives.
            Taken::default()
        }
    };
    let mut items = std::mem::take(&mut taken.items);
    // Without a queue write the note only exists in memory; the last item is the new one either way.
    let own = if pushed.is_ok() {
        items.iter().rposition(|i| *i == entry)
    } else {
        items.push(entry);
        Some(items.len() - 1)
    };
    let mut progress = Progress {
        items: items.into(),
        popped: 0,
        own,
        queued_sent: 0,
    };

    let outcome = tokio::select! {
        result = deliver(paths, cfg, profile, target, &mut progress, &log) => Outcome::Done(result),
        () = stop.wait() => Outcome::Interrupted,
    };
    let unsent: Vec<Queued> = progress.items.iter().cloned().collect();
    let committed = queue.commit(taken, &unsent);
    let code = exit_code(&outcome, committed.is_err());
    match (outcome, committed) {
        (Outcome::Done(Ok(())), committed) => {
            if let Err(e) = committed {
                notify::log_line(&log, &format!("could not update the queue: {e}"));
            }
            if handed_over(pushed.is_ok(), progress.own) {
                notify::send_logged(&log, &t(msg::CAPTURE_QUEUED_TITLE), &shown, false);
            } else {
                let body = if progress.queued_sent > 0 {
                    format!(
                        "{shown}\n{}",
                        t_args(
                            msg::CAPTURE_QUEUE_FLUSHED,
                            &[("count", progress.queued_sent.into())]
                        )
                    )
                } else {
                    shown
                };
                notify::send_logged(&log, &t(msg::CAPTURE_ADDED_TITLE), &body, false);
            }
        }
        (Outcome::Done(Err(e)), Ok(())) => {
            notify::log_line(&log, &format!("{target}: {e}"));
            notify::send_logged(&log, &t(msg::CAPTURE_QUEUED_TITLE), &e, true);
        }
        (Outcome::Interrupted, Ok(())) => {
            notify::log_line(
                &log,
                "interrupted while sending, the unsent notes are queued",
            );
            notify::send_logged(
                &log,
                &t(msg::CAPTURE_QUEUED_TITLE),
                &t(msg::CAPTURE_INTERRUPTED),
                true,
            );
        }
        (Outcome::Done(Err(e)), Err(io)) => {
            report_commit_failure(&log, &format!("{target}: {e}"), &io, pushed.is_ok());
        }
        (Outcome::Interrupted, Err(io)) => {
            report_commit_failure(&log, "interrupted", &io, pushed.is_ok());
        }
    }
    Ok(code)
}

fn exit_code(outcome: &Outcome, commit_failed: bool) -> i32 {
    i32::from(!matches!(outcome, Outcome::Done(Ok(()))) || commit_failed)
}

// The taken files still hold every note when the push worked, so only a failed push makes a failed commit a loss.
fn commit_failure_title(pushed: bool) -> &'static str {
    if pushed {
        msg::CAPTURE_QUEUED_TITLE
    } else {
        msg::CAPTURE_LOST_TITLE
    }
}

fn report_commit_failure(log: &std::path::Path, why: &str, io: &std::io::Error, pushed: bool) {
    notify::log_line(log, &format!("{why}; could not write the queue: {io}"));
    let title = t(commit_failure_title(pushed));
    notify::send_logged(log, &title, &io.to_string(), true);
}

// A concurrent run took the queue file that holds this note, so that run delivers it.
fn handed_over(pushed: bool, own: Option<usize>) -> bool {
    pushed && own.is_none()
}

enum Outcome {
    Done(Result<(), String>),
    Interrupted,
}

struct Progress {
    items: VecDeque<Queued>,
    popped: usize,
    own: Option<usize>,
    queued_sent: usize,
}

struct Stop {
    term: Signal,
    int: Signal,
}

impl Stop {
    fn new() -> std::io::Result<Stop> {
        Ok(Stop {
            term: signal(SignalKind::terminate())?,
            int: signal(SignalKind::interrupt())?,
        })
    }

    async fn wait(&mut self) {
        tokio::select! {
            _ = self.term.recv() => {}
            _ = self.int.recv() => {}
        }
    }
}

fn parse(
    profile: &Profile,
    text: &str,
    at: DateTime<FixedOffset>,
    words: &capture::Words,
) -> Result<capture::Capture, String> {
    capture::parse(
        text,
        at,
        words,
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
    cfg: &Config,
    profile: &Profile,
    target: &str,
    progress: &mut Progress,
    log: &std::path::Path,
) -> Result<(), String> {
    let spec = profile
        .box_index(target)
        .map(|i| &profile.boxes[i])
        .ok_or_else(|| t_args(msg::CAPTURE_NO_BOX, &[("target", target.into())]))?;
    let home = dirs::home_dir().unwrap_or_default();
    let mut host: PluginHost = build_host(paths, &home, spec)?;
    while let Some(entry) = progress.items.front() {
        let lang = entry.lang.as_deref().or(i18n::requested());
        let words = capture::Words::new(&i18n::localizer(lang), &cfg.capture_words);
        let is_own = progress.own == Some(progress.popped);
        match parse(profile, &entry.text, entry.at, &words) {
            Ok(parsed) => {
                let drop = DropParams {
                    item: parsed.into_item(),
                    from: None,
                };
                host.call::<_, Item>("drop", &drop)
                    .await
                    .map_err(|e| e.to_string())?;
                if !is_own {
                    progress.queued_sent += 1;
                }
            }
            Err(e) => notify::log_line(log, &format!("skipped a queued note: {e}")),
        }
        progress.items.pop_front();
        progress.popped += 1;
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
    fn note_taken_by_another_run_is_handed_over() {
        assert!(handed_over(true, None));
        assert!(!handed_over(true, Some(0)));
        assert!(!handed_over(false, Some(0)));
    }

    #[test]
    fn a_failed_commit_is_lost_only_when_the_push_failed_too() {
        assert_eq!(commit_failure_title(true), msg::CAPTURE_QUEUED_TITLE);
        assert_eq!(commit_failure_title(false), msg::CAPTURE_LOST_TITLE);
    }

    #[test]
    fn a_failed_commit_fails_the_run_even_when_delivery_worked() {
        assert_eq!(exit_code(&Outcome::Done(Ok(())), false), 0);
        assert_eq!(exit_code(&Outcome::Done(Ok(())), true), 1);
        assert_eq!(exit_code(&Outcome::Done(Err("x".into())), false), 1);
        assert_eq!(exit_code(&Outcome::Interrupted, false), 1);
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

//! `focusdeck remind`: lists or announces reminders whose time has come.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use anyhow::Context;
use chrono::{DateTime, Duration, FixedOffset};
use focusdeck_protocol::{Reminder, RemindersParams, RemindersResult};
use serde::{Deserialize, Serialize};

use crate::notify;
use crate::paths::Paths;
use crate::profile::Profile;
use crate::ui::run::build_host;

const LOOKBACK_HOURS: i64 = 12;
const KEEP_DAYS: i64 = 7;

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct State(pub BTreeMap<String, DateTime<FixedOffset>>);

impl State {
    // an unreadable or corrupt file counts as empty: the cost is one repeated notification
    pub fn load(path: &Path) -> State {
        std::fs::read(path)
            .ok()
            .and_then(|raw| serde_json::from_slice(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let json = serde_json::to_vec(self).map_err(io::Error::other)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }
}

fn key(box_id: &str, reminder: &Reminder) -> String {
    format!("{box_id}:{}@{}", reminder.item.id, reminder.at.to_rfc3339())
}

pub fn due<'a>(
    box_id: &str,
    reminders: &'a [Reminder],
    now: DateTime<FixedOffset>,
    state: &State,
) -> Vec<(String, &'a Reminder)> {
    reminders
        .iter()
        .filter(|r| r.at <= now && now - r.at <= Duration::hours(LOOKBACK_HOURS))
        .map(|r| (key(box_id, r), r))
        .filter(|(k, _)| !state.0.contains_key(k))
        .collect()
}

pub fn prune(state: &mut State, now: DateTime<FixedOffset>) {
    state
        .0
        .retain(|_, at| now - *at <= Duration::days(KEEP_DAYS));
}

pub async fn run(
    paths: &Paths,
    profile: &Profile,
    send: bool,
    now: DateTime<FixedOffset>,
) -> anyhow::Result<usize> {
    let state_path = paths.state_dir.join("reminded.json");
    let log = paths.log_file();
    let mut state = State::load(&state_path);
    let home = dirs::home_dir().unwrap_or_default();
    let since = now - Duration::hours(LOOKBACK_HOURS);

    let mut found: Vec<(String, Reminder)> = Vec::new();
    for spec in &profile.boxes {
        if !paths.plugin_dir().join(&spec.plugin).is_file() {
            continue;
        }
        let mut host = match build_host(paths, &home, spec) {
            Ok(host) => host,
            Err(e) => {
                notify::log_line(&log, &format!("{}: {e}", spec.id));
                continue;
            }
        };
        let fetched = async {
            if !host.describe().await?.reminders {
                return Ok(Vec::new());
            }
            host.call::<_, RemindersResult>("reminders", &RemindersParams { since })
                .await
                .map(|r| r.reminders)
        };
        match fetched.await {
            Ok(reminders) => {
                for (k, r) in due(&spec.id, &reminders, now, &state) {
                    found.push((k, r.clone()));
                }
            }
            Err(e) => notify::log_line(&log, &format!("{}: hatırlatmalar alınamadı: {e}", spec.id)),
        }
    }
    found.sort_by_key(|(_, r)| r.at);

    for (k, r) in &found {
        let time = r.at.with_timezone(now.offset()).format("%H:%M").to_string();
        if send {
            notify::send_logged(&log, &format!("⏰ {}", r.item.title), &time, true);
            state.0.insert(k.clone(), r.at);
        } else {
            println!("{time}  {}", r.item.title);
        }
    }
    if send {
        prune(&mut state, now);
        state
            .save(&state_path)
            .with_context(|| format!("{} yazılamadı", state_path.display()))?;
    }
    Ok(found.len())
}

//! `focusdeck remind`: lists or announces reminders whose time has come.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use anyhow::Context;
use chrono::{DateTime, Duration, FixedOffset};
use focusdeck_protocol::{Reminder, RemindersParams, RemindersResult};
use serde::{Deserialize, Serialize};

use crate::cli::display_safe;
use crate::i18n::{msg, t_args};
use crate::local::LOCAL;
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
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        let json = serde_json::to_vec(self).map_err(io::Error::other)?;
        crate::private::write(Path::new(&tmp), &json)?;
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

fn lock(state_path: &Path) -> io::Result<std::fs::File> {
    let mut name = state_path.as_os_str().to_owned();
    name.push(".lock");
    let file = crate::private::append(Path::new(&name))?;
    file.lock()?;
    Ok(file)
}

fn write_failed(path: &Path) -> String {
    t_args(
        msg::REMIND_WRITE_FAILED,
        &[("path", path.display().to_string().as_str().into())],
    )
}

pub async fn run(
    paths: &Paths,
    profile: &Profile,
    send: bool,
    now: DateTime<FixedOffset>,
) -> anyhow::Result<usize> {
    let state_path = paths.state_dir.join("reminded.json");
    let log = paths.log_file();
    // Held from before the load until after the save, so two runs never announce the same reminder.
    let _lock = if send {
        Some(lock(&state_path).with_context(|| write_failed(&state_path))?)
    } else {
        None
    };
    let mut state = State::load(&state_path);
    let home = dirs::home_dir().unwrap_or_default();
    let since = now - Duration::hours(LOOKBACK_HOURS);

    let fetches = profile.boxes.iter().filter_map(|spec| {
        if spec.plugin != LOCAL && !paths.plugin_dir().join(&spec.plugin).is_file() {
            return None;
        }
        let (log, home) = (&log, &home);
        Some(async move {
            let mut host = match build_host(paths, home, spec) {
                Ok(host) => host,
                Err(e) => {
                    notify::log_line(log, &format!("{}: {e}", spec.id));
                    return (spec, Vec::new());
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
                Ok(reminders) => (spec, reminders),
                Err(e) => {
                    notify::log_line(log, &format!("{}: could not fetch reminders: {e}", spec.id));
                    (spec, Vec::new())
                }
            }
        })
    });
    let mut found: Vec<(String, Reminder)> = Vec::new();
    for (spec, reminders) in futures_util::future::join_all(fetches).await {
        for (k, r) in due(&spec.id, &reminders, now, &state) {
            found.push((k, r.clone()));
        }
    }
    found.sort_by_key(|(_, r)| r.at);

    for (k, r) in &found {
        let time = r.at.with_timezone(now.offset()).format("%H:%M").to_string();
        if send {
            if notify::send_logged(&log, &format!("⏰ {}", r.item.title), &time, true) {
                state.0.insert(k.clone(), r.at);
            }
        } else {
            println!("{time}  {}", display_safe(&r.item.title));
        }
    }
    if send {
        prune(&mut state, now);
        state
            .save(&state_path)
            .with_context(|| write_failed(&state_path))?;
    }
    Ok(found.len())
}

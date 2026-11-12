//! Terminal setup, plugin workers and the select loop that drives `App`.

use std::io::{Stdout, stdout};
use std::process::Stdio;
use std::time::{Duration, Instant};

use anyhow::Context;
use chrono::Local;
use crossterm::cursor::Show;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture, Event, EventStream};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use focusdeck_protocol::View;
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

use super::app::{App, Effect};
use super::{layout, render};
use crate::capture;
use crate::config::Config;
use crate::host::PluginHost;
use crate::i18n::{msg, t, t_args};
use crate::install;
use crate::paths::Paths;
use crate::profile::Profile;
use crate::secrets;
use crate::worker::{self, BoxCmd, BoxEvent};

const COMMAND_QUEUE: usize = 32;

type Term = Terminal<CrosstermBackend<Stdout>>;

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen, Show);
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

fn enter_terminal() -> anyhow::Result<(Term, TerminalGuard)> {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
    let guard = TerminalGuard;
    enable_raw_mode().with_context(|| t(msg::TERMINAL_RAW_MODE))?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture)
        .with_context(|| t(msg::TERMINAL_ALT_SCREEN))?;
    let term = Terminal::new(CrosstermBackend::new(stdout()))?;
    Ok((term, guard))
}

/// Boxes that fill another box always need the full list; the rest only while zoomed.
pub fn view_for(app: &App, idx: usize) -> View {
    let plans = app
        .boxes
        .get(idx)
        .is_some_and(|b| b.spec.plan_into.is_some());
    if plans || app.zoomed_box() == Some(idx) {
        View::Full
    } else {
        View::Summary
    }
}

struct Runtime<'a> {
    profile: &'a Profile,
    cmds: Vec<Option<mpsc::Sender<BoxCmd>>>,
}

impl Runtime<'_> {
    fn refresh(&self, app: &App, idx: usize) {
        if let Some(Some(tx)) = self.cmds.get(idx) {
            // a full queue means a refresh is already pending behind a slow request
            let _ = tx.try_send(BoxCmd::Refresh(view_for(app, idx)));
        }
    }

    fn send(&self, app: &mut App, idx: usize, cmd: BoxCmd) {
        let title = app
            .boxes
            .get(idx)
            .map(|b| b.spec.title.clone())
            .unwrap_or_default();
        let flash = |id: &str| t_args(id, &[("title", title.as_str().into())]);
        match self.cmds.get(idx).and_then(Option::as_ref) {
            None => app.flash = Some(flash(msg::FLASH_BOX_CLOSED)),
            Some(tx) => match tx.try_send(cmd) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => app.flash = Some(flash(msg::FLASH_BOX_BUSY)),
                Err(TrySendError::Closed(_)) => app.flash = Some(flash(msg::FLASH_BOX_STOPPED)),
            },
        }
    }

    // returns true when the app should quit
    fn exec(&self, app: &mut App, effect: Effect) -> bool {
        match effect {
            Effect::Refresh(idx, _) => self.refresh(app, idx),
            Effect::Action {
                idx,
                item,
                action,
                args,
            } => self.send(app, idx, BoxCmd::Action { item, action, args }),
            Effect::Drop { idx, item, from } => {
                self.send(app, idx, BoxCmd::Drop { item, from });
            }
            Effect::Open(url) => open_url(app, &url),
            Effect::Capture(text) => self.capture(app, &text),
            Effect::Quit => return true,
        }
        false
    }

    fn capture(&self, app: &mut App, text: &str) {
        let Some(target) = self
            .profile
            .capture_into
            .as_deref()
            .and_then(|id| self.profile.box_index(id))
        else {
            app.flash = Some(t(msg::FLASH_NO_CAPTURE_INTO));
            return;
        };
        let now = Local::now().fixed_offset();
        match capture::parse(
            text,
            now,
            &self.profile.capture_refs,
            &self.profile.capture_label_aliases,
        ) {
            Ok(parsed) => self.send(
                app,
                target,
                BoxCmd::Drop {
                    item: parsed.into_item(),
                    from: None,
                },
            ),
            Err(message) => {
                app.flash = Some(t_args(
                    msg::FLASH_CAPTURE_FAILED,
                    &[("message", message.as_str().into())],
                ));
            }
        }
    }

    // one server task can sit in several boxes of the same plugin, so every action reloads them all
    fn siblings_after_finish(&self, app: &App, ev: &BoxEvent) -> Vec<usize> {
        let BoxEvent::Finished {
            idx,
            result: Ok(()),
            ..
        } = ev
        else {
            return Vec::new();
        };
        let Some(plugin) = app.boxes.get(*idx).map(|b| b.spec.plugin.as_str()) else {
            return Vec::new();
        };
        (0..app.boxes.len())
            .filter(|i| i != idx && app.boxes[*i].spec.plugin == plugin)
            .collect()
    }
}

fn open_url(app: &mut App, url: &str) {
    let spawned = tokio::process::Command::new("xdg-open")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match spawned {
        Ok(mut child) => {
            tokio::spawn(async move {
                let _ = child.wait().await;
            });
        }
        Err(e) => {
            app.flash = Some(t_args(
                msg::FLASH_OPEN_FAILED,
                &[("error", e.to_string().as_str().into())],
            ));
        }
    }
}

pub fn build_host(
    paths: &Paths,
    home: &std::path::Path,
    spec: &crate::profile::BoxSpec,
) -> Result<PluginHost, String> {
    install::validate_name(&spec.plugin).map_err(|e| e.to_string())?;
    let exe = paths.plugin_dir().join(&spec.plugin);
    if !exe.is_file() {
        return Err(t_args(
            msg::ERROR_PLUGIN_NOT_INSTALLED,
            &[("plugin", spec.plugin.as_str().into())],
        ));
    }
    let config = secrets::plugin_config(spec, home).map_err(|e| e.to_string())?;
    Ok(
        PluginHost::new(spec.id.clone(), exe, config, paths.log_file())
            .with_lang(crate::i18n::requested().map(str::to_owned)),
    )
}

pub async fn tui(paths: &Paths, _cfg: &Config, profile: Profile) -> anyhow::Result<()> {
    let (mut terminal, _guard) = enter_terminal()?;
    let mut app = App::new(&profile);
    let home = dirs::home_dir().unwrap_or_default();
    let (event_tx, mut event_rx) = mpsc::channel::<BoxEvent>(64);
    let (tick_tx, mut tick_rx) = mpsc::channel::<usize>(64);
    let mut tasks: Vec<JoinHandle<()>> = Vec::new();
    let mut cmds = Vec::new();
    for (idx, spec) in profile.boxes.iter().enumerate() {
        match build_host(paths, &home, spec) {
            Ok(host) => {
                let (tx, rx) = mpsc::channel(COMMAND_QUEUE);
                tasks.push(worker::spawn(idx, host, rx, event_tx.clone()));
                tasks.push(spawn_ticker(idx, spec.refresh, tick_tx.clone()));
                cmds.push(Some(tx));
            }
            Err(message) => {
                app.boxes[idx].error = Some(message);
                cmds.push(None);
            }
        }
    }
    let rt = Runtime {
        profile: &profile,
        cmds,
    };
    let result = event_loop(&mut terminal, &mut app, &rt, &mut event_rx, &mut tick_rx).await;
    for task in tasks {
        task.abort();
    }
    result
}

fn spawn_ticker(idx: usize, every: Duration, tx: mpsc::Sender<usize>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(every);
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if tx.send(idx).await.is_err() {
                return;
            }
        }
    })
}

async fn event_loop(
    terminal: &mut Term,
    app: &mut App,
    rt: &Runtime<'_>,
    event_rx: &mut mpsc::Receiver<BoxEvent>,
    tick_rx: &mut mpsc::Receiver<usize>,
) -> anyhow::Result<()> {
    let mut input = EventStream::new();
    let mut clock = tokio::time::interval(Duration::from_secs(1));
    clock.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        let size = terminal.size()?;
        let area = Rect::new(0, 0, size.width, size.height);
        layout::fit_scroll(app, area);
        terminal.draw(|frame| render::draw(frame, app))?;

        let zoomed_before = app.zoomed_box();
        let mut extra_refresh = Vec::new();
        let effects = tokio::select! {
            ev = input.next() => match ev {
                Some(Ok(Event::Key(key))) => app.on_key(key),
                Some(Ok(Event::Mouse(mouse))) => {
                    let rects = layout::frame_rects(area, app);
                    let target = layout::hit(&rects, app, mouse.column, mouse.row);
                    app.on_mouse(mouse, Instant::now(), |_, _| target)
                }
                Some(Ok(_)) => Vec::new(),
                Some(Err(e)) => return Err(e).with_context(|| t(msg::TERMINAL_EVENT)),
                None => return Ok(()),
            },
            Some(ev) = event_rx.recv() => {
                extra_refresh = rt.siblings_after_finish(app, &ev);
                app.on_event(ev)
            }
            Some(idx) = tick_rx.recv() => {
                rt.refresh(app, idx);
                Vec::new()
            }
            _ = clock.tick() => Vec::new(),
        };

        for effect in effects {
            if rt.exec(app, effect) {
                return Ok(());
            }
        }
        for idx in extra_refresh {
            rt.refresh(app, idx);
        }
        if let Some(old) = zoomed_before
            && app.zoomed_box() != Some(old)
        {
            rt.refresh(app, old);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::app::Mode;
    use super::super::app::testkit::*;
    use super::*;

    #[test]
    fn plan_boxes_always_ask_for_full() {
        let app = loaded_app();
        assert_eq!(view_for(&app, REDMINE), View::Full);
        assert_eq!(view_for(&app, SOZLU), View::Summary);
    }

    #[test]
    fn other_boxes_ask_for_full_only_while_zoomed() {
        let mut app = loaded_app();
        app.mode = Mode::Zoom(ZULIP);
        assert_eq!(view_for(&app, ZULIP), View::Full);
        assert_eq!(view_for(&app, SOZLU), View::Summary);
        app.mode = Mode::Board;
        assert_eq!(view_for(&app, ZULIP), View::Summary);
    }

    #[test]
    fn finished_action_refreshes_same_plugin_siblings_only() {
        let mut profile = profile();
        profile.boxes[ZULIP].plugin = "vikunja".into();
        let app = App::new(&profile);
        let rt = Runtime {
            profile: &profile,
            cmds: Vec::new(),
        };
        let done = |what: &str, result| BoxEvent::Finished {
            idx: SOZLU,
            what: what.into(),
            result,
        };
        for what in ["drop", "done", "delete", "reorder", "move", "unstar"] {
            assert_eq!(rt.siblings_after_finish(&app, &done(what, Ok(()))), [ZULIP]);
        }
        assert!(
            rt.siblings_after_finish(&app, &done("drop", Err("x".into())))
                .is_empty()
        );
    }

    #[test]
    fn missing_plugin_binary_gives_the_install_hint() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("c"),
            data_dir: dir.path().join("d"),
            state_dir: dir.path().join("s"),
        };
        let spec = &profile().boxes[SOZLU];
        let err = build_host(&paths, dir.path(), spec).err().unwrap();
        assert_eq!(
            err,
            "vikunja is not installed, run focusdeck plugin install vikunja"
        );
    }
}

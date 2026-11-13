//! Terminal setup, plugin workers and the select loop that drives `App`.

use std::io::{Stdout, stdout};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use chrono::Local;
use crossterm::cursor::Show;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    EventStream, MouseEventKind,
};
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
use crate::api::ProfileView;
use crate::capture;
use crate::config::Config;
use crate::host::{BoxHost, PluginHost};
use crate::i18n::{self, msg, t, t_args};
use crate::install;
use crate::links::openable;
use crate::local::LOCAL;
use crate::paths::Paths;
use crate::profile::Profile;
use crate::remote::{RemoteBox, RemoteClient};
use crate::secrets;
use crate::worker::{self, BoxCmd, BoxEvent};

const COMMAND_QUEUE: usize = 32;

type Term = Terminal<CrosstermBackend<Stdout>>;

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(
        stdout(),
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen,
        Show
    );
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
    execute!(
        stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )
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
    words: capture::Words,
    cmds: Vec<Option<mpsc::Sender<BoxCmd>>>,
}

impl Runtime<'_> {
    fn refresh(&self, app: &mut App, idx: usize) {
        let Some(Some(tx)) = self.cmds.get(idx) else {
            return;
        };
        // a full queue means a refresh is already pending behind a slow request
        if tx.try_send(BoxCmd::Refresh(view_for(app, idx))).is_ok()
            && let Some(state) = app.boxes.get_mut(idx)
        {
            state.pending += 1;
        }
    }

    fn send(&self, app: &mut App, idx: usize, cmd: BoxCmd) -> bool {
        let title = app
            .boxes
            .get(idx)
            .map(|b| b.spec.title.clone())
            .unwrap_or_default();
        let flash = |id: &str| t_args(id, &[("title", title.as_str().into())]);
        match self.cmds.get(idx).and_then(Option::as_ref) {
            None => app.flash = Some(flash(msg::FLASH_BOX_CLOSED)),
            Some(tx) => match tx.try_send(cmd) {
                Ok(()) => {
                    if let Some(state) = app.boxes.get_mut(idx) {
                        state.pending += 1;
                    }
                    return true;
                }
                Err(TrySendError::Full(_)) => app.flash = Some(flash(msg::FLASH_BOX_BUSY)),
                Err(TrySendError::Closed(_)) => app.flash = Some(flash(msg::FLASH_BOX_STOPPED)),
            },
        }
        false
    }

    // returns true when the app should quit
    fn exec(&self, app: &mut App, effect: Effect) -> bool {
        match effect {
            Effect::Refresh(idx) => self.refresh(app, idx),
            Effect::Action {
                idx,
                item,
                action,
                args,
            } => {
                self.send(app, idx, BoxCmd::Action { item, action, args });
            }
            Effect::Drop { idx, item, from } => {
                let cmd = BoxCmd::Drop {
                    item,
                    from,
                    capture: false,
                };
                self.send(app, idx, cmd);
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
            app.reopen_capture(text);
            return;
        };
        let now = Local::now().fixed_offset();
        match capture::parse(
            text,
            now,
            &self.words,
            &self.profile.capture_refs,
            &self.profile.capture_label_aliases,
        ) {
            Ok(parsed) => {
                let cmd = BoxCmd::Drop {
                    item: parsed.into_item(),
                    from: None,
                    capture: true,
                };
                if self.send(app, target, cmd) {
                    app.pending_capture.push_back((target, text.to_owned()));
                } else {
                    app.reopen_capture(text);
                }
            }
            Err(message) => {
                app.flash = Some(t_args(
                    msg::FLASH_CAPTURE_FAILED,
                    &[("message", message.as_str().into())],
                ));
                app.reopen_capture(text);
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
    if !openable(url) {
        app.flash = Some(t_args(msg::FLASH_LINK_UNSUPPORTED, &[("url", url.into())]));
        return;
    }
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

#[derive(Debug)]
pub enum BuildError {
    Secret(crate::secrets::SecretError),
    Other(String),
}

impl BuildError {
    /// The text safe to hand to API clients: it never names a secret file.
    pub fn public(&self) -> String {
        match self {
            BuildError::Secret(e) => e.without_path(i18n::current()),
            BuildError::Other(message) => message.clone(),
        }
    }
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Secret(e) => e.fmt(f),
            BuildError::Other(message) => f.write_str(message),
        }
    }
}

pub fn build_host(
    paths: &Paths,
    home: &std::path::Path,
    spec: &crate::profile::BoxSpec,
) -> Result<PluginHost, String> {
    build_host_typed(paths, home, spec).map_err(|e| e.to_string())
}

pub fn build_host_typed(
    paths: &Paths,
    home: &std::path::Path,
    spec: &crate::profile::BoxSpec,
) -> Result<PluginHost, BuildError> {
    let other = |e: &dyn std::fmt::Display| BuildError::Other(e.to_string());
    install::validate_name(&spec.plugin).map_err(|e| other(&e))?;
    let (exe, args) = if spec.plugin == LOCAL {
        let exe = std::env::current_exe().map_err(|e| other(&e))?;
        (exe, vec!["serve-local".to_owned()])
    } else {
        let exe = paths.plugin_dir().join(&spec.plugin);
        if !exe.is_file() {
            return Err(BuildError::Other(t_args(
                msg::ERROR_PLUGIN_NOT_INSTALLED,
                &[("plugin", spec.plugin.as_str().into())],
            )));
        }
        install::verify_installed(&paths.plugin_dir(), &spec.plugin).map_err(|e| other(&e))?;
        (exe, Vec::new())
    };
    let mut config = secrets::plugin_config(spec, home).map_err(BuildError::Secret)?;
    if spec.plugin == LOCAL {
        config = local_config(spec, config);
    }
    Ok(
        PluginHost::new(spec.id.clone(), exe, config, paths.log_file())
            .with_args(args)
            .with_lang(crate::i18n::requested().map(str::to_owned)),
    )
}

fn local_config(
    spec: &crate::profile::BoxSpec,
    mut config: serde_json::Value,
) -> serde_json::Value {
    if let Some(map) = config.as_object_mut() {
        map.entry("list").or_insert_with(|| spec.id.clone().into());
    }
    config
}

// built once at startup
#[allow(clippy::large_enum_variant)]
pub enum Deck {
    Local(Profile),
    Remote {
        client: Arc<RemoteClient>,
        name: String,
        view: ProfileView,
    },
}

type Remote = (Arc<RemoteClient>, String, ProfileView);

/// The profile, capture words and plan-link mode of a deck; a served profile brings its own words.
fn deck_parts(deck: Deck, cfg: &Config) -> (Profile, capture::Words, bool, Option<Remote>) {
    match deck {
        Deck::Local(profile) => {
            let words = capture::Words::new(i18n::current(), &cfg.capture_words);
            (profile, words, false, None)
        }
        Deck::Remote { client, name, view } => {
            let words = capture::Words::new(i18n::current(), &view.capture_words);
            (view.to_profile(), words, true, Some((client, name, view)))
        }
    }
}

pub async fn tui(paths: &Paths, cfg: &Config, deck: Deck) -> anyhow::Result<()> {
    let (profile, words, server_links, remote) = deck_parts(deck, cfg);
    let (mut terminal, _guard) = enter_terminal()?;
    let mut app = App::new(&profile);
    if server_links {
        app = app.with_server_links();
    }
    let home = dirs::home_dir().unwrap_or_default();
    let (event_tx, mut event_rx) = mpsc::channel::<BoxEvent>(64);
    let (tick_tx, mut tick_rx) = mpsc::channel::<usize>(64);
    let mut tasks: Vec<JoinHandle<()>> = Vec::new();
    let mut cmds = Vec::new();
    for (idx, spec) in profile.boxes.iter().enumerate() {
        let host = match &remote {
            Some((client, name, view)) => Ok(BoxHost::Remote(RemoteBox::new(
                client.clone(),
                name.clone(),
                &view.boxes[idx],
            ))),
            None => build_host(paths, &home, spec).map(BoxHost::Process),
        };
        match host {
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
        words,
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

fn mouse_needs_redraw(kind: MouseEventKind) -> bool {
    !matches!(kind, MouseEventKind::Moved)
}

fn record_rows(app: &mut App, area: Rect) {
    let rects = layout::frame_rects(area, app);
    for (state, rect) in app.boxes.iter_mut().zip(rects) {
        if !rect.is_empty() {
            state.rows = usize::from(layout::list_area(rect, state.error.is_some()).height);
        }
    }
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
    let mut redraw = true;
    loop {
        let size = terminal.size()?;
        let area = Rect::new(0, 0, size.width, size.height);
        if redraw {
            layout::fit_scroll(app, area);
            record_rows(app, area);
            terminal.draw(|frame| render::draw(frame, app))?;
        }
        redraw = true;

        let zoomed_before = app.zoomed_box();
        let mut extra_refresh = Vec::new();
        let effects = tokio::select! {
            ev = input.next() => match ev {
                Some(Ok(Event::Key(key))) => app.on_key(key),
                Some(Ok(Event::Paste(text))) => app.on_paste(&text),
                Some(Ok(Event::Mouse(mouse))) => {
                    redraw = mouse_needs_redraw(mouse.kind);
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
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton};

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
            words: capture::Words::new(&i18n::localizer(None), &Default::default()),
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

    #[test]
    fn local_box_needs_no_plugin_file() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("c"),
            data_dir: dir.path().join("d"),
            state_dir: dir.path().join("s"),
        };
        let mut spec = profile().boxes[SOZLU].clone();
        spec.plugin = crate::local::LOCAL.into();
        assert!(build_host(&paths, dir.path(), &spec).is_ok());
        spec.plugin = "vikunja".into();
        let err = build_host(&paths, dir.path(), &spec).err().unwrap();
        assert_eq!(
            err,
            "vikunja is not installed, run focusdeck plugin install vikunja"
        );
    }

    #[test]
    fn local_config_defaults_list_to_box_id() {
        let spec = &profile().boxes[SOZLU];
        let id = spec.id.as_str();
        assert_eq!(
            local_config(spec, serde_json::json!({})),
            serde_json::json!({ "list": id })
        );
        assert_eq!(
            local_config(spec, serde_json::json!({ "list": "mine", "x": 1 })),
            serde_json::json!({ "list": "mine", "x": 1 })
        );
    }

    type Slots = Vec<Option<mpsc::Sender<BoxCmd>>>;

    fn four_slots() -> (Profile, Slots, [mpsc::Receiver<BoxCmd>; 2]) {
        let mut profile = profile();
        profile.boxes.push(spec("quad", "vikunja", None));
        profile.boxes[0].id = "closed".into();
        profile.boxes[0].title = "closed".into();
        profile.boxes[1].title = "full".into();
        profile.boxes[2].title = "gone".into();
        profile.boxes[3].title = "live".into();
        let (full, full_rx) = mpsc::channel(1);
        full.try_send(BoxCmd::Refresh(View::Summary)).unwrap();
        let (gone, gone_rx) = mpsc::channel(1);
        drop(gone_rx);
        let (live, live_rx) = mpsc::channel(1);
        (
            profile,
            vec![None, Some(full), Some(gone), Some(live)],
            [full_rx, live_rx],
        )
    }

    fn runtime<'a>(profile: &'a Profile, cmds: Vec<Option<mpsc::Sender<BoxCmd>>>) -> Runtime<'a> {
        Runtime {
            profile,
            words: capture::Words::new(&i18n::localizer(None), &Default::default()),
            cmds,
        }
    }

    #[test]
    fn effects_reach_the_box_channel_or_flash() {
        let (profile, cmds, mut keep) = four_slots();
        let rt = runtime(&profile, cmds);
        let mut app = App::new(&profile);
        let done = |idx| Effect::Action {
            idx,
            item: "1".into(),
            action: "done".into(),
            args: serde_json::json!({}),
        };
        for (idx, text) in [
            (0, "closed box is not open"),
            (1, "full box is busy"),
            (2, "the plugin task of the gone box stopped"),
        ] {
            assert!(!rt.exec(&mut app, done(idx)));
            assert_eq!(app.flash.as_deref(), Some(text));
            assert_eq!(app.boxes[idx].pending, 0, "box {idx}");
            assert!(!rt.send(&mut app, idx, BoxCmd::Refresh(View::Full)));
        }
        app.flash = None;
        assert!(rt.send(&mut app, 3, BoxCmd::Refresh(View::Full)));
        assert_eq!(app.flash, None);
        assert_eq!(app.boxes[3].pending, 1);
        assert!(matches!(
            keep[1].try_recv().unwrap(),
            BoxCmd::Refresh(View::Full)
        ));
    }

    #[test]
    fn refresh_counts_as_pending_only_when_queued() {
        let (profile, cmds, _keep) = four_slots();
        let rt = runtime(&profile, cmds);
        let mut app = App::new(&profile);
        for idx in 0..3 {
            rt.refresh(&mut app, idx);
            assert_eq!(app.boxes[idx].pending, 0);
        }
        rt.refresh(&mut app, 3);
        assert_eq!(app.boxes[3].pending, 1);
    }

    fn capture_app(profile: &Profile) -> App {
        let mut app = App::new(profile);
        app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        app.on_paste("süt al");
        app
    }

    fn submit(rt: &Runtime<'_>, app: &mut App) {
        for effect in app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)) {
            rt.exec(app, effect);
        }
    }

    #[test]
    fn busy_box_keeps_the_capture_text() {
        let mut profile = profile();
        profile.capture_into = Some("full".into());
        profile.boxes[1].id = "full".into();
        let (_, cmds, _keep) = four_slots();
        let rt = runtime(&profile, cmds);
        let mut app = capture_app(&profile);
        submit(&rt, &mut app);
        assert_eq!(
            app.mode,
            Mode::Capture {
                input: "süt al".into()
            }
        );
        assert!(app.pending_capture.is_empty());
        assert_eq!(app.flash.as_deref(), Some("redmine box is busy"));
    }

    #[test]
    fn queued_capture_waits_for_the_result() {
        let profile = profile();
        let (tx, mut rx) = mpsc::channel(1);
        let rt = runtime(&profile, vec![Some(tx), None, None]);
        let mut app = capture_app(&profile);
        submit(&rt, &mut app);
        assert_eq!(app.mode, Mode::Board);
        assert_eq!(app.pending_capture, [(0, "süt al".into())]);
        assert_eq!(app.boxes[0].pending, 1);
        match rx.try_recv().unwrap() {
            BoxCmd::Drop {
                item,
                from,
                capture,
            } => {
                assert_eq!((item.title.as_str(), from, capture), ("süt al", None, true));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn capture_without_a_target_or_with_a_parse_error_keeps_the_text() {
        let mut profile = profile();
        profile.capture_into = None;
        let rt = runtime(&profile, vec![None, None, None]);
        let mut app = capture_app(&profile);
        submit(&rt, &mut app);
        assert_eq!(
            app.mode,
            Mode::Capture {
                input: "süt al".into()
            }
        );
        assert_eq!(
            app.flash.as_deref(),
            Some("the profile has no capture_into")
        );

        let profile = self::profile();
        let rt = runtime(&profile, vec![None, None, None]);
        let mut app = App::new(&profile);
        app.on_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        app.on_paste("#urgent");
        submit(&rt, &mut app);
        assert_eq!(
            app.mode,
            Mode::Capture {
                input: "#urgent".into()
            }
        );
        assert!(
            app.flash
                .as_deref()
                .unwrap()
                .starts_with("could not add the note")
        );
    }

    #[test]
    fn moved_events_never_redraw() {
        assert!(!mouse_needs_redraw(MouseEventKind::Moved));
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
            MouseEventKind::Drag(MouseButton::Left),
            MouseEventKind::ScrollDown,
        ] {
            assert!(mouse_needs_redraw(kind), "{kind:?}");
        }
    }

    #[test]
    fn refusing_an_unopenable_link_flashes_it() {
        let mut app = loaded_app();
        open_url(&mut app, "file:///etc/passwd");
        assert_eq!(
            app.flash.as_deref(),
            Some("cannot open this kind of link: file:///etc/passwd")
        );
    }

    #[test]
    fn deck_parts_of_a_served_profile_use_server_words_and_links() {
        use crate::api::ProfileView;
        use crate::config::{CaptureWords, ServerConfig};
        use crate::remote::RemoteClient;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let token = dir.path().join("token");
        std::fs::write(&token, "0123456789abcdef0123456789abcdef0123456789").unwrap();
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
        let server = ServerConfig {
            url: "http://127.0.0.1:1".into(),
            token_file: token.display().to_string(),
        };
        let client = Arc::new(RemoteClient::connect(&server, dir.path()).unwrap());
        let view = ProfileView::from_profile(
            &profile(),
            &CaptureWords {
                tomorrow: vec!["zzmorrow".into()],
                ..CaptureWords::default()
            },
            Vec::new(),
        );
        let mut cfg = Config::default();
        cfg.capture_words.tomorrow = vec!["cfgmorrow".into()];
        let now = Local::now().fixed_offset();
        let known = |words: &capture::Words, text: &str| {
            let none = Default::default();
            capture::parse(text, now, words, &none, &none)
                .unwrap()
                .into_item()
                .title
                == "al"
        };

        let remote = Deck::Remote {
            client,
            name: "iş".into(),
            view,
        };
        let (profile_out, words, links, served) = deck_parts(remote, &cfg);
        assert!(links && served.is_some());
        assert_eq!(profile_out.name, "iş");
        assert!(known(&words, "al zzmorrow 10:00"));
        assert!(!known(&words, "al cfgmorrow 10:00"));

        let (_, words, links, served) = deck_parts(Deck::Local(profile()), &cfg);
        assert!(!links && served.is_none());
        assert!(known(&words, "al cfgmorrow 10:00"));
        assert!(!known(&words, "al zzmorrow 10:00"));
    }
}

//! Server-side boxes: one plugin host per profile box, cached lists, plan links and error mapping.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex as StdMutex;
use std::time::Instant;

use anyhow::Context;
use chrono::{DateTime, FixedOffset};
use focusdeck_protocol::{
    ActionParams, Describe, DropParams, Item, ListParams, ListResult, RemindersParams,
    RemindersResult, View,
};
use serde_json::json;
use tokio::sync::{Mutex, MutexGuard};

use crate::api::{
    ActionRequest, ActionResponse, BoxReminder, CaptureRequest, CaptureResponse, DropRequest,
    DropResponse, ErrorCode, ItemsResponse, ProfileView, ProfilesResponse, RemindersResponse,
};
use crate::capture;
use crate::config::{CaptureWords, Config};
use crate::host::PluginHost;
use crate::i18n::{self, msg, t_args};
use crate::links::closures_after_list;
use crate::paths::Paths;
use crate::profile::{self, BoxSpec, Profile};
use crate::ui::run::build_host;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardError {
    NotFound,
    Invalid(String),
    Unsupported(String),
    Plugin(String),
}

impl BoardError {
    pub fn code(&self) -> ErrorCode {
        match self {
            BoardError::NotFound => ErrorCode::NotFound,
            BoardError::Invalid(_) => ErrorCode::Invalid,
            BoardError::Unsupported(_) => ErrorCode::Unsupported,
            BoardError::Plugin(_) => ErrorCode::Plugin,
        }
    }

    pub fn message(&self) -> String {
        match self {
            BoardError::NotFound => "not found".to_owned(),
            BoardError::Invalid(m) | BoardError::Unsupported(m) | BoardError::Plugin(m) => {
                m.clone()
            }
        }
    }
}

impl std::fmt::Display for BoardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for BoardError {}

#[derive(Default)]
struct Cache {
    items: Option<Vec<Item>>,
    listed_at: Option<DateTime<FixedOffset>>,
    fetched: Option<Instant>,
    empty_lists: u8,
    // bumped on every invalidation so a list that started before it never refills the cache
    generation: u64,
}

struct BoxState {
    spec: BoxSpec,
    host: Result<Mutex<PluginHost>, String>,
    describe: StdMutex<Option<Result<Describe, String>>>,
    cache: StdMutex<Cache>,
}

impl BoxState {
    fn new(spec: BoxSpec, host: Result<PluginHost, String>) -> BoxState {
        BoxState {
            spec,
            host: host.map(Mutex::new),
            describe: StdMutex::new(None),
            cache: StdMutex::new(Cache::default()),
        }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn remember(&self, describe: Result<Describe, String>) {
        *self.describe.lock().unwrap_or_else(|e| e.into_inner()) = Some(describe);
    }

    fn known_describe(&self) -> Option<Describe> {
        match &*self.describe.lock().unwrap_or_else(|e| e.into_inner()) {
            Some(Ok(d)) => Some(d.clone()),
            _ => None,
        }
    }

    /// Takes the box lock and makes sure the plugin answers describe, restarting it if it is down.
    async fn ready(&self) -> Result<(MutexGuard<'_, PluginHost>, Describe), BoardError> {
        let host = self
            .host
            .as_ref()
            .map_err(|message| BoardError::Plugin(message.clone()))?;
        let mut guard = host.lock().await;
        match guard.describe().await {
            Ok(describe) => {
                self.remember(Ok(describe.clone()));
                Ok((guard, describe))
            }
            Err(e) => {
                let message = e.to_string();
                self.remember(Err(message.clone()));
                Err(BoardError::Plugin(message))
            }
        }
    }

    fn fresh(&self) -> Option<ItemsResponse> {
        let cache = self.cache();
        let fetched = cache.fetched?;
        if fetched.elapsed() >= self.spec.refresh {
            return None;
        }
        Some(ItemsResponse {
            items: cache.items.clone()?,
            listed_at: cache.listed_at?,
        })
    }

    fn drop_cache(&self) {
        let mut cache = self.cache();
        cache.items = None;
        cache.fetched = None;
        cache.generation += 1;
    }
}

struct ProfileState {
    profile: Profile,
    boxes: Vec<BoxState>,
}

impl ProfileState {
    fn find(&self, b: &str) -> Result<(usize, &BoxState), BoardError> {
        let idx = self.profile.box_index(b).ok_or(BoardError::NotFound)?;
        Ok((idx, &self.boxes[idx]))
    }
}

pub struct Board {
    profiles: BTreeMap<String, ProfileState>,
    words: CaptureWords,
    language: Option<String>,
    default_profile: Option<String>,
}

pub(crate) type BoxHosts = Vec<Result<PluginHost, String>>;

impl Board {
    pub fn load(paths: &Paths, cfg: &Config, home: &Path) -> anyhow::Result<Board> {
        let dir = paths.config_dir.join("profiles");
        let mut stems = Vec::new();
        let entries = std::fs::read_dir(&dir).with_context(|| {
            t_args(
                msg::SERVER_NO_PROFILES,
                &[("dir", dir.display().to_string().as_str().into())],
            )
        })?;
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "toml")
                && path.is_file()
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && !stem.starts_with('.')
            {
                stems.push(stem.to_owned());
            }
        }
        if stems.is_empty() {
            anyhow::bail!(t_args(
                msg::SERVER_NO_PROFILES,
                &[("dir", dir.display().to_string().as_str().into())]
            ));
        }
        let mut loaded = Vec::new();
        for stem in stems {
            let profile = profile::load(&paths.profile_path(&stem))?;
            let hosts = profile
                .boxes
                .iter()
                .map(|spec| build_host(paths, home, spec))
                .collect();
            loaded.push((stem, profile, hosts));
        }
        Ok(Board::from_parts(
            loaded,
            cfg.capture_words.clone(),
            cfg.language.clone(),
            cfg.default_profile.clone(),
        ))
    }

    pub(crate) fn from_parts(
        loaded: Vec<(String, Profile, BoxHosts)>,
        words: CaptureWords,
        language: Option<String>,
        default_profile: Option<String>,
    ) -> Board {
        let profiles = loaded
            .into_iter()
            .map(|(name, profile, hosts)| {
                let boxes = profile
                    .boxes
                    .iter()
                    .cloned()
                    .zip(hosts)
                    .map(|(spec, host)| BoxState::new(spec, host))
                    .collect();
                (name, ProfileState { profile, boxes })
            })
            .collect();
        Board {
            profiles,
            words,
            language,
            default_profile,
        }
    }

    fn profile(&self, p: &str) -> Result<&ProfileState, BoardError> {
        self.profiles.get(p).ok_or(BoardError::NotFound)
    }

    pub async fn profiles(&self) -> ProfilesResponse {
        let default = self
            .default_profile
            .as_ref()
            .filter(|name| self.profiles.contains_key(*name))
            .cloned()
            .or_else(|| {
                (self.profiles.len() == 1)
                    .then(|| self.profiles.keys().next().cloned())
                    .flatten()
            });
        ProfilesResponse {
            profiles: self.profiles.keys().cloned().collect(),
            default,
        }
    }

    pub async fn profile_view(&self, p: &str) -> Result<ProfileView, BoardError> {
        let state = self.profile(p)?;
        let describes = futures_util::future::join_all(state.boxes.iter().map(|bx| async move {
            match bx.known_describe() {
                Some(d) => Ok(d),
                None => bx.ready().await.map(|(_, d)| d).map_err(|e| e.message()),
            }
        }))
        .await;
        Ok(ProfileView::from_profile(
            &state.profile,
            &self.words,
            describes,
        ))
    }

    pub async fn items(
        &self,
        p: &str,
        b: &str,
        now: DateTime<FixedOffset>,
    ) -> Result<ItemsResponse, BoardError> {
        let state = self.profile(p)?;
        let (idx, bx) = state.find(b)?;
        let (response, real) = self.list_box(bx, now).await?;
        if real {
            self.close_linked(state, idx, &response.items, now).await;
        }
        Ok(response)
    }

    /// The flag tells a real plugin list from a cached one.
    async fn list_box(
        &self,
        bx: &BoxState,
        now: DateTime<FixedOffset>,
    ) -> Result<(ItemsResponse, bool), BoardError> {
        if let Some(cached) = bx.fresh() {
            return Ok((cached, false));
        }
        let (mut host, _) = bx.ready().await?;
        // another request may have refilled the cache while this one waited for the lock
        if let Some(cached) = bx.fresh() {
            return Ok((cached, false));
        }
        let generation = bx.cache().generation;
        let listed = host
            .call::<_, ListResult>("list", &ListParams { view: View::Full })
            .await;
        drop(host);
        let mut cache = bx.cache();
        match listed {
            Err(e) => {
                cache.empty_lists = 0;
                Err(BoardError::Plugin(e.to_string()))
            }
            Ok(list) => {
                if cache.generation == generation {
                    cache.items = Some(list.items.clone());
                    cache.listed_at = Some(now);
                    cache.fetched = Some(Instant::now());
                }
                Ok((
                    ItemsResponse {
                        items: list.items,
                        listed_at: now,
                    },
                    true,
                ))
            }
        }
    }

    // a failure here leaves the links open; the next real list retries them
    async fn close_linked(
        &self,
        state: &ProfileState,
        idx: usize,
        items: &[Item],
        now: DateTime<FixedOffset>,
    ) {
        let source = &state.boxes[idx];
        let Some(target) = source
            .spec
            .plan_into
            .as_deref()
            .and_then(|id| state.profile.box_index(id))
        else {
            return;
        };
        let target = &state.boxes[target];
        let Ok((target_items, _)) = self.list_box(target, now).await else {
            return;
        };
        let closing = {
            let mut cache = source.cache();
            closures_after_list(
                &mut cache.empty_lists,
                &source.spec.plugin,
                items,
                &target_items.items,
            )
        };
        for id in closing {
            let request = ActionRequest {
                item: id,
                action: "done".to_owned(),
                args: json!({}),
            };
            let _ = self.run_action(target, request).await;
        }
    }

    pub async fn action(
        &self,
        p: &str,
        b: &str,
        req: ActionRequest,
    ) -> Result<ActionResponse, BoardError> {
        let (_, bx) = self.profile(p)?.find(b)?;
        self.run_action(bx, req).await
    }

    async fn run_action(
        &self,
        bx: &BoxState,
        req: ActionRequest,
    ) -> Result<ActionResponse, BoardError> {
        let (mut host, describe) = bx.ready().await?;
        if !describe.actions.contains(&req.action) {
            return Err(BoardError::Unsupported(format!(
                "box {} does not support the action {}",
                bx.spec.id, req.action
            )));
        }
        let params = ActionParams {
            item: req.item,
            action: req.action,
            args: req.args,
        };
        let result = host.call::<_, Option<Item>>("action", &params).await;
        drop(host);
        let item = result.map_err(|e| BoardError::Plugin(e.to_string()))?;
        self.invalidate(&[bx.spec.plugin.as_str()]);
        Ok(ActionResponse { item })
    }

    pub async fn drop(
        &self,
        p: &str,
        b: &str,
        req: DropRequest,
    ) -> Result<DropResponse, BoardError> {
        let (_, bx) = self.profile(p)?.find(b)?;
        self.run_drop(bx, req).await
    }

    async fn run_drop(&self, bx: &BoxState, req: DropRequest) -> Result<DropResponse, BoardError> {
        let (mut host, describe) = bx.ready().await?;
        if !describe.accepts_drop {
            return Err(BoardError::Unsupported(format!(
                "box {} does not accept drops",
                bx.spec.id
            )));
        }
        let from_plugin = req.from.as_ref().map(|s| s.plugin.clone());
        let result = host.call::<_, Item>("drop", &req).await;
        drop(host);
        let item = result.map_err(|e| BoardError::Plugin(e.to_string()))?;
        let mut plugins = vec![bx.spec.plugin.as_str()];
        plugins.extend(from_plugin.as_deref());
        self.invalidate(&plugins);
        Ok(DropResponse { item })
    }

    pub async fn capture(
        &self,
        p: &str,
        req: CaptureRequest,
    ) -> Result<CaptureResponse, BoardError> {
        let state = self.profile(p)?;
        let (target_id, bx) = state
            .profile
            .capture_into
            .as_deref()
            .and_then(|id| state.find(id).ok().map(|(_, bx)| (id, bx)))
            .ok_or_else(|| BoardError::Invalid("the profile has no capture_into box".to_owned()))?;
        let localizer = i18n::localizer(req.lang.as_deref().or(self.language.as_deref()));
        let words = capture::Words::new(&localizer, &self.words);
        let parsed = capture::parse(
            &req.text,
            req.at,
            &words,
            &state.profile.capture_refs,
            &state.profile.capture_label_aliases,
        )
        .map_err(BoardError::Invalid)?;
        let drop = DropParams {
            item: parsed.into_item(),
            from: None,
        };
        let response = self.run_drop(bx, drop).await?;
        Ok(CaptureResponse {
            box_id: target_id.to_owned(),
            item: response.item,
        })
    }

    /// A box that fails is skipped; only when every box that was asked fails does the call answer `Plugin`.
    pub async fn reminders(
        &self,
        p: &str,
        since: DateTime<FixedOffset>,
    ) -> Result<RemindersResponse, BoardError> {
        let state = self.profile(p)?;
        let asked = futures_util::future::join_all(state.boxes.iter().map(|bx| async move {
            let (mut host, describe) = match bx.ready().await {
                Ok(ready) => ready,
                Err(e) => return Some((bx, Err(e))),
            };
            if !describe.reminders {
                return None;
            }
            let result = host
                .call::<_, RemindersResult>("reminders", &RemindersParams { since })
                .await
                .map(|r| r.reminders)
                .map_err(|e| BoardError::Plugin(e.to_string()));
            Some((bx, result))
        }))
        .await;
        let mut reminders = Vec::new();
        let mut failure = None;
        let mut succeeded = false;
        for (bx, result) in asked.into_iter().flatten() {
            match result {
                Ok(found) => {
                    succeeded = true;
                    reminders.extend(found.into_iter().map(|r| BoxReminder {
                        box_id: bx.spec.id.clone(),
                        at: r.at,
                        item: r.item,
                    }));
                }
                Err(e) => failure = failure.or(Some(e)),
            }
        }
        match failure {
            Some(e) if !succeeded => Err(e),
            _ => {
                reminders.sort_by_key(|r| r.at);
                Ok(RemindersResponse { reminders })
            }
        }
    }

    // one task can sit in several boxes of the same plugin, in any profile
    fn invalidate(&self, plugins: &[&str]) {
        for state in self.profiles.values() {
            for bx in &state.boxes {
                if plugins.contains(&bx.spec.plugin.as_str()) {
                    bx.drop_cache();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use focusdeck_protocol::Source;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::{Arc, LazyLock};
    use std::time::Duration;

    fn fake_plugin() -> PathBuf {
        static BUILT: LazyLock<PathBuf> = LazyLock::new(|| {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
            let status = Command::new(cargo)
                .args(["build", "-p", "fake-plugin"])
                .current_dir(&workspace)
                .status()
                .expect("cargo build failed to start");
            assert!(status.success(), "fake-plugin did not build");
            let target = match std::env::var_os("CARGO_TARGET_DIR") {
                Some(dir) => workspace.join(dir),
                None => workspace.join("target"),
            };
            target.join("debug/fake-plugin")
        });
        BUILT.clone()
    }

    struct Env {
        dir: tempfile::TempDir,
    }

    impl Env {
        fn new(mode: &str) -> Env {
            let env = Env {
                dir: tempfile::tempdir().unwrap(),
            };
            env.set_mode(mode);
            env
        }

        fn set_mode(&self, mode: &str) {
            std::fs::write(self.dir.path().join("mode"), mode).unwrap();
        }

        fn set_items(&self, items: &[Item]) {
            std::fs::write(
                self.dir.path().join("items.json"),
                serde_json::to_string(items).unwrap(),
            )
            .unwrap();
        }

        fn log(&self) -> String {
            std::fs::read_to_string(self.dir.path().join("fake.log")).unwrap_or_default()
        }

        fn count(&self, prefix: &str) -> usize {
            self.log().lines().filter(|l| l.starts_with(prefix)).count()
        }

        // the wrapper script gives every plugin its own environment without touching the process env
        fn host(&self, id: &str) -> PluginHost {
            let script = self.dir.path().join(format!("run-{id}.sh"));
            let items = self.dir.path().join("items.json");
            let items_env = if items.exists() {
                format!("FAKE_ITEMS_FILE='{}' ", items.display())
            } else {
                String::new()
            };
            let body = format!(
                "#!/bin/sh\n{items_env}FAKE_MODE_FILE='{}' FAKE_LOG='{}' exec '{}'\n",
                self.dir.path().join("mode").display(),
                self.dir.path().join("fake.log").display(),
                fake_plugin().display()
            );
            let status = Command::new("sh")
                .args(["-c", "printf %s \"$1\" > \"$2\" && chmod 755 \"$2\"", "sh"])
                .arg(body)
                .arg(&script)
                .status()
                .unwrap();
            assert!(status.success());
            PluginHost::new(
                id.into(),
                script,
                json!({}),
                self.dir.path().join("host.log"),
            )
            .with_timeout(Duration::from_secs(10))
        }
    }

    fn spec(id: &str, plugin: &str, refresh: Duration, plan_into: Option<&str>) -> BoxSpec {
        BoxSpec {
            id: id.into(),
            title: id.into(),
            plugin: plugin.into(),
            row: 0,
            col: 0,
            width: 1,
            height: 1,
            refresh,
            plan_into: plan_into.map(str::to_owned),
            config: toml::Table::new(),
        }
    }

    fn profile(name: &str, boxes: Vec<BoxSpec>, capture_into: Option<&str>) -> Profile {
        Profile {
            name: name.into(),
            capture_into: capture_into.map(str::to_owned),
            capture_refs: BTreeMap::new(),
            capture_label_aliases: BTreeMap::new(),
            boxes,
        }
    }

    fn board(entries: Vec<(&str, Profile, BoxHosts)>) -> Board {
        Board::from_parts(
            entries
                .into_iter()
                .map(|(name, p, hosts)| (name.to_owned(), p, hosts))
                .collect(),
            CaptureWords::default(),
            Some("en".into()),
            None,
        )
    }

    fn now() -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339("2026-11-13T10:00:00+03:00").unwrap()
    }

    fn item(id: &str, source: Option<(&str, &str)>) -> Item {
        Item {
            id: id.into(),
            title: format!("item {id}"),
            subtitle: None,
            url: None,
            due: None,
            overdue: false,
            source: source.map(|(plugin, sid)| Source {
                plugin: plugin.into(),
                id: sid.into(),
            }),
            description: None,
            labels: vec![],
            from: None,
            links: vec![],
        }
    }

    fn done(id: &str) -> ActionRequest {
        ActionRequest {
            item: id.into(),
            action: "done".into(),
            args: json!({}),
        }
    }

    #[test]
    fn board_is_shareable_between_handlers() {
        fn shareable<T: Send + Sync>() {}
        shareable::<Board>();
    }

    const LONG: Duration = Duration::from_secs(600);

    #[tokio::test]
    async fn items_within_refresh_come_from_cache() {
        let env = Env::new("ok");
        let p = profile("p", vec![spec("a", "fake", LONG, None)], None);
        let board = board(vec![("p", p, vec![Ok(env.host("a"))])]);
        let first = board.items("p", "a", now()).await.unwrap();
        let second = board.items("p", "a", now()).await.unwrap();
        assert_eq!(first.items.len(), 2);
        assert_eq!(first, second);
        assert_eq!(env.count("list"), 1);
    }

    #[tokio::test]
    async fn action_drops_cache_of_boxes_with_same_plugin() {
        let env = Env::new("ok");
        let other = Env::new("ok");
        let p1 = profile(
            "p1",
            vec![
                spec("a", "fake", LONG, None),
                spec("c", "different", LONG, None),
            ],
            None,
        );
        let p2 = profile("p2", vec![spec("b", "fake", LONG, None)], None);
        let board = board(vec![
            ("p1", p1, vec![Ok(env.host("a")), Ok(other.host("c"))]),
            ("p2", p2, vec![Ok(env.host("b"))]),
        ]);
        for (p, b) in [("p1", "a"), ("p1", "c"), ("p2", "b")] {
            board.items(p, b, now()).await.unwrap();
        }
        board.action("p1", "a", done("1")).await.unwrap();
        for (p, b) in [("p1", "a"), ("p1", "c"), ("p2", "b")] {
            board.items(p, b, now()).await.unwrap();
        }
        assert_eq!(env.count("list"), 4);
        assert_eq!(other.count("list"), 1);
    }

    #[tokio::test]
    async fn unsupported_action_is_refused_without_calling_plugin() {
        let env = Env::new("ok");
        let p = profile("p", vec![spec("a", "fake", LONG, None)], None);
        let board = board(vec![("p", p, vec![Ok(env.host("a"))])]);
        let request = ActionRequest {
            action: "explode".into(),
            ..done("1")
        };
        let err = board.action("p", "a", request).await.unwrap_err();
        assert!(matches!(err, BoardError::Unsupported(_)), "{err:?}");
        assert_eq!(err.code(), ErrorCode::Unsupported);
        assert_eq!(env.count("action"), 0);
    }

    #[tokio::test]
    async fn unknown_profile_or_box_is_not_found() {
        let env = Env::new("ok");
        let p = profile("p", vec![spec("a", "fake", LONG, None)], None);
        let board = board(vec![("p", p, vec![Ok(env.host("a"))])]);
        assert_eq!(
            board.items("zzz", "a", now()).await.unwrap_err(),
            BoardError::NotFound
        );
        assert_eq!(
            board.items("p", "zzz", now()).await.unwrap_err(),
            BoardError::NotFound
        );
        assert_eq!(
            board.profile_view("zzz").await.unwrap_err().code(),
            ErrorCode::NotFound
        );
        assert_eq!(
            board.action("p", "zzz", done("1")).await.unwrap_err(),
            BoardError::NotFound
        );
    }

    #[tokio::test]
    async fn crashed_plugin_answers_plugin_then_recovers() {
        let env = Env::new("crash");
        let p = profile("p", vec![spec("a", "fake", Duration::ZERO, None)], None);
        let board = board(vec![("p", p, vec![Ok(env.host("a"))])]);
        let err = board.items("p", "a", now()).await.unwrap_err();
        assert_eq!(err.code(), ErrorCode::Plugin);
        env.set_mode("ok");
        let listed = board.items("p", "a", now()).await.unwrap();
        assert_eq!(listed.items.len(), 2);
    }

    #[tokio::test]
    async fn broken_box_reports_error_in_profile_and_retries_describe() {
        let env = Env::new("ok");
        let p = profile(
            "p",
            vec![spec("a", "fake", LONG, None), spec("b", "gone", LONG, None)],
            None,
        );
        let board = board(vec![(
            "p",
            p,
            vec![
                Ok(env.host("a")),
                Err("plugin gone is not installed".into()),
            ],
        )]);
        let view = board.profile_view("p").await.unwrap();
        assert!(view.boxes[0].describe.is_some() && view.boxes[0].error.is_none());
        assert!(view.boxes[1].describe.is_none());
        assert_eq!(
            view.boxes[1].error.as_deref(),
            Some("plugin gone is not installed")
        );
        let err = board.items("p", "b", now()).await.unwrap_err();
        assert_eq!(
            err,
            BoardError::Plugin("plugin gone is not installed".into())
        );
    }

    #[tokio::test]
    async fn plan_link_closes_task_on_server_after_two_empty_lists() {
        let source = Env::new("empty");
        let target = Env::new("ok");
        target.set_items(&[item("t1", Some(("src", "9"))), item("t2", None)]);
        let p = profile(
            "p",
            vec![
                spec("from", "src", Duration::ZERO, Some("to")),
                spec("to", "tgt", LONG, None),
            ],
            None,
        );
        let board = board(vec![(
            "p",
            p,
            vec![Ok(source.host("from")), Ok(target.host("to"))],
        )]);
        board.items("p", "from", now()).await.unwrap();
        assert_eq!(target.count("action"), 0);
        board.items("p", "from", now()).await.unwrap();
        assert_eq!(
            target
                .log()
                .lines()
                .filter(|l| *l == "action t1 done")
                .count(),
            1
        );
        assert_eq!(target.count("action"), 1);
    }

    #[tokio::test]
    async fn cached_list_does_not_advance_plan_link_counter() {
        let source = Env::new("empty");
        let target = Env::new("ok");
        target.set_items(&[item("t1", Some(("src", "9")))]);
        let p = profile(
            "p",
            vec![
                spec("from", "src", LONG, Some("to")),
                spec("to", "tgt", LONG, None),
            ],
            None,
        );
        let board = board(vec![(
            "p",
            p,
            vec![Ok(source.host("from")), Ok(target.host("to"))],
        )]);
        for _ in 0..3 {
            board.items("p", "from", now()).await.unwrap();
        }
        assert_eq!(source.count("list"), 1);
        assert_eq!(target.count("action"), 0);
    }

    #[tokio::test]
    async fn concurrent_actions_on_one_box_are_serialized() {
        let env = Env::new("slow-action");
        let p = profile("p", vec![spec("a", "fake", LONG, None)], None);
        let board = Arc::new(board(vec![("p", p, vec![Ok(env.host("a"))])]));
        let started = Instant::now();
        let tasks: Vec<_> = ["1", "2", "3"]
            .into_iter()
            .map(|id| {
                let board = Arc::clone(&board);
                tokio::spawn(async move { board.action("p", "a", done(id)).await })
            })
            .collect();
        for task in tasks {
            task.await.unwrap().unwrap();
        }
        assert!(started.elapsed() >= Duration::from_millis(900));
        let log = env.log();
        let marks: Vec<&str> = log.lines().filter(|l| l.starts_with("action-")).collect();
        assert_eq!(marks, ["action-start", "action-end"].repeat(3), "{log}");
    }

    #[tokio::test]
    async fn capture_without_capture_into_is_invalid() {
        let env = Env::new("ok");
        let p = profile("p", vec![spec("a", "fake", LONG, None)], None);
        let board = board(vec![("p", p, vec![Ok(env.host("a"))])]);
        let request = CaptureRequest {
            text: "buy milk".into(),
            at: now(),
            lang: None,
        };
        let err = board.capture("p", request).await.unwrap_err();
        assert_eq!(err.code(), ErrorCode::Invalid);
        assert_eq!(env.count("drop"), 0);
    }

    #[tokio::test]
    async fn capture_drops_parsed_note_into_capture_box() {
        let env = Env::new("ok");
        let p = profile("p", vec![spec("a", "fake", LONG, None)], Some("a"));
        let board = board(vec![("p", p, vec![Ok(env.host("a"))])]);
        let request = CaptureRequest {
            text: "buy milk".into(),
            at: now(),
            lang: Some("en".into()),
        };
        let response = board.capture("p", request).await.unwrap();
        assert_eq!(response.box_id, "a");
        assert_eq!(response.item.title, "buy milk");
        assert_eq!(env.count("drop"), 1);
    }

    #[tokio::test]
    async fn drop_invalidates_source_plugin_boxes_too() {
        let target = Env::new("ok");
        let origin = Env::new("ok");
        let p = profile(
            "p",
            vec![
                spec("to", "tgt", LONG, None),
                spec("from", "src", LONG, None),
            ],
            None,
        );
        let board = board(vec![(
            "p",
            p,
            vec![Ok(target.host("to")), Ok(origin.host("from"))],
        )]);
        board.items("p", "from", now()).await.unwrap();
        let request = DropRequest {
            item: item("1", None),
            from: Some(Source {
                plugin: "src".into(),
                id: "1".into(),
            }),
        };
        board.drop("p", "to", request).await.unwrap();
        board.items("p", "from", now()).await.unwrap();
        assert_eq!(origin.count("list"), 2);
    }

    #[tokio::test]
    async fn profiles_default_follows_config_or_single_profile() {
        let env = Env::new("ok");
        let one = || profile("p", vec![spec("a", "fake", LONG, None)], None);
        let single = board(vec![("p", one(), vec![Ok(env.host("a"))])]);
        assert_eq!(single.profiles().await.default.as_deref(), Some("p"));
        let two = Board::from_parts(
            vec![
                ("p".into(), one(), vec![Ok(env.host("a"))]),
                ("q".into(), one(), vec![Ok(env.host("a"))]),
            ],
            CaptureWords::default(),
            None,
            Some("q".into()),
        );
        let response = two.profiles().await;
        assert_eq!(response.profiles, ["p", "q"]);
        assert_eq!(response.default.as_deref(), Some("q"));
    }

    #[tokio::test]
    async fn reminders_skip_failing_box_unless_all_fail() {
        let good = Env::new("ok");
        let reminders = good.dir.path().join("reminders.json");
        let at = now().to_rfc3339();
        std::fs::write(
            &reminders,
            serde_json::to_string(&json!([{"at": at, "item": item("1", None)}])).unwrap(),
        )
        .unwrap();
        let script = |env: &Env, id: &str| {
            let path = env.dir.path().join(format!("rem-{id}.sh"));
            let body = format!(
                "#!/bin/sh\nFAKE_MODE_FILE='{}' FAKE_REMINDERS_FILE='{}' exec '{}'\n",
                env.dir.path().join("mode").display(),
                reminders.display(),
                fake_plugin().display()
            );
            let status = Command::new("sh")
                .args(["-c", "printf %s \"$1\" > \"$2\" && chmod 755 \"$2\"", "sh"])
                .arg(body)
                .arg(&path)
                .status()
                .unwrap();
            assert!(status.success());
            PluginHost::new(id.into(), path, json!({}), env.dir.path().join("h.log"))
                .with_timeout(Duration::from_secs(10))
        };
        let bad = Env::new("fail-reminders");
        let p = profile(
            "p",
            vec![
                spec("a", "fake", LONG, None),
                spec("b", "other", LONG, None),
            ],
            None,
        );
        let mixed = board(vec![(
            "p",
            p.clone(),
            vec![Ok(script(&good, "a")), Ok(script(&bad, "b"))],
        )]);
        let response = mixed.reminders("p", now()).await.unwrap();
        assert_eq!(response.reminders.len(), 1);
        assert_eq!(response.reminders[0].box_id, "a");
        let all_bad = board(vec![(
            "p",
            p,
            vec![Ok(script(&bad, "a")), Ok(script(&bad, "b"))],
        )]);
        let err = all_bad.reminders("p", now()).await.unwrap_err();
        assert_eq!(err.code(), ErrorCode::Plugin);
    }
}

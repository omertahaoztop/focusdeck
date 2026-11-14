//! Server-side boxes: one plugin host per profile box, cached lists, plan links and error mapping.

use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::Context;
use chrono::{DateTime, FixedOffset};
use seenboard_protocol::{
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
use crate::i18n::{self, msg, t, t_args};
use crate::links::closures_after_list;
use crate::paths::Paths;
use crate::profile::{self, BoxSpec, Profile};
use crate::ui::run::build_host_typed;

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
            BoardError::NotFound => t(msg::BOARD_NOT_FOUND),
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

/// Builds a box's host; the error text is safe to send to API clients.
pub(crate) type Builder = Box<dyn Fn() -> Result<PluginHost, String> + Send + Sync>;

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
    build: Builder,
    host: Mutex<Option<PluginHost>>,
    describe: StdMutex<Option<Result<Describe, String>>>,
    cache: StdMutex<Cache>,
    link_running: AtomicBool,
}

struct HostGuard<'a>(MutexGuard<'a, Option<PluginHost>>);

impl Deref for HostGuard<'_> {
    type Target = PluginHost;

    fn deref(&self) -> &PluginHost {
        self.0
            .as_ref()
            .expect("the host is built before the guard is handed out")
    }
}

impl DerefMut for HostGuard<'_> {
    fn deref_mut(&mut self) -> &mut PluginHost {
        self.0
            .as_mut()
            .expect("the host is built before the guard is handed out")
    }
}

impl BoxState {
    fn new(spec: BoxSpec, build: Builder) -> BoxState {
        BoxState {
            spec,
            build,
            host: Mutex::new(None),
            describe: StdMutex::new(None),
            cache: StdMutex::new(Cache::default()),
            link_running: AtomicBool::new(false),
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

    /// Takes the box lock and makes sure the plugin answers describe, building and restarting it as needed.
    async fn ready(&self) -> Result<(HostGuard<'_>, Describe), BoardError> {
        let mut guard = self.host.lock().await;
        if guard.is_none() {
            match (self.build)() {
                Ok(host) => *guard = Some(host),
                Err(message) => {
                    self.remember(Err(message.clone()));
                    return Err(BoardError::Plugin(message));
                }
            }
        }
        let mut guard = HostGuard(guard);
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
        // the slack keeps a client polling at `refresh` from landing on the cache
        let slack = (self.spec.refresh / 10).min(Duration::from_secs(2));
        if fetched.elapsed() >= self.spec.refresh.saturating_sub(slack) {
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

struct Inner {
    profiles: BTreeMap<String, ProfileState>,
    words: CaptureWords,
    language: Option<String>,
    default_profile: Option<String>,
    // tests await the background plan-link steps through these handles
    #[cfg(test)]
    links: StdMutex<Vec<tokio::task::JoinHandle<()>>>,
}

pub struct Board(Arc<Inner>);

/// Builds the real host of one box; failures are logged in full and reported without file names.
pub(crate) fn host_builder(paths: Paths, home: PathBuf, spec: BoxSpec) -> Builder {
    Box::new(move || {
        build_host_typed(&paths, &home, &spec).map_err(|e| {
            eprintln!("box {}: {e}", spec.id);
            e.public()
        })
    })
}

impl Board {
    pub fn load(paths: &Paths, cfg: &Config, home: &Path) -> anyhow::Result<Board> {
        let dir = paths.config_dir.join("profiles");
        let no_profiles = || {
            t_args(
                msg::SERVER_NO_PROFILES,
                &[("dir", dir.display().to_string().as_str().into())],
            )
        };
        let mut stems = Vec::new();
        for entry in std::fs::read_dir(&dir).with_context(no_profiles)? {
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
            anyhow::bail!(no_profiles());
        }
        let mut loaded = Vec::new();
        for stem in stems {
            let profile = profile::load(&paths.profile_path(&stem))?;
            let builders = profile
                .boxes
                .iter()
                .map(|spec| host_builder(paths.clone(), home.to_path_buf(), spec.clone()))
                .collect();
            loaded.push((stem, profile, builders));
        }
        Ok(Board::from_parts(
            loaded,
            cfg.capture_words.clone(),
            cfg.language.clone(),
            cfg.default_profile.clone(),
        ))
    }

    pub(crate) fn from_parts(
        loaded: Vec<(String, Profile, Vec<Builder>)>,
        words: CaptureWords,
        language: Option<String>,
        default_profile: Option<String>,
    ) -> Board {
        let profiles = loaded
            .into_iter()
            .map(|(name, profile, builders)| {
                let boxes = profile
                    .boxes
                    .iter()
                    .cloned()
                    .zip(builders)
                    .map(|(spec, build)| BoxState::new(spec, build))
                    .collect();
                (name, ProfileState { profile, boxes })
            })
            .collect();
        Board(Arc::new(Inner {
            profiles,
            words,
            language,
            default_profile,
            #[cfg(test)]
            links: StdMutex::new(Vec::new()),
        }))
    }

    pub async fn profiles(&self) -> ProfilesResponse {
        self.0.profiles_response()
    }

    pub async fn profile_view(&self, p: &str) -> Result<ProfileView, BoardError> {
        self.0.profile_view(p).await
    }

    pub async fn items(
        &self,
        p: &str,
        b: &str,
        now: DateTime<FixedOffset>,
    ) -> Result<ItemsResponse, BoardError> {
        let state = self.0.profile(p)?;
        let (idx, bx) = state.find(b)?;
        let (response, real) = self.0.list_box(bx, now).await?;
        if real {
            self.spawn_link(p, idx, response.items.clone(), now);
        }
        Ok(response)
    }

    // runs after the source list is answered, so a slow target never delays it
    fn spawn_link(&self, p: &str, idx: usize, items: Vec<Item>, now: DateTime<FixedOffset>) {
        let source = &self.0.profiles[p].boxes[idx];
        if source.spec.plan_into.is_none() || source.link_running.swap(true, Ordering::SeqCst) {
            return;
        }
        let inner = Arc::clone(&self.0);
        let p = p.to_owned();
        let handle = tokio::spawn(async move {
            inner.close_linked(&p, idx, &items, now).await;
            inner.profiles[&p].boxes[idx]
                .link_running
                .store(false, Ordering::SeqCst);
        });
        #[cfg(test)]
        self.0
            .links
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(handle);
        #[cfg(not(test))]
        drop(handle);
    }

    #[cfg(test)]
    async fn settle_links(&self) {
        let handles: Vec<_> = std::mem::take(&mut *self.0.links.lock().unwrap());
        for handle in handles {
            handle.await.unwrap();
        }
    }

    pub async fn action(
        &self,
        p: &str,
        b: &str,
        req: ActionRequest,
    ) -> Result<ActionResponse, BoardError> {
        let (_, bx) = self.0.profile(p)?.find(b)?;
        self.0.run_action(bx, req).await
    }

    pub async fn drop(
        &self,
        p: &str,
        b: &str,
        req: DropRequest,
    ) -> Result<DropResponse, BoardError> {
        let (_, bx) = self.0.profile(p)?.find(b)?;
        self.0.run_drop(bx, req).await
    }

    pub async fn capture(
        &self,
        p: &str,
        req: CaptureRequest,
    ) -> Result<CaptureResponse, BoardError> {
        self.0.capture(p, req).await
    }

    /// A box that fails is skipped; only when every box that was asked fails does the call answer `Plugin`.
    pub async fn reminders(
        &self,
        p: &str,
        since: DateTime<FixedOffset>,
    ) -> Result<RemindersResponse, BoardError> {
        self.0.reminders(p, since).await
    }
}

impl Inner {
    fn profile(&self, p: &str) -> Result<&ProfileState, BoardError> {
        self.profiles.get(p).ok_or(BoardError::NotFound)
    }

    fn profiles_response(&self) -> ProfilesResponse {
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

    async fn profile_view(&self, p: &str) -> Result<ProfileView, BoardError> {
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
        let started = Instant::now();
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
                    cache.fetched = Some(started);
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
    async fn close_linked(&self, p: &str, idx: usize, items: &[Item], now: DateTime<FixedOffset>) {
        let state = &self.profiles[p];
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
            if let Err(e) = self.run_action(target, request).await {
                eprintln!("plan link into {}: {e}", target.spec.id);
            }
        }
    }

    async fn run_action(
        &self,
        bx: &BoxState,
        req: ActionRequest,
    ) -> Result<ActionResponse, BoardError> {
        let (mut host, describe) = bx.ready().await?;
        if !describe.actions.contains(&req.action) {
            return Err(BoardError::Unsupported(t_args(
                msg::BOARD_UNSUPPORTED_ACTION,
                &[
                    ("box", bx.spec.id.as_str().into()),
                    ("action", req.action.as_str().into()),
                ],
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

    async fn run_drop(&self, bx: &BoxState, req: DropRequest) -> Result<DropResponse, BoardError> {
        let (mut host, describe) = bx.ready().await?;
        if !describe.accepts_drop {
            return Err(BoardError::Unsupported(t_args(
                msg::BOARD_NO_DROPS,
                &[("box", bx.spec.id.as_str().into())],
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

    async fn capture(&self, p: &str, req: CaptureRequest) -> Result<CaptureResponse, BoardError> {
        let state = self.profile(p)?;
        let (target_id, bx) = state
            .profile
            .capture_into
            .as_deref()
            .and_then(|id| state.find(id).ok().map(|(_, bx)| (id, bx)))
            .ok_or_else(|| BoardError::Invalid(t(msg::BOARD_NO_CAPTURE_INTO)))?;
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

    async fn reminders(
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
    use seenboard_protocol::Source;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::Ordering;
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

    type BoxHosts = Vec<Result<PluginHost, String>>;

    fn once(host: Result<PluginHost, String>) -> Builder {
        let cell = StdMutex::new(Some(host));
        Box::new(move || {
            cell.lock()
                .unwrap()
                .take()
                .unwrap_or_else(|| Err("host already taken".into()))
        })
    }

    fn builders(hosts: BoxHosts) -> Vec<Builder> {
        hosts.into_iter().map(once).collect()
    }

    fn board(entries: Vec<(&str, Profile, BoxHosts)>) -> Board {
        Board::from_parts(
            entries
                .into_iter()
                .map(|(name, p, hosts)| (name.to_owned(), p, builders(hosts)))
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
    async fn poll_one_refresh_apart_reaches_the_plugin_but_half_a_refresh_does_not() {
        let env = Env::new("slow-list");
        let refresh = Duration::from_secs(1);
        let p = profile("p", vec![spec("a", "fake", refresh, None)], None);
        let board = board(vec![("p", p, vec![Ok(env.host("a"))])]);
        board.profile_view("p").await.unwrap();
        let t0 = Instant::now();
        board.items("p", "a", now()).await.unwrap();
        tokio::time::sleep_until((t0 + refresh / 2).into()).await;
        board.items("p", "a", now()).await.unwrap();
        assert_eq!(env.count("list"), 1);
        tokio::time::sleep_until((t0 + refresh).into()).await;
        board.items("p", "a", now()).await.unwrap();
        assert_eq!(env.count("list"), 2);
    }

    #[tokio::test]
    async fn list_overtaken_by_an_action_does_not_refill_the_cache() {
        let slow = Env::new("slow-list");
        let other = Env::new("ok");
        let p = profile(
            "p",
            vec![spec("a", "fake", LONG, None), spec("b", "fake", LONG, None)],
            None,
        );
        let board = Arc::new(board(vec![(
            "p",
            p,
            vec![Ok(slow.host("a")), Ok(other.host("b"))],
        )]));
        let listing = {
            let board = Arc::clone(&board);
            tokio::spawn(async move { board.items("p", "a", now()).await })
        };
        while slow.count("list") == 0 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        board.action("p", "b", done("1")).await.unwrap();
        listing.await.unwrap().unwrap();
        board.items("p", "a", now()).await.unwrap();
        assert_eq!(slow.count("list"), 2);
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
    async fn broken_box_reports_error_in_profile() {
        let env = Env::new("ok");
        let p = profile(
            "p",
            vec![spec("a", "fake", LONG, None), spec("b", "gone", LONG, None)],
            None,
        );
        let gone: Builder = Box::new(|| Err("plugin gone is not installed".into()));
        let board = Board::from_parts(
            vec![("p".into(), p, vec![once(Ok(env.host("a"))), gone])],
            CaptureWords::default(),
            None,
            None,
        );
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
    async fn box_whose_host_failed_to_build_recovers_without_restart() {
        let env = Env::new("ok");
        let p = profile("p", vec![spec("a", "fake", LONG, None)], None);
        let host = StdMutex::new(Some(env.host("a")));
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let flaky: Builder = Box::new(move || {
            if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                return Err("plugin fake is not installed".into());
            }
            Ok(host.lock().unwrap().take().unwrap())
        });
        let board = Board::from_parts(
            vec![("p".into(), p, vec![flaky])],
            CaptureWords::default(),
            None,
            None,
        );
        assert_eq!(
            board.items("p", "a", now()).await.unwrap_err().code(),
            ErrorCode::Plugin
        );
        assert_eq!(board.items("p", "a", now()).await.unwrap().items.len(), 2);
        let view = board.profile_view("p").await.unwrap();
        assert!(view.boxes[0].error.is_none());
    }

    #[tokio::test]
    async fn failed_describe_is_retried_by_the_next_request() {
        let env = Env::new("oldproto");
        let p = profile("p", vec![spec("a", "fake", LONG, None)], None);
        let board = board(vec![("p", p, vec![Ok(env.host("a"))])]);
        let view = board.profile_view("p").await.unwrap();
        assert!(view.boxes[0].error.is_some());
        assert!(board.items("p", "a", now()).await.is_err());
        env.set_mode("ok");
        assert_eq!(board.items("p", "a", now()).await.unwrap().items.len(), 2);
        assert!(
            board.profile_view("p").await.unwrap().boxes[0]
                .error
                .is_none()
        );
    }

    #[tokio::test]
    async fn secret_file_path_never_reaches_api_errors() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: home.path().join("config"),
            data_dir: home.path().join("data"),
            state_dir: home.path().join("state"),
        };
        let mut spec = spec("a", "local", LONG, None);
        spec.config = "token_file = \"~/nokey-file\"".parse().unwrap();
        let p = profile("p", vec![spec.clone()], None);
        let builder = host_builder(paths, home.path().to_path_buf(), spec);
        let board = Board::from_parts(
            vec![("p".into(), p, vec![builder])],
            CaptureWords::default(),
            None,
            None,
        );
        let view = board.profile_view("p").await.unwrap();
        let shown = view.boxes[0].error.clone().unwrap();
        let listed = board.items("p", "a", now()).await.unwrap_err().message();
        for text in [shown, listed] {
            let home = home.path().to_string_lossy();
            assert!(!text.contains(home.as_ref()), "{text}");
            assert!(!text.contains("nokey-file"), "{text}");
            assert!(text.contains("secret file"), "{text}");
        }
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
        board.settle_links().await;
        assert_eq!(target.count("action"), 0);
        board.items("p", "from", now()).await.unwrap();
        board.settle_links().await;
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
        board.settle_links().await;
        assert_eq!(source.count("list"), 1);
        assert_eq!(target.count("action"), 0);
    }

    #[tokio::test]
    async fn hung_link_target_does_not_delay_source_list() {
        let source = Env::new("ok");
        let target = Env::new("hang");
        let p = profile(
            "p",
            vec![
                spec("from", "src", Duration::ZERO, Some("to")),
                spec("to", "tgt", LONG, None),
            ],
            None,
        );
        let hanging = target.host("to").with_timeout(Duration::from_millis(1500));
        let board = board(vec![("p", p, vec![Ok(source.host("from")), Ok(hanging)])]);
        let started = Instant::now();
        let listed = board.items("p", "from", now()).await.unwrap();
        assert_eq!(listed.items.len(), 2);
        assert!(started.elapsed() < Duration::from_millis(1000));
        board.settle_links().await;
        assert!(started.elapsed() >= Duration::from_millis(1500));
    }

    #[tokio::test]
    async fn concurrent_actions_on_one_box_are_serialized() {
        let env = Env::new("slow-action");
        let p = profile("p", vec![spec("a", "fake", LONG, None)], None);
        let board = Arc::new(board(vec![("p", p, vec![Ok(env.host("a"))])]));
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
        let log = env.log();
        assert!(!log.contains("overlap"), "{log}");
        assert_eq!(env.count("action "), 3, "{log}");
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
                ("p".into(), one(), builders(vec![Ok(env.host("a"))])),
                ("q".into(), one(), builders(vec![Ok(env.host("a"))])),
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

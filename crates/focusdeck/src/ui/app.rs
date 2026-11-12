use crate::links::{items_to_close, linked_ids, linked_item};
use crate::profile::{BoxSpec, Profile};
use crate::worker::BoxEvent;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use focusdeck_protocol::{Describe, Item, Source, View};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

#[derive(Debug, Clone)]
pub struct BoxState {
    pub spec: BoxSpec,
    pub describe: Option<Describe>,
    pub items: Vec<Item>,
    pub error: Option<String>,
    pub selected: usize,
    pub scroll: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Board,
    Zoom(usize),
    Capture { input: String },
    MovePicker { from: usize, item: String },
    Search { zoom: usize, query: String },
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub box_idx: usize,
    pub row: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Effect {
    Refresh(usize, View),
    Action {
        idx: usize,
        item: String,
        action: String,
        args: Value,
    },
    Drop {
        idx: usize,
        item: Item,
        from: Option<Source>,
    },
    Open(String),
    Capture(String),
    Quit,
}

#[derive(Debug, Clone, Copy)]
struct Pressed {
    box_idx: usize,
    row: usize,
}

#[derive(Debug)]
pub struct App {
    pub boxes: Vec<BoxState>,
    pub focus: usize,
    pub mode: Mode,
    pub flash: Option<String>,
    return_to: Mode,
    filter: String,
    pressed: Option<Pressed>,
    last_click: Option<(Pressed, Instant)>,
}

pub fn fold_tr(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            'İ' => 'i'.to_lowercase(),
            'I' => 'ı'.to_lowercase(),
            c => c.to_lowercase(),
        })
        .collect()
}

fn has_action(state: &BoxState, action: &str) -> bool {
    state
        .describe
        .as_ref()
        .is_some_and(|d| d.actions.iter().any(|a| a == action))
}

fn source_of(spec: &BoxSpec, item: &Item) -> Source {
    Source {
        plugin: spec.plugin.clone(),
        id: item.id.clone(),
    }
}

fn plain_action(idx: usize, item: &str, action: &str, args: Value) -> Effect {
    Effect::Action {
        idx,
        item: item.to_owned(),
        action: action.to_owned(),
        args,
    }
}

impl App {
    pub fn new(profile: &Profile) -> App {
        App {
            boxes: profile
                .boxes
                .iter()
                .map(|spec| BoxState {
                    spec: spec.clone(),
                    describe: None,
                    items: Vec::new(),
                    error: None,
                    selected: 0,
                    scroll: 0,
                })
                .collect(),
            focus: 0,
            mode: Mode::Board,
            flash: None,
            return_to: Mode::Board,
            filter: String::new(),
            pressed: None,
            last_click: None,
        }
    }

    fn plan_target(&self, idx: usize) -> Option<usize> {
        let target = self.boxes.get(idx)?.spec.plan_into.as_deref()?;
        self.boxes.iter().position(|b| b.spec.id == target)
    }

    pub fn visible(&self, idx: usize) -> Vec<&Item> {
        let Some(state) = self.boxes.get(idx) else {
            return Vec::new();
        };
        let query = match &self.mode {
            Mode::Search { zoom, query } if *zoom == idx => Some(query.as_str()),
            _ if self.zoomed_box() == Some(idx) => Some(self.filter.as_str()),
            _ => None,
        };
        match query {
            Some(query) => {
                let needle = fold_tr(query);
                state
                    .items
                    .iter()
                    .filter(|i| needle.is_empty() || fold_tr(&i.title).contains(&needle))
                    .collect()
            }
            None => match self.plan_target(idx) {
                Some(target) => {
                    let linked = linked_ids(&self.boxes[target].items, &state.spec.plugin);
                    state
                        .items
                        .iter()
                        .filter(|i| linked.contains(&i.id))
                        .collect()
                }
                None => state.items.iter().collect(),
            },
        }
    }

    pub fn zoomed_box(&self) -> Option<usize> {
        match (&self.mode, &self.return_to) {
            (Mode::Zoom(idx), _) | (Mode::Search { zoom: idx, .. }, _) => Some(*idx),
            (Mode::Capture { .. } | Mode::MovePicker { .. } | Mode::Help, Mode::Zoom(idx)) => {
                Some(*idx)
            }
            _ => None,
        }
    }

    pub fn planned_count(&self, idx: usize) -> Option<(usize, usize)> {
        let target = self.plan_target(idx)?;
        let state = &self.boxes[idx];
        let linked = linked_ids(&self.boxes[target].items, &state.spec.plugin);
        let planned = state
            .items
            .iter()
            .filter(|i| linked.contains(&i.id))
            .count();
        Some((planned, state.items.len()))
    }

    pub fn is_planned(&self, idx: usize, item: &Item) -> bool {
        self.plan_target(idx).is_some_and(|target| {
            linked_item(
                &self.boxes[target].items,
                &self.boxes[idx].spec.plugin,
                &item.id,
            )
            .is_some()
        })
    }

    fn clamp(&mut self, idx: usize) {
        let len = self.visible(idx).len();
        let state = &mut self.boxes[idx];
        state.selected = state.selected.min(len.saturating_sub(1));
        state.scroll = state.scroll.min(state.selected);
    }

    fn selected_item(&self, idx: usize) -> Option<Item> {
        let selected = self.boxes.get(idx)?.selected;
        self.visible(idx).get(selected).map(|i| (*i).clone())
    }

    fn step(&mut self, idx: usize, down: bool) {
        let len = self.visible(idx).len();
        let state = &mut self.boxes[idx];
        if down {
            state.selected = (state.selected + 1).min(len.saturating_sub(1));
        } else {
            state.selected = state.selected.saturating_sub(1);
        }
        state.scroll = state.scroll.min(state.selected);
    }

    fn scroll_wheel(&mut self, idx: usize, down: bool) {
        let len = self.visible(idx).len();
        let state = &mut self.boxes[idx];
        if down {
            state.selected = (state.selected + 1).min(len.saturating_sub(1));
            state.scroll = (state.scroll + 1).min(state.selected);
        } else {
            state.selected = state.selected.saturating_sub(1);
            state.scroll = state.scroll.saturating_sub(1);
        }
    }

    fn enter_mode(&mut self, mode: Mode) {
        self.return_to = self.mode.clone();
        self.mode = mode;
    }

    fn leave_mode(&mut self) {
        self.mode = std::mem::replace(&mut self.return_to, Mode::Board);
    }

    fn zoom(&mut self, idx: usize) -> Vec<Effect> {
        self.focus = idx;
        self.filter.clear();
        self.mode = Mode::Zoom(idx);
        self.clamp(idx);
        vec![Effect::Refresh(idx, View::Full)]
    }

    pub fn on_event(&mut self, ev: BoxEvent) -> Vec<Effect> {
        match ev {
            BoxEvent::Described { idx, describe } => {
                if let Some(state) = self.boxes.get_mut(idx) {
                    state.describe = Some(describe);
                }
                Vec::new()
            }
            BoxEvent::Listed { idx, view, result } => {
                if idx >= self.boxes.len() {
                    return Vec::new();
                }
                match result {
                    Ok(items) => {
                        let effects = self.link_closures(idx, view, &items);
                        let state = &mut self.boxes[idx];
                        state.items = items;
                        state.error = None;
                        self.clamp(idx);
                        effects
                    }
                    Err(message) => {
                        self.boxes[idx].error = Some(message);
                        Vec::new()
                    }
                }
            }
            BoxEvent::Finished { idx, what, result } => match result {
                Ok(()) => {
                    let view = if self.zoomed_box() == Some(idx) {
                        View::Full
                    } else {
                        View::Summary
                    };
                    vec![Effect::Refresh(idx, view)]
                }
                Err(message) => {
                    self.flash = Some(format!("{what} başarısız: {message}"));
                    Vec::new()
                }
            },
        }
    }

    fn link_closures(&self, idx: usize, view: View, items: &[Item]) -> Vec<Effect> {
        if view != View::Full {
            return Vec::new();
        }
        let Some(target) = self.plan_target(idx) else {
            return Vec::new();
        };
        items_to_close(
            &self.boxes[idx].spec.plugin,
            items,
            &self.boxes[target].items,
        )
        .into_iter()
        .map(|id| plain_action(target, &id, "done", json!({})))
        .collect()
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.kind == KeyEventKind::Release {
            return Vec::new();
        }
        self.flash = None;
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return vec![Effect::Quit];
        }
        match self.mode.clone() {
            Mode::Board | Mode::Zoom(_) => self.on_browse_key(key),
            Mode::Capture { input } => self.on_capture_key(key, input),
            Mode::MovePicker { from, item } => self.on_picker_key(key, from, &item),
            Mode::Search { zoom, query } => self.on_search_key(key, zoom, query),
            Mode::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                    self.leave_mode();
                }
                Vec::new()
            }
        }
    }

    fn on_browse_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        let zoomed = matches!(self.mode, Mode::Zoom(_));
        let idx = self.focus;
        if idx >= self.boxes.len() {
            return Vec::new();
        }
        match key.code {
            KeyCode::Char('q') => vec![Effect::Quit],
            KeyCode::Char('?') => {
                self.enter_mode(Mode::Help);
                Vec::new()
            }
            KeyCode::Char('a') => {
                self.enter_mode(Mode::Capture {
                    input: String::new(),
                });
                Vec::new()
            }
            KeyCode::Tab if !zoomed => {
                self.focus = (idx + 1) % self.boxes.len();
                Vec::new()
            }
            KeyCode::Down => {
                self.step(idx, true);
                Vec::new()
            }
            KeyCode::Up => {
                self.step(idx, false);
                Vec::new()
            }
            KeyCode::Enter if !zoomed => self.zoom(idx),
            KeyCode::Esc if zoomed => {
                self.filter.clear();
                self.mode = Mode::Board;
                self.clamp(idx);
                Vec::new()
            }
            KeyCode::Char('/') if zoomed => {
                self.mode = Mode::Search {
                    zoom: idx,
                    query: self.filter.clone(),
                };
                Vec::new()
            }
            KeyCode::Char('x') => self.require_action(idx, "done", "bu kutuda bitirme yok"),
            KeyCode::Char('u') => {
                self.require_action(idx, "unstar", "bu kutuda yıldız kaldırma yok")
            }
            KeyCode::Char('o') => self.open_selected(idx),
            KeyCode::Char('m') => {
                if let Some(item) = self.selected_item(idx) {
                    self.enter_mode(Mode::MovePicker {
                        from: idx,
                        item: item.id,
                    });
                }
                Vec::new()
            }
            KeyCode::Char(' ') if zoomed => self.toggle_plan(idx),
            _ => Vec::new(),
        }
    }

    fn require_action(&mut self, idx: usize, action: &str, missing: &str) -> Vec<Effect> {
        if !has_action(&self.boxes[idx], action) {
            self.flash = Some(missing.to_owned());
            return Vec::new();
        }
        match self.selected_item(idx) {
            Some(item) => vec![plain_action(idx, &item.id, action, json!({}))],
            None => Vec::new(),
        }
    }

    fn open_selected(&mut self, idx: usize) -> Vec<Effect> {
        let Some(item) = self.selected_item(idx) else {
            return Vec::new();
        };
        match item.url {
            Some(url) => vec![Effect::Open(url)],
            None => {
                self.flash = Some("bu öğenin linki yok".into());
                Vec::new()
            }
        }
    }

    fn toggle_plan(&mut self, idx: usize) -> Vec<Effect> {
        let (Some(target), Some(item)) = (self.plan_target(idx), self.selected_item(idx)) else {
            return Vec::new();
        };
        let spec = &self.boxes[idx].spec;
        match linked_item(&self.boxes[target].items, &spec.plugin, &item.id) {
            Some(task) => vec![plain_action(target, &task.id, "delete", json!({}))],
            None => vec![Effect::Drop {
                idx: target,
                from: Some(source_of(spec, &item)),
                item,
            }],
        }
    }

    fn on_capture_key(&mut self, key: KeyEvent, mut input: String) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                self.leave_mode();
                Vec::new()
            }
            KeyCode::Enter => {
                self.leave_mode();
                let text = input.trim();
                if text.is_empty() {
                    Vec::new()
                } else {
                    vec![Effect::Capture(text.to_owned())]
                }
            }
            KeyCode::Backspace => {
                input.pop();
                self.mode = Mode::Capture { input };
                Vec::new()
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                input.push(c);
                self.mode = Mode::Capture { input };
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn picker_targets(&self, from: usize) -> Vec<usize> {
        (0..self.boxes.len())
            .filter(|&i| {
                i != from
                    && self.boxes[i]
                        .describe
                        .as_ref()
                        .is_some_and(|d| d.accepts_drop)
            })
            .collect()
    }

    fn on_picker_key(&mut self, key: KeyEvent, from: usize, item_id: &str) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                self.leave_mode();
                Vec::new()
            }
            KeyCode::Char(c @ '1'..='9') => {
                let n = c as usize - '1' as usize;
                let Some(&target) = self.picker_targets(from).get(n) else {
                    return Vec::new();
                };
                self.leave_mode();
                let source = &self.boxes[from];
                match source.items.iter().find(|i| i.id == item_id) {
                    Some(item) => vec![Effect::Drop {
                        idx: target,
                        from: Some(source_of(&source.spec, item)),
                        item: item.clone(),
                    }],
                    None => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    fn on_search_key(&mut self, key: KeyEvent, zoom: usize, mut query: String) -> Vec<Effect> {
        match key.code {
            KeyCode::Esc => {
                self.filter.clear();
                self.mode = Mode::Zoom(zoom);
            }
            KeyCode::Enter => {
                self.filter = query;
                self.mode = Mode::Zoom(zoom);
            }
            KeyCode::Down => self.step(zoom, true),
            KeyCode::Up => self.step(zoom, false),
            KeyCode::Backspace => {
                query.pop();
                self.mode = Mode::Search { zoom, query };
                self.boxes[zoom].selected = 0;
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                query.push(c);
                self.mode = Mode::Search { zoom, query };
                self.boxes[zoom].selected = 0;
            }
            _ => return Vec::new(),
        }
        self.clamp(zoom);
        Vec::new()
    }

    pub fn on_mouse(
        &mut self,
        ev: MouseEvent,
        now: Instant,
        hit: impl Fn(u16, u16) -> Option<Hit>,
    ) -> Vec<Effect> {
        if !matches!(self.mode, Mode::Board | Mode::Zoom(_)) {
            return Vec::new();
        }
        let target = hit(ev.column, ev.row).filter(|h| h.box_idx < self.boxes.len());
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => self.on_press(target, now),
            MouseEventKind::Up(MouseButton::Left) => self.on_release(target),
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                if let Some(h) = target {
                    self.focus = h.box_idx;
                    self.scroll_wheel(h.box_idx, ev.kind == MouseEventKind::ScrollDown);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn on_press(&mut self, target: Option<Hit>, now: Instant) -> Vec<Effect> {
        self.flash = None;
        self.pressed = None;
        let Some(hit) = target else {
            return Vec::new();
        };
        self.focus = hit.box_idx;
        let Some(row) = hit.row else {
            self.last_click = None;
            return Vec::new();
        };
        let pressed = Pressed {
            box_idx: hit.box_idx,
            row,
        };
        if row < self.visible(hit.box_idx).len() {
            self.boxes[hit.box_idx].selected = row;
        } else {
            self.last_click = None;
            return Vec::new();
        }
        self.pressed = Some(pressed);
        let double = self.last_click.is_some_and(|(prev, at)| {
            prev.box_idx == pressed.box_idx
                && prev.row == pressed.row
                && now.saturating_duration_since(at) <= DOUBLE_CLICK
        });
        if double {
            self.last_click = None;
            self.pressed = None;
            return if matches!(self.mode, Mode::Zoom(_)) {
                Vec::new()
            } else {
                self.zoom(hit.box_idx)
            };
        }
        self.last_click = Some((pressed, now));
        Vec::new()
    }

    fn on_release(&mut self, target: Option<Hit>) -> Vec<Effect> {
        let Some(from) = self.pressed.take() else {
            return Vec::new();
        };
        let Some(hit) = target else {
            return Vec::new();
        };
        let Some(item) = self
            .visible(from.box_idx)
            .get(from.row)
            .map(|i| (*i).clone())
        else {
            return Vec::new();
        };
        if hit.box_idx == from.box_idx {
            let Some(row) = hit.row.filter(|&r| r != from.row) else {
                return Vec::new();
            };
            self.last_click = None;
            if !has_action(&self.boxes[from.box_idx], "reorder") {
                return Vec::new();
            }
            return vec![plain_action(
                from.box_idx,
                &item.id,
                "reorder",
                json!({ "position": row }),
            )];
        }
        self.last_click = None;
        let target = &self.boxes[hit.box_idx];
        if target.describe.as_ref().is_some_and(|d| d.accepts_drop) {
            vec![Effect::Drop {
                idx: hit.box_idx,
                from: Some(source_of(&self.boxes[from.box_idx].spec, &item)),
                item,
            }]
        } else {
            self.flash = Some(format!(
                "{} kutusu bırakılan öğe kabul etmiyor",
                target.spec.title
            ));
            Vec::new()
        }
    }
}

#[cfg(test)]
pub(crate) mod testkit {
    use super::*;
    use crate::profile::{BoxSpec, Profile};
    use crate::worker::BoxEvent;
    use focusdeck_protocol::{Describe, Item, Source, View};
    use std::collections::BTreeMap;
    use std::time::Duration;

    pub const SOZLU: usize = 0;
    pub const REDMINE: usize = 1;
    pub const ZULIP: usize = 2;

    fn spec(id: &str, plugin: &str, plan_into: Option<&str>) -> BoxSpec {
        BoxSpec {
            id: id.into(),
            title: id.into(),
            plugin: plugin.into(),
            row: 0,
            col: 0,
            width: 1,
            height: 1,
            refresh: Duration::from_secs(120),
            plan_into: plan_into.map(str::to_owned),
            config: toml::Table::new(),
        }
    }

    pub fn item(id: &str, title: &str) -> Item {
        Item {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            url: None,
            due: None,
            overdue: false,
            source: None,
            description: None,
            labels: vec![],
            from: None,
            links: vec![],
        }
    }

    pub fn linked(id: &str, title: &str, plugin: &str, sid: &str) -> Item {
        Item {
            source: Some(Source {
                plugin: plugin.into(),
                id: sid.into(),
            }),
            ..item(id, title)
        }
    }

    pub fn with_url(mut it: Item, url: &str) -> Item {
        it.url = Some(url.into());
        it
    }

    pub fn describe(name: &str, actions: &[&str], accepts_drop: bool) -> Describe {
        Describe {
            name: name.into(),
            version: "0.1.0".into(),
            protocol: 1,
            actions: actions.iter().map(|a| a.to_string()).collect(),
            accepts_drop,
            reminders: false,
        }
    }

    pub fn profile() -> Profile {
        Profile {
            name: "iş".into(),
            capture_into: Some("sozlu".into()),
            capture_refs: BTreeMap::new(),
            capture_label_aliases: BTreeMap::new(),
            boxes: vec![
                spec("sozlu", "vikunja", None),
                spec("redmine", "redmine", Some("sozlu")),
                spec("zulip", "zulip", None),
            ],
        }
    }

    pub fn list(app: &mut App, idx: usize, view: View, items: Vec<Item>) -> Vec<Effect> {
        app.on_event(BoxEvent::Listed {
            idx,
            view,
            result: Ok(items),
        })
    }

    pub fn loaded_app() -> App {
        let mut app = App::new(&profile());
        app.on_event(BoxEvent::Described {
            idx: SOZLU,
            describe: describe("vikunja", &["done", "delete", "move", "reorder"], true),
        });
        app.on_event(BoxEvent::Described {
            idx: REDMINE,
            describe: describe("redmine", &[], false),
        });
        app.on_event(BoxEvent::Described {
            idx: ZULIP,
            describe: describe("zulip", &["unstar"], false),
        });
        list(
            &mut app,
            SOZLU,
            View::Full,
            vec![
                item("t1", "birinci görev"),
                linked("t2", "Tüyap hazırlığı", "redmine", "2"),
                item("t3", "üçüncü görev"),
            ],
        );
        list(
            &mut app,
            REDMINE,
            View::Full,
            vec![
                with_url(item("1", "İstanbul sunucusu"), "https://destek/1"),
                item("2", "TÜYAP hazırlığı"),
                item("3", "ISPARTA kurulumu"),
            ],
        );
        list(
            &mut app,
            ZULIP,
            View::Full,
            vec![item("z1", "zulip mesajı"), item("z2", "ikinci mesaj")],
        );
        app
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;
    use crate::worker::BoxEvent;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use focusdeck_protocol::{Source, View};
    use serde_json::json;
    use std::time::{Duration, Instant};

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn code(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    fn ids(app: &App, idx: usize) -> Vec<String> {
        app.visible(idx).iter().map(|i| i.id.clone()).collect()
    }

    #[test]
    fn board_filters_plan_source_box_to_linked_items() {
        let app = loaded_app();
        assert_eq!(ids(&app, REDMINE), vec!["2"]);
        assert_eq!(ids(&app, SOZLU), vec!["t1", "t2", "t3"]);
    }

    #[test]
    fn zoom_shows_every_item_in_plugin_order() {
        let mut app = loaded_app();
        app.mode = Mode::Zoom(REDMINE);
        assert_eq!(ids(&app, REDMINE), vec!["1", "2", "3"]);
    }

    #[test]
    fn planned_count_counts_linked_over_total() {
        let app = loaded_app();
        assert_eq!(app.planned_count(REDMINE), Some((1, 3)));
        assert_eq!(app.planned_count(SOZLU), None);
        assert!(app.is_planned(REDMINE, &item("2", "x")));
        assert!(!app.is_planned(REDMINE, &item("1", "x")));
    }

    #[test]
    fn search_is_turkish_case_insensitive() {
        let mut app = loaded_app();
        for (query, expected) in [
            ("tüyap", "2"),
            ("istanbul", "1"),
            ("ısparta", "3"),
            ("İSTANBUL", "1"),
            ("ISPARTA", "3"),
        ] {
            app.mode = Mode::Search {
                zoom: REDMINE,
                query: query.into(),
            };
            assert_eq!(ids(&app, REDMINE), vec![expected], "query {query}");
        }
    }

    #[test]
    fn search_typing_enter_and_esc() {
        let mut app = loaded_app();
        app.focus = REDMINE;
        app.on_key(code(KeyCode::Enter));
        assert_eq!(app.mode, Mode::Zoom(REDMINE));
        app.on_key(key('/'));
        for c in "tüy".chars() {
            app.on_key(key(c));
        }
        assert_eq!(ids(&app, REDMINE), vec!["2"]);
        app.on_key(code(KeyCode::Enter));
        assert_eq!(app.mode, Mode::Zoom(REDMINE));
        assert_eq!(ids(&app, REDMINE), vec!["2"]);
        app.on_key(key('/'));
        app.on_key(code(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Zoom(REDMINE));
        assert_eq!(ids(&app, REDMINE), vec!["1", "2", "3"]);
    }

    #[test]
    fn full_list_closes_linked_items_that_left_the_source() {
        let mut app = loaded_app();
        let effects = list(&mut app, REDMINE, View::Full, vec![item("1", "kaldı")]);
        assert_eq!(
            effects,
            vec![Effect::Action {
                idx: SOZLU,
                item: "t2".into(),
                action: "done".into(),
                args: json!({}),
            }]
        );
    }

    #[test]
    fn full_list_that_still_has_the_item_closes_nothing() {
        let mut app = loaded_app();
        let effects = list(&mut app, REDMINE, View::Full, vec![item("2", "duruyor")]);
        assert!(effects.is_empty());
    }

    #[test]
    fn failed_list_keeps_items_sets_error_and_closes_nothing() {
        let mut app = loaded_app();
        let effects = app.on_event(BoxEvent::Listed {
            idx: REDMINE,
            view: View::Full,
            result: Err("bağlantı koptu".into()),
        });
        assert!(effects.is_empty());
        assert_eq!(app.boxes[REDMINE].items.len(), 3);
        assert_eq!(app.boxes[REDMINE].error.as_deref(), Some("bağlantı koptu"));
        list(&mut app, REDMINE, View::Full, vec![item("2", "x")]);
        assert_eq!(app.boxes[REDMINE].error, None);
    }

    #[test]
    fn summary_list_never_closes_links() {
        let mut app = loaded_app();
        let effects = list(&mut app, REDMINE, View::Summary, vec![]);
        assert!(effects.is_empty());
    }

    #[test]
    fn listed_clamps_selection() {
        let mut app = loaded_app();
        app.boxes[ZULIP].selected = 1;
        list(&mut app, ZULIP, View::Full, vec![item("z1", "tek")]);
        assert_eq!(app.boxes[ZULIP].selected, 0);
        list(&mut app, ZULIP, View::Full, vec![]);
        assert_eq!(app.boxes[ZULIP].selected, 0);
    }

    #[test]
    fn finished_ok_refreshes_and_err_flashes() {
        let mut app = loaded_app();
        let effects = app.on_event(BoxEvent::Finished {
            idx: SOZLU,
            what: "done".into(),
            result: Ok(()),
        });
        assert_eq!(effects, vec![Effect::Refresh(SOZLU, View::Summary)]);
        app.mode = Mode::Zoom(SOZLU);
        let effects = app.on_event(BoxEvent::Finished {
            idx: SOZLU,
            what: "done".into(),
            result: Ok(()),
        });
        assert_eq!(effects, vec![Effect::Refresh(SOZLU, View::Full)]);
        let effects = app.on_event(BoxEvent::Finished {
            idx: SOZLU,
            what: "done".into(),
            result: Err("sunucu hatası".into()),
        });
        assert!(effects.is_empty());
        assert!(app.flash.as_deref().unwrap().contains("sunucu hatası"));
    }

    #[test]
    fn overlay_opened_from_zoom_keeps_zoom_list_and_returns_to_zoom() {
        let mut app = loaded_app();
        app.focus = REDMINE;
        app.mode = Mode::Zoom(REDMINE);
        app.boxes[REDMINE].selected = 2;
        app.on_key(key('?'));
        assert_eq!(ids(&app, REDMINE), vec!["1", "2", "3"]);
        let same = app.boxes[REDMINE].items.clone();
        list(&mut app, REDMINE, View::Full, same);
        assert_eq!(app.boxes[REDMINE].selected, 2);
        app.on_key(code(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Zoom(REDMINE));
        app.on_key(code(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Board);
        assert_eq!(app.boxes[REDMINE].selected, 0);
    }

    #[test]
    fn capture_flow() {
        let mut app = loaded_app();
        assert!(app.on_key(key('a')).is_empty());
        assert!(matches!(app.mode, Mode::Capture { .. }));
        for c in "not x".chars() {
            app.on_key(key(c));
        }
        app.on_key(code(KeyCode::Backspace));
        for c in "yazılan".chars() {
            app.on_key(key(c));
        }
        let effects = app.on_key(code(KeyCode::Enter));
        assert_eq!(effects, vec![Effect::Capture("not yazılan".into())]);
        assert_eq!(app.mode, Mode::Board);
    }

    #[test]
    fn capture_esc_and_empty_enter_do_nothing() {
        let mut app = loaded_app();
        app.on_key(key('a'));
        app.on_key(key('b'));
        assert!(app.on_key(code(KeyCode::Esc)).is_empty());
        assert_eq!(app.mode, Mode::Board);
        app.on_key(key('a'));
        assert!(app.on_key(code(KeyCode::Enter)).is_empty());
        assert_eq!(app.mode, Mode::Board);
    }

    #[test]
    fn x_marks_done_only_where_supported() {
        let mut app = loaded_app();
        app.focus = SOZLU;
        app.boxes[SOZLU].selected = 1;
        assert_eq!(
            app.on_key(key('x')),
            vec![Effect::Action {
                idx: SOZLU,
                item: "t2".into(),
                action: "done".into(),
                args: json!({}),
            }]
        );
        app.focus = ZULIP;
        assert!(app.on_key(key('x')).is_empty());
        assert_eq!(app.flash.as_deref(), Some("bu kutuda bitirme yok"));
    }

    #[test]
    fn u_unstars_in_zulip() {
        let mut app = loaded_app();
        app.focus = ZULIP;
        assert_eq!(
            app.on_key(key('u')),
            vec![Effect::Action {
                idx: ZULIP,
                item: "z1".into(),
                action: "unstar".into(),
                args: json!({}),
            }]
        );
    }

    #[test]
    fn o_opens_url_or_flashes() {
        let mut app = loaded_app();
        app.mode = Mode::Zoom(REDMINE);
        app.focus = REDMINE;
        assert_eq!(
            app.on_key(key('o')),
            vec![Effect::Open("https://destek/1".into())]
        );
        app.on_key(code(KeyCode::Down));
        assert!(app.on_key(key('o')).is_empty());
        assert_eq!(app.flash.as_deref(), Some("bu öğenin linki yok"));
    }

    #[test]
    fn enter_zooms_and_requests_full_list_esc_returns() {
        let mut app = loaded_app();
        app.focus = ZULIP;
        assert_eq!(
            app.on_key(code(KeyCode::Enter)),
            vec![Effect::Refresh(ZULIP, View::Full)]
        );
        assert_eq!(app.mode, Mode::Zoom(ZULIP));
        assert!(app.on_key(code(KeyCode::Esc)).is_empty());
        assert_eq!(app.mode, Mode::Board);
    }

    #[test]
    fn space_in_zoom_plans_unplanned_item() {
        let mut app = loaded_app();
        app.focus = REDMINE;
        app.mode = Mode::Zoom(REDMINE);
        let effects = app.on_key(key(' '));
        assert_eq!(
            effects,
            vec![Effect::Drop {
                idx: SOZLU,
                item: with_url(item("1", "İstanbul sunucusu"), "https://destek/1"),
                from: Some(Source {
                    plugin: "redmine".into(),
                    id: "1".into()
                }),
            }]
        );
    }

    #[test]
    fn space_in_zoom_unplans_planned_item() {
        let mut app = loaded_app();
        app.focus = REDMINE;
        app.mode = Mode::Zoom(REDMINE);
        app.boxes[REDMINE].selected = 1;
        assert_eq!(
            app.on_key(key(' ')),
            vec![Effect::Action {
                idx: SOZLU,
                item: "t2".into(),
                action: "delete".into(),
                args: json!({}),
            }]
        );
    }

    #[test]
    fn space_does_nothing_on_board_or_without_plan_into() {
        let mut app = loaded_app();
        app.focus = REDMINE;
        assert!(app.on_key(key(' ')).is_empty());
        app.focus = ZULIP;
        app.mode = Mode::Zoom(ZULIP);
        assert!(app.on_key(key(' ')).is_empty());
    }

    #[test]
    fn move_picker_drops_into_first_eligible_box() {
        let mut app = loaded_app();
        app.focus = ZULIP;
        app.on_key(key('m'));
        assert_eq!(
            app.mode,
            Mode::MovePicker {
                from: ZULIP,
                item: "z1".into()
            }
        );
        let effects = app.on_key(key('1'));
        assert_eq!(
            effects,
            vec![Effect::Drop {
                idx: SOZLU,
                item: item("z1", "zulip mesajı"),
                from: Some(Source {
                    plugin: "zulip".into(),
                    id: "z1".into()
                }),
            }]
        );
        assert_eq!(app.mode, Mode::Board);
    }

    #[test]
    fn move_picker_excludes_source_and_non_accepting_boxes() {
        let mut app = loaded_app();
        app.focus = SOZLU;
        app.on_key(key('m'));
        assert!(app.on_key(key('1')).is_empty());
        assert!(matches!(app.mode, Mode::MovePicker { .. }));
        assert!(app.on_key(code(KeyCode::Esc)).is_empty());
        assert_eq!(app.mode, Mode::Board);
    }

    #[test]
    fn move_picker_excludes_boxes_without_describe() {
        let mut app = App::new(&profile());
        app.on_event(BoxEvent::Described {
            idx: SOZLU,
            describe: describe("vikunja", &[], true),
        });
        list(&mut app, ZULIP, View::Full, vec![item("z1", "m")]);
        app.focus = ZULIP;
        app.on_key(key('m'));
        assert_eq!(app.on_key(key('1')).len(), 1);
        app.focus = REDMINE;
        list(&mut app, REDMINE, View::Full, vec![item("r", "m")]);
        app.mode = Mode::Zoom(REDMINE);
        app.on_key(key('m'));
        assert_eq!(app.on_key(key('2')), vec![]);
    }

    #[test]
    fn tab_wraps_and_question_mark_and_q() {
        let mut app = loaded_app();
        app.on_key(code(KeyCode::Tab));
        assert_eq!(app.focus, 1);
        app.on_key(code(KeyCode::Tab));
        app.on_key(code(KeyCode::Tab));
        assert_eq!(app.focus, 0);
        app.on_key(key('?'));
        assert_eq!(app.mode, Mode::Help);
        app.on_key(code(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Board);
        assert_eq!(app.on_key(key('q')), vec![Effect::Quit]);
    }

    #[test]
    fn navigation_stops_at_bounds_and_handles_empty() {
        let mut app = loaded_app();
        app.focus = ZULIP;
        for _ in 0..5 {
            app.on_key(code(KeyCode::Down));
        }
        assert_eq!(app.boxes[ZULIP].selected, 1);
        for _ in 0..5 {
            app.on_key(code(KeyCode::Up));
        }
        assert_eq!(app.boxes[ZULIP].selected, 0);
        list(&mut app, ZULIP, View::Full, vec![]);
        app.on_key(code(KeyCode::Down));
        app.on_key(code(KeyCode::Up));
        assert_eq!(app.boxes[ZULIP].selected, 0);
        assert!(app.on_key(key('x')).is_empty());
        assert!(app.on_key(key('o')).is_empty());
        assert!(app.on_key(key('m')).is_empty());
    }

    fn hit(col: u16, row: u16) -> Option<Hit> {
        let box_idx = (col / 10) as usize;
        (box_idx < 3).then(|| Hit {
            box_idx,
            row: row.checked_sub(1).map(|r| r as usize),
        })
    }

    fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn down(app: &mut App, col: u16, row: u16, now: Instant) -> Vec<Effect> {
        app.on_mouse(
            mouse(MouseEventKind::Down(MouseButton::Left), col, row),
            now,
            hit,
        )
    }

    fn up(app: &mut App, col: u16, row: u16, now: Instant) -> Vec<Effect> {
        app.on_mouse(
            mouse(MouseEventKind::Up(MouseButton::Left), col, row),
            now,
            hit,
        )
    }

    #[test]
    fn click_focuses_and_selects() {
        let mut app = loaded_app();
        let t = Instant::now();
        assert!(down(&mut app, 21, 2, t).is_empty());
        assert!(up(&mut app, 21, 2, t).is_empty());
        assert_eq!(app.focus, ZULIP);
        assert_eq!(app.boxes[ZULIP].selected, 1);
        assert_eq!(app.mode, Mode::Board);
    }

    #[test]
    fn double_click_within_400ms_zooms() {
        let mut app = loaded_app();
        let t = Instant::now();
        down(&mut app, 21, 2, t);
        up(&mut app, 21, 2, t);
        let effects = down(&mut app, 21, 2, t + Duration::from_millis(400));
        assert_eq!(effects, vec![Effect::Refresh(ZULIP, View::Full)]);
        assert_eq!(app.mode, Mode::Zoom(ZULIP));
    }

    #[test]
    fn slow_second_click_does_not_zoom() {
        let mut app = loaded_app();
        let t = Instant::now();
        down(&mut app, 21, 2, t);
        up(&mut app, 21, 2, t);
        assert!(down(&mut app, 21, 2, t + Duration::from_millis(500)).is_empty());
        assert_eq!(app.mode, Mode::Board);
    }

    #[test]
    fn clicks_on_different_rows_are_not_a_double_click() {
        let mut app = loaded_app();
        let t = Instant::now();
        down(&mut app, 21, 1, t);
        up(&mut app, 21, 1, t);
        down(&mut app, 21, 2, t + Duration::from_millis(100));
        assert_eq!(app.mode, Mode::Board);
    }

    #[test]
    fn drag_to_accepting_box_drops_with_source() {
        let mut app = loaded_app();
        let t = Instant::now();
        down(&mut app, 21, 1, t);
        let effects = up(&mut app, 1, 3, t);
        assert_eq!(
            effects,
            vec![Effect::Drop {
                idx: SOZLU,
                item: item("z1", "zulip mesajı"),
                from: Some(Source {
                    plugin: "zulip".into(),
                    id: "z1".into()
                }),
            }]
        );
    }

    #[test]
    fn drag_inside_box_reorders_to_target_row() {
        let mut app = loaded_app();
        let t = Instant::now();
        down(&mut app, 1, 1, t);
        let effects = up(&mut app, 1, 3, t);
        assert_eq!(
            effects,
            vec![Effect::Action {
                idx: SOZLU,
                item: "t1".into(),
                action: "reorder".into(),
                args: json!({"position": 2}),
            }]
        );
    }

    #[test]
    fn drag_inside_box_without_reorder_does_nothing() {
        let mut app = loaded_app();
        let t = Instant::now();
        down(&mut app, 21, 1, t);
        assert!(up(&mut app, 21, 2, t).is_empty());
    }

    #[test]
    fn drag_to_non_accepting_box_flashes() {
        let mut app = loaded_app();
        let t = Instant::now();
        down(&mut app, 21, 1, t);
        assert!(up(&mut app, 11, 1, t).is_empty());
        assert_eq!(
            app.flash.as_deref(),
            Some("redmine kutusu bırakılan öğe kabul etmiyor")
        );
    }

    #[test]
    fn drag_released_outside_any_box_does_nothing() {
        let mut app = loaded_app();
        let t = Instant::now();
        down(&mut app, 21, 1, t);
        assert!(up(&mut app, 90, 1, t).is_empty());
        assert_eq!(app.flash, None);
    }

    #[test]
    fn wheel_scrolls_hit_box_within_bounds() {
        let mut app = loaded_app();
        let t = Instant::now();
        for _ in 0..6 {
            app.on_mouse(mouse(MouseEventKind::ScrollDown, 21, 2), t, hit);
        }
        assert_eq!(app.boxes[ZULIP].selected, 1);
        assert!(app.boxes[ZULIP].scroll <= 1);
        for _ in 0..6 {
            app.on_mouse(mouse(MouseEventKind::ScrollUp, 21, 2), t, hit);
        }
        assert_eq!(app.boxes[ZULIP].selected, 0);
        assert_eq!(app.boxes[ZULIP].scroll, 0);
        list(&mut app, ZULIP, View::Full, vec![]);
        app.on_mouse(mouse(MouseEventKind::ScrollDown, 21, 2), t, hit);
        assert_eq!(app.boxes[ZULIP].selected, 0);
    }
}

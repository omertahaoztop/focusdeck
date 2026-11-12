use crate::i18n::{msg, t, t_args};
use crate::links::{closures_after_list, linked_ids, linked_item};
use crate::profile::{BoxSpec, Profile};
use crate::worker::BoxEvent;
use chrono::{DateTime, Local};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use focusdeck_protocol::{Describe, Item, Source, View};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;

const DOUBLE_CLICK: Duration = Duration::from_millis(400);

#[derive(Debug, Clone)]
pub struct BoxState {
    pub spec: BoxSpec,
    pub describe: Option<Describe>,
    pub items: Vec<Item>,
    pub error: Option<String>,
    pub selected: usize,
    pub scroll: usize,
    /// Commands in flight, refreshes included.
    pub pending: usize,
    /// Item rows the box showed in the last frame; 0 until one is drawn.
    pub rows: usize,
    empty_lists: u8,
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

#[derive(Debug, Clone, Copy)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hit {
    pub box_idx: usize,
    pub row: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum Effect {
    Refresh(usize),
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

/// What releasing the mouse right now would do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPlan {
    Reorder { idx: usize, row: usize },
    Into(usize),
    Rejected(usize),
    Nothing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Drag {
    pub from: usize,
    pub item: Item,
    pub x: u16,
    pub y: u16,
    pub plan: DropPlan,
}

#[derive(Debug, Clone)]
struct Pressed {
    box_idx: usize,
    row: usize,
    item: String,
}

#[derive(Debug)]
pub struct App {
    pub profile_name: String,
    pub refreshed_at: Option<DateTime<Local>>,
    pub boxes: Vec<BoxState>,
    pub focus: usize,
    pub mode: Mode,
    pub flash: Option<String>,
    /// Target box and text of a capture whose drop is still in flight.
    pub pending_capture: Option<(usize, String)>,
    /// Box and item id armed for deletion by the first space.
    pub confirm: Option<(usize, String)>,
    return_to: Mode,
    filter: String,
    pressed: Option<Pressed>,
    pointer: Option<(u16, u16, Option<Hit>)>,
    last_click: Option<(usize, usize, Instant)>,
}

/// Compares `İ`, `I`, `ı` and `i` equal and ignores a combining dot above, in every UI language.
pub fn fold(s: &str) -> String {
    s.chars()
        .filter(|&c| c != '\u{307}')
        .flat_map(|c| match c {
            'İ' | 'I' | 'ı' | 'i' => 'i'.to_lowercase(),
            c => c.to_lowercase(),
        })
        .collect()
}

fn paste_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut broke = false;
    for c in text.chars() {
        if matches!(c, '\r' | '\n') {
            if !broke {
                out.push(' ');
                broke = true;
            }
        } else if !c.is_control() {
            out.push(c);
            broke = false;
        }
    }
    out
}

fn pop_grapheme(s: &mut String) {
    if let Some((at, _)) = s.grapheme_indices(true).next_back() {
        s.truncate(at);
    }
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
            profile_name: profile.name.clone(),
            refreshed_at: None,
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
                    pending: 0,
                    rows: 0,
                    empty_lists: 0,
                })
                .collect(),
            focus: 0,
            mode: Mode::Board,
            flash: None,
            pending_capture: None,
            confirm: None,
            return_to: Mode::Board,
            filter: String::new(),
            pressed: None,
            pointer: None,
            last_click: None,
        }
    }

    /// Present only once the pointer has left the pressed cell, so a plain click never shows it.
    pub fn drag(&self) -> Option<Drag> {
        let (x, y, over) = self.pointer?;
        let pressed = self.pressed.as_ref()?;
        let item = self
            .visible(pressed.box_idx)
            .into_iter()
            .find(|i| i.id == pressed.item)?
            .clone();
        Some(Drag {
            from: pressed.box_idx,
            item,
            x,
            y,
            plan: self.drop_plan(pressed, over),
        })
    }

    fn drop_plan(&self, from: &Pressed, over: Option<Hit>) -> DropPlan {
        let Some(hit) = over else {
            return DropPlan::Nothing;
        };
        if hit.box_idx == from.box_idx {
            if !has_action(&self.boxes[hit.box_idx], "reorder") {
                return DropPlan::Nothing;
            }
            let visible = self.visible(hit.box_idx);
            let Some(from_row) = visible.iter().position(|i| i.id == from.item) else {
                return DropPlan::Nothing;
            };
            let last = visible.len().saturating_sub(1);
            return match hit.row.map(|r| r.min(last)) {
                Some(row) if row != from_row => DropPlan::Reorder {
                    idx: hit.box_idx,
                    row,
                },
                _ => DropPlan::Nothing,
            };
        }
        let accepts = self.boxes[hit.box_idx]
            .describe
            .as_ref()
            .is_some_and(|d| d.accepts_drop);
        if accepts {
            DropPlan::Into(hit.box_idx)
        } else {
            DropPlan::Rejected(hit.box_idx)
        }
    }

    fn plan_target(&self, idx: usize) -> Option<usize> {
        let target = self.boxes.get(idx)?.spec.plan_into.as_deref()?;
        self.boxes.iter().position(|b| b.spec.id == target)
    }

    fn query_for(&self, idx: usize) -> Option<&str> {
        match &self.mode {
            Mode::Search { zoom, query } if *zoom == idx => Some(query.as_str()),
            _ if self.zoomed_box() == Some(idx) => Some(self.filter.as_str()),
            _ => None,
        }
    }

    /// The search text narrowing box `idx` right now; `None` when it is empty.
    pub fn active_filter(&self, idx: usize) -> Option<&str> {
        self.query_for(idx).filter(|q| !q.is_empty())
    }

    pub fn visible(&self, idx: usize) -> Vec<&Item> {
        let Some(state) = self.boxes.get(idx) else {
            return Vec::new();
        };
        match self.query_for(idx) {
            Some(query) => {
                let needle = fold(query);
                state
                    .items
                    .iter()
                    .filter(|i| needle.is_empty() || fold(&i.title).contains(&needle))
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

    fn reorder_effect(
        &self,
        idx: usize,
        item: &str,
        from_row: usize,
        row: usize,
    ) -> Option<Effect> {
        let target = self.visible(idx).get(row)?.id.clone();
        let at = self.boxes[idx]
            .items
            .iter()
            .filter(|i| i.id != item)
            .position(|i| i.id == target)?;
        let position = if row > from_row { at + 1 } else { at };
        Some(plain_action(
            idx,
            item,
            "reorder",
            json!({ "position": position }),
        ))
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

    fn step_or_leave(&mut self, idx: usize, down: bool) {
        let last = self.visible(idx).len().saturating_sub(1);
        let selected = self.boxes[idx].selected;
        let at_edge = if down {
            selected >= last
        } else {
            selected == 0
        };
        let dir = if down { Dir::Down } else { Dir::Up };
        match self.neighbor(idx, dir).filter(|_| at_edge) {
            Some(next) => self.focus = next,
            None => self.step(idx, down),
        }
    }

    fn neighbor(&self, idx: usize, dir: Dir) -> Option<usize> {
        let vertical = matches!(dir, Dir::Up | Dir::Down);
        let span = |b: &BoxSpec, rows: bool| {
            let (start, len) = if rows {
                (b.row, b.height)
            } else {
                (b.col, b.width)
            };
            (u32::from(start), u32::from(start) + u32::from(len))
        };
        let from = &self.boxes.get(idx)?.spec;
        let (lo, hi) = span(from, vertical);
        let (cross_lo, cross_hi) = span(from, !vertical);
        self.boxes
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != idx)
            .filter_map(|(i, b)| {
                let (b_lo, b_hi) = span(&b.spec, vertical);
                let (b_cross_lo, b_cross_hi) = span(&b.spec, !vertical);
                if b_cross_lo >= cross_hi || b_cross_hi <= cross_lo {
                    return None;
                }
                let gap = match dir {
                    Dir::Right | Dir::Down => b_lo.checked_sub(hi)?,
                    Dir::Left | Dir::Up => lo.checked_sub(b_hi)?,
                };
                Some(((gap, b_cross_lo.abs_diff(cross_lo)), i))
            })
            .min()
            .map(|(_, i)| i)
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
        vec![Effect::Refresh(idx)]
    }

    fn selection_ids(&self) -> Vec<Option<String>> {
        (0..self.boxes.len())
            .map(|idx| self.selected_item(idx).map(|i| i.id))
            .collect()
    }

    fn restore_selection(&mut self, ids: Vec<Option<String>>) {
        for (idx, id) in ids.into_iter().enumerate() {
            let row = id.and_then(|id| self.visible(idx).iter().position(|i| i.id == id));
            if let Some(row) = row {
                self.boxes[idx].selected = row;
            }
            self.clamp(idx);
        }
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
                let Some(state) = self.boxes.get_mut(idx) else {
                    return Vec::new();
                };
                state.pending = state.pending.saturating_sub(1);
                match result {
                    Ok(items) => {
                        let effects = self.link_closures(idx, view, &items);
                        let keep = self.selection_ids();
                        let state = &mut self.boxes[idx];
                        state.items = items;
                        state.error = None;
                        self.refreshed_at = Some(Local::now());
                        self.restore_selection(keep);
                        if let Some((target, id)) = &self.confirm
                            && !self.boxes[*target].items.iter().any(|i| i.id == *id)
                        {
                            self.confirm = None;
                        }
                        effects
                    }
                    Err(message) => {
                        let state = &mut self.boxes[idx];
                        state.error = Some(message);
                        state.empty_lists = 0;
                        Vec::new()
                    }
                }
            }
            BoxEvent::Finished { idx, what, result } => {
                let Some(state) = self.boxes.get_mut(idx) else {
                    return Vec::new();
                };
                state.pending = state.pending.saturating_sub(1);
                let capture = self
                    .pending_capture
                    .take_if(|(target, _)| *target == idx && what == "drop");
                match result {
                    Ok(()) => vec![Effect::Refresh(idx)],
                    Err(message) => {
                        self.flash = Some(t_args(
                            msg::FLASH_ACTION_FAILED,
                            &[
                                ("box", self.boxes[idx].spec.title.as_str().into()),
                                ("what", what.as_str().into()),
                                ("message", message.as_str().into()),
                            ],
                        ));
                        if let Some((_, text)) = capture {
                            self.reopen_capture(&text);
                        }
                        Vec::new()
                    }
                }
            }
        }
    }

    fn link_closures(&mut self, idx: usize, view: View, items: &[Item]) -> Vec<Effect> {
        if view != View::Full {
            return Vec::new();
        }
        let Some(target) = self.plan_target(idx) else {
            return Vec::new();
        };
        let mut empty_lists = self.boxes[idx].empty_lists;
        let closing = closures_after_list(
            &mut empty_lists,
            &self.boxes[idx].spec.plugin,
            items,
            &self.boxes[target].items,
        );
        self.boxes[idx].empty_lists = empty_lists;
        closing
            .into_iter()
            .map(|id| plain_action(target, &id, "done", json!({})))
            .collect()
    }

    pub fn on_paste(&mut self, text: &str) -> Vec<Effect> {
        let text = paste_text(text);
        match &mut self.mode {
            Mode::Capture { input } => input.push_str(&text),
            Mode::Search { zoom, query } => {
                query.push_str(&text);
                let zoom = *zoom;
                self.boxes[zoom].selected = 0;
                self.clamp(zoom);
            }
            _ => {}
        }
        Vec::new()
    }

    pub fn reopen_capture(&mut self, text: &str) {
        match &mut self.mode {
            Mode::Capture { input } if input.is_empty() => *input = text.to_owned(),
            Mode::Capture { input } => *input = format!("{text} {input}"),
            _ => self.enter_mode(Mode::Capture {
                input: text.to_owned(),
            }),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Vec<Effect> {
        if key.kind == KeyEventKind::Release {
            return Vec::new();
        }
        self.flash = None;
        if key.code != KeyCode::Char(' ') {
            self.confirm = None;
        }
        if key.code == KeyCode::Esc && self.pointer.is_some() {
            self.pressed = None;
            self.pointer = None;
            return Vec::new();
        }
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
        match key.code {
            KeyCode::Char('q') => return vec![Effect::Quit],
            KeyCode::Char('?') => {
                self.enter_mode(Mode::Help);
                return Vec::new();
            }
            _ => {}
        }
        if idx >= self.boxes.len() {
            return Vec::new();
        }
        match key.code {
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
            KeyCode::BackTab if !zoomed => {
                self.focus = (idx + self.boxes.len() - 1) % self.boxes.len();
                Vec::new()
            }
            KeyCode::Left | KeyCode::Right if !zoomed => {
                let dir = if key.code == KeyCode::Left {
                    Dir::Left
                } else {
                    Dir::Right
                };
                if let Some(next) = self.neighbor(idx, dir) {
                    self.focus = next;
                }
                Vec::new()
            }
            KeyCode::Down | KeyCode::Up
                if zoomed && key.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                self.move_selected(idx, key.code == KeyCode::Down)
            }
            KeyCode::Down | KeyCode::Up if zoomed => {
                self.step(idx, key.code == KeyCode::Down);
                Vec::new()
            }
            KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End if zoomed => {
                self.jump(idx, key.code);
                Vec::new()
            }
            KeyCode::Down | KeyCode::Up => {
                self.step_or_leave(idx, key.code == KeyCode::Down);
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
            KeyCode::Char('x') => self.require_action(idx, "done", msg::FLASH_NO_DONE),
            KeyCode::Char('u') => self.require_action(idx, "unstar", msg::FLASH_NO_UNSTAR),
            KeyCode::Char('o') => self.open_selected(idx),
            KeyCode::Char('m') => self.open_picker(idx),
            KeyCode::Char(' ') if zoomed => self.toggle_plan(idx),
            _ => Vec::new(),
        }
    }

    fn require_action(&mut self, idx: usize, action: &str, missing_msg: &str) -> Vec<Effect> {
        if !has_action(&self.boxes[idx], action) {
            self.flash = Some(t(missing_msg));
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
                self.flash = Some(t(msg::FLASH_NO_LINK));
                Vec::new()
            }
        }
    }

    fn toggle_plan(&mut self, idx: usize) -> Vec<Effect> {
        let (Some(target), Some(item)) = (self.plan_target(idx), self.selected_item(idx)) else {
            return Vec::new();
        };
        if self.boxes[target].pending > 0 {
            return Vec::new();
        }
        let spec = &self.boxes[idx].spec;
        let planned = linked_item(&self.boxes[target].items, &spec.plugin, &item.id)
            .map(|task| (task.id.clone(), task.title.clone()));
        match planned {
            Some((id, title)) => {
                let armed = (target, id);
                if self.confirm.as_ref() == Some(&armed) {
                    self.confirm = None;
                    vec![plain_action(target, &armed.1, "delete", json!({}))]
                } else {
                    self.flash = Some(t_args(
                        msg::FLASH_CONFIRM_UNPLAN,
                        &[
                            ("title", title.as_str().into()),
                            ("box", self.boxes[target].spec.title.as_str().into()),
                        ],
                    ));
                    self.confirm = Some(armed);
                    Vec::new()
                }
            }
            None => vec![Effect::Drop {
                idx: target,
                from: Some(source_of(spec, &item)),
                item,
            }],
        }
    }

    fn open_picker(&mut self, idx: usize) -> Vec<Effect> {
        let Some(item) = self.selected_item(idx) else {
            return Vec::new();
        };
        if self.picker_targets(idx).is_empty() {
            self.flash = Some(t(msg::FLASH_NO_MOVE_TARGET));
            return Vec::new();
        }
        self.enter_mode(Mode::MovePicker {
            from: idx,
            item: item.id,
        });
        Vec::new()
    }

    fn move_selected(&mut self, idx: usize, down: bool) -> Vec<Effect> {
        if !has_action(&self.boxes[idx], "reorder") {
            return Vec::new();
        }
        let Some(item) = self.selected_item(idx) else {
            return Vec::new();
        };
        let from = self.boxes[idx].selected;
        let to = if down {
            from + 1
        } else {
            let Some(to) = from.checked_sub(1) else {
                return Vec::new();
            };
            to
        };
        if to >= self.visible(idx).len() {
            return Vec::new();
        }
        self.reorder_effect(idx, &item.id, from, to)
            .into_iter()
            .collect()
    }

    fn jump(&mut self, idx: usize, code: KeyCode) {
        let last = self.visible(idx).len().saturating_sub(1);
        let state = &mut self.boxes[idx];
        let page = if state.rows == 0 { 10 } else { state.rows };
        state.selected = match code {
            KeyCode::PageUp => state.selected.saturating_sub(page),
            KeyCode::PageDown => (state.selected + page).min(last),
            KeyCode::Home => 0,
            _ => last,
        };
        state.scroll = state.scroll.min(state.selected);
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
                pop_grapheme(&mut input);
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

    pub fn picker_targets(&self, from: usize) -> Vec<usize> {
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
                pop_grapheme(&mut query);
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
            MouseEventKind::Drag(MouseButton::Left) => {
                self.on_drag(ev.column, ev.row, target);
                Vec::new()
            }
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

    fn on_drag(&mut self, x: u16, y: u16, target: Option<Hit>) {
        let Some(pressed) = self.pressed.as_ref() else {
            return;
        };
        let on_pressed_cell =
            target.is_some_and(|h| h.box_idx == pressed.box_idx && h.row == Some(pressed.row));
        if self.pointer.is_some() || !on_pressed_cell {
            self.pointer = Some((x, y, target));
        }
    }

    fn on_press(&mut self, target: Option<Hit>, now: Instant) -> Vec<Effect> {
        self.flash = None;
        self.pressed = None;
        self.pointer = None;
        let Some(hit) = target else {
            return Vec::new();
        };
        self.focus = hit.box_idx;
        let Some(row) = hit.row else {
            self.last_click = None;
            return Vec::new();
        };
        let Some(id) = self.visible(hit.box_idx).get(row).map(|i| i.id.clone()) else {
            self.last_click = None;
            return Vec::new();
        };
        self.boxes[hit.box_idx].selected = row;
        self.pressed = Some(Pressed {
            box_idx: hit.box_idx,
            row,
            item: id,
        });
        let double = self.last_click.is_some_and(|(box_idx, prev_row, at)| {
            box_idx == hit.box_idx
                && prev_row == row
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
        self.last_click = Some((hit.box_idx, row, now));
        Vec::new()
    }

    fn on_release(&mut self, target: Option<Hit>) -> Vec<Effect> {
        self.pointer = None;
        let Some(from) = self.pressed.take() else {
            return Vec::new();
        };
        let Some((from_row, item)) = self
            .visible(from.box_idx)
            .into_iter()
            .enumerate()
            .find(|(_, i)| i.id == from.item)
            .map(|(row, i)| (row, i.clone()))
        else {
            return Vec::new();
        };
        if target.is_some_and(|h| h.box_idx != from.box_idx || h.row != Some(from.row)) {
            self.last_click = None;
        }
        match self.drop_plan(&from, target) {
            DropPlan::Reorder { idx, row } => self
                .reorder_effect(idx, &item.id, from_row, row)
                .into_iter()
                .collect(),
            DropPlan::Into(idx) => vec![Effect::Drop {
                idx,
                from: Some(source_of(&self.boxes[from.box_idx].spec, &item)),
                item,
            }],
            DropPlan::Rejected(idx) => {
                self.flash = Some(t_args(
                    msg::FLASH_DROP_REJECTED,
                    &[("title", self.boxes[idx].spec.title.as_str().into())],
                ));
                Vec::new()
            }
            DropPlan::Nothing => Vec::new(),
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

    pub fn spec(id: &str, plugin: &str, plan_into: Option<&str>) -> BoxSpec {
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
                linked("t2", "Depo hazırlığı", "redmine", "2"),
                item("t3", "üçüncü görev"),
            ],
        );
        list(
            &mut app,
            REDMINE,
            View::Full,
            vec![
                with_url(item("1", "İstanbul sunucusu"), "https://destek/1"),
                item("2", "DEPO hazırlığı"),
                item("3", "IŞIK kurulumu"),
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
    use crate::profile::{BoxSpec, Profile};
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
    fn search_is_case_insensitive_across_turkish_and_plain_letters() {
        let mut app = loaded_app();
        for (query, expected) in [
            ("depo", "2"),
            ("istanbul", "1"),
            ("ışık", "3"),
            ("İSTANBUL", "1"),
            ("IŞIK", "3"),
        ] {
            app.mode = Mode::Search {
                zoom: REDMINE,
                query: query.into(),
            };
            assert_eq!(ids(&app, REDMINE), vec![expected], "query {query}");
        }
    }

    #[test]
    fn search_is_plain_case_insensitive_in_english() {
        let mut app = loaded_app();
        app.boxes[REDMINE].items = vec![item("1", "Invoice due"), item("2", "invoice paid")];
        for query in ["invoice", "INVOICE", "Invoice"] {
            app.mode = Mode::Search {
                zoom: REDMINE,
                query: query.into(),
            };
            assert_eq!(ids(&app, REDMINE), vec!["1", "2"], "query {query}");
        }
    }

    #[test]
    fn search_typing_enter_and_esc() {
        let mut app = loaded_app();
        app.focus = REDMINE;
        app.on_key(code(KeyCode::Enter));
        assert_eq!(app.mode, Mode::Zoom(REDMINE));
        app.on_key(key('/'));
        for c in "dep".chars() {
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
        assert_eq!(effects, vec![Effect::Refresh(SOZLU)]);
        app.mode = Mode::Zoom(SOZLU);
        let effects = app.on_event(BoxEvent::Finished {
            idx: SOZLU,
            what: "done".into(),
            result: Ok(()),
        });
        assert_eq!(effects, vec![Effect::Refresh(SOZLU)]);
        let effects = app.on_event(BoxEvent::Finished {
            idx: SOZLU,
            what: "done".into(),
            result: Err("sunucu hatası".into()),
        });
        assert!(effects.is_empty());
        assert_eq!(
            app.flash.as_deref(),
            Some("sozlu: done failed: sunucu hatası")
        );
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
        assert_eq!(app.flash.as_deref(), Some("this box has no done action"));
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
        assert_eq!(app.flash.as_deref(), Some("this item has no link"));
    }

    #[test]
    fn enter_zooms_and_requests_full_list_esc_returns() {
        let mut app = loaded_app();
        app.focus = ZULIP;
        assert_eq!(
            app.on_key(code(KeyCode::Enter)),
            vec![Effect::Refresh(ZULIP)]
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
        let mut app = plan_view();
        app.on_key(key(' '));
        assert_eq!(app.on_key(key(' ')), vec![delete_t2()]);
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
    fn move_picker_excludes_the_source_box() {
        let mut app = loaded_app();
        app.focus = ZULIP;
        app.on_key(key('m'));
        assert!(app.on_key(key('2')).is_empty());
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

    fn grid_app(cells: &[(u16, u16, u16)]) -> App {
        let boxes = cells
            .iter()
            .enumerate()
            .map(|(i, &(row, col, width))| BoxSpec {
                row,
                col,
                width,
                ..spec(&format!("b{i}"), "vikunja", None)
            })
            .collect();
        App::new(&Profile { boxes, ..profile() })
    }

    const TWO_BY_THREE: [(u16, u16, u16); 6] = [
        (0, 0, 1),
        (0, 1, 1),
        (0, 2, 1),
        (1, 0, 1),
        (1, 1, 1),
        (1, 2, 1),
    ];

    #[test]
    fn left_right_switch_columns_without_wrapping() {
        let mut app = grid_app(&TWO_BY_THREE);
        app.on_key(code(KeyCode::Right));
        assert_eq!(app.focus, 1);
        app.on_key(code(KeyCode::Right));
        app.on_key(code(KeyCode::Right));
        assert_eq!(app.focus, 2);
        app.on_key(code(KeyCode::Left));
        assert_eq!(app.focus, 1);
        app.focus = 3;
        app.on_key(code(KeyCode::Left));
        assert_eq!(app.focus, 3);
    }

    #[test]
    fn up_down_leave_the_box_only_at_its_edges() {
        let mut app = grid_app(&TWO_BY_THREE);
        list(
            &mut app,
            0,
            View::Full,
            vec![item("a", "a"), item("b", "b")],
        );
        list(&mut app, 3, View::Full, vec![item("c", "c")]);
        app.on_key(code(KeyCode::Down));
        assert_eq!((app.focus, app.boxes[0].selected), (0, 1));
        app.on_key(code(KeyCode::Down));
        assert_eq!(app.focus, 3);
        app.on_key(code(KeyCode::Down));
        assert_eq!(app.focus, 3);
        app.on_key(code(KeyCode::Up));
        assert_eq!((app.focus, app.boxes[0].selected), (0, 1));
        app.on_key(code(KeyCode::Up));
        app.on_key(code(KeyCode::Up));
        assert_eq!((app.focus, app.boxes[0].selected), (0, 0));
        app.focus = 1;
        app.on_key(code(KeyCode::Down));
        assert_eq!(app.focus, 4);
    }

    #[test]
    fn arrows_pick_the_nearest_overlapping_box_under_a_wide_one() {
        let mut app = grid_app(&[(0, 0, 2), (1, 0, 1), (1, 1, 1)]);
        app.on_key(code(KeyCode::Down));
        assert_eq!(app.focus, 1);
        app.focus = 2;
        app.on_key(code(KeyCode::Up));
        assert_eq!(app.focus, 0);
    }

    #[test]
    fn arrows_stay_inside_a_zoomed_box() {
        let mut app = grid_app(&TWO_BY_THREE);
        list(&mut app, 0, View::Full, vec![item("a", "a")]);
        app.mode = Mode::Zoom(0);
        app.on_key(code(KeyCode::Down));
        app.on_key(code(KeyCode::Right));
        assert_eq!(app.focus, 0);
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

    fn drag_to(app: &mut App, col: u16, row: u16, now: Instant) {
        app.on_mouse(
            mouse(MouseEventKind::Drag(MouseButton::Left), col, row),
            now,
            hit,
        );
    }

    #[test]
    fn drag_state_appears_only_after_leaving_the_pressed_cell() {
        let mut app = loaded_app();
        let t = Instant::now();
        assert_eq!(app.drag(), None);
        down(&mut app, 21, 1, t);
        drag_to(&mut app, 22, 1, t);
        assert_eq!(app.drag(), None);
        drag_to(&mut app, 3, 3, t);
        assert_eq!(
            app.drag(),
            Some(Drag {
                from: ZULIP,
                item: item("z1", "zulip mesajı"),
                x: 3,
                y: 3,
                plan: DropPlan::Into(SOZLU),
            })
        );
        drag_to(&mut app, 22, 1, t);
        assert!(app.drag().is_some());
        up(&mut app, 3, 3, t);
        assert_eq!(app.drag(), None);
    }

    #[test]
    fn escape_cancels_a_drag_without_dropping() {
        let mut app = loaded_app();
        let t = Instant::now();
        down(&mut app, 21, 1, t);
        drag_to(&mut app, 3, 3, t);
        assert!(app.on_key(code(KeyCode::Esc)).is_empty());
        assert_eq!(app.drag(), None);
        assert!(up(&mut app, 3, 3, t).is_empty());
        assert_eq!(app.mode, Mode::Board);
    }

    #[test]
    fn drag_without_press_is_ignored() {
        let mut app = loaded_app();
        drag_to(&mut app, 3, 3, Instant::now());
        assert_eq!(app.drag(), None);
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
        assert_eq!(effects, vec![Effect::Refresh(ZULIP)]);
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
            Some("redmine box does not accept the dropped item")
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
    fn drag_plan_follows_the_box_under_the_pointer() {
        let mut app = loaded_app();
        let t = Instant::now();
        down(&mut app, 21, 1, t);
        drag_to(&mut app, 1, 2, t);
        assert_eq!(app.drag().unwrap().plan, DropPlan::Into(SOZLU));
        drag_to(&mut app, 11, 2, t);
        assert_eq!(app.drag().unwrap().plan, DropPlan::Rejected(REDMINE));
        drag_to(&mut app, 21, 2, t);
        assert_eq!(app.drag().unwrap().plan, DropPlan::Nothing);
        drag_to(&mut app, 90, 2, t);
        assert_eq!(app.drag().unwrap().plan, DropPlan::Nothing);
    }

    #[test]
    fn reorder_past_the_last_item_lands_on_the_last_row() {
        let mut app = loaded_app();
        let t = Instant::now();
        down(&mut app, 1, 1, t);
        drag_to(&mut app, 1, 9, t);
        assert_eq!(
            app.drag().unwrap().plan,
            DropPlan::Reorder { idx: SOZLU, row: 2 }
        );
        assert_eq!(
            up(&mut app, 1, 9, t),
            vec![plain_action(SOZLU, "t1", "reorder", json!({"position": 2}))]
        );
    }

    #[test]
    fn wheel_scrolls_hit_box_within_bounds() {
        let mut app = loaded_app();
        let t = Instant::now();
        for _ in 0..6 {
            app.on_mouse(mouse(MouseEventKind::ScrollDown, 21, 2), t, hit);
        }
        assert_eq!(app.boxes[ZULIP].selected, 1);
        assert_eq!(app.boxes[ZULIP].scroll, 1);
        for _ in 0..6 {
            app.on_mouse(mouse(MouseEventKind::ScrollUp, 21, 2), t, hit);
        }
        assert_eq!(app.boxes[ZULIP].selected, 0);
        assert_eq!(app.boxes[ZULIP].scroll, 0);
        list(&mut app, ZULIP, View::Full, vec![]);
        app.on_mouse(mouse(MouseEventKind::ScrollDown, 21, 2), t, hit);
        assert_eq!(app.boxes[ZULIP].selected, 0);
    }

    fn paste_into_capture(app: &mut App, text: &str) {
        app.on_key(key('a'));
        app.on_paste(text);
    }

    #[test]
    fn paste_in_capture_keeps_all_lines_as_text() {
        let mut app = loaded_app();
        paste_into_capture(&mut app, "Call vendor\r\nreview\n\nquote\u{7}\t!");
        assert_eq!(
            app.on_key(code(KeyCode::Enter)),
            vec![Effect::Capture("Call vendor review quote!".into())]
        );
    }

    #[test]
    fn paste_in_search_narrows_the_list() {
        let mut app = loaded_app();
        app.focus = REDMINE;
        app.on_key(code(KeyCode::Enter));
        app.on_key(key('/'));
        app.boxes[REDMINE].selected = 2;
        assert!(app.on_paste("dep").is_empty());
        assert_eq!(ids(&app, REDMINE), vec!["2"]);
        assert_eq!(app.boxes[REDMINE].selected, 0);
        assert_eq!(app.active_filter(REDMINE), Some("dep"));
    }

    #[test]
    fn paste_on_the_board_runs_nothing() {
        let mut app = loaded_app();
        let before: Vec<_> = app.boxes.iter().map(|b| b.items.clone()).collect();
        assert!(app.on_paste("x\nq\na\n ").is_empty());
        assert_eq!(app.mode, Mode::Board);
        let after: Vec<_> = app.boxes.iter().map(|b| b.items.clone()).collect();
        assert_eq!(before, after);
        app.mode = Mode::Zoom(ZULIP);
        assert!(app.on_paste("x\nq").is_empty());
        assert_eq!(app.mode, Mode::Zoom(ZULIP));
    }

    fn finished(idx: usize, what: &str, result: Result<(), String>) -> BoxEvent {
        BoxEvent::Finished {
            idx,
            what: what.into(),
            result,
        }
    }

    #[test]
    fn failed_drop_reopens_capture_with_the_text() {
        let mut app = loaded_app();
        app.pending_capture = Some((SOZLU, "süt al".into()));
        app.on_event(finished(ZULIP, "drop", Err("x".into())));
        assert_eq!(app.mode, Mode::Board);
        assert!(app.pending_capture.is_some());
        app.on_event(finished(SOZLU, "drop", Err("offline".into())));
        assert_eq!(
            app.mode,
            Mode::Capture {
                input: "süt al".into()
            }
        );
        assert_eq!(app.pending_capture, None);
        assert_eq!(app.flash.as_deref(), Some("sozlu: drop failed: offline"));
        app.on_key(code(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Board);
    }

    #[test]
    fn successful_drop_clears_the_pending_capture() {
        let mut app = loaded_app();
        app.pending_capture = Some((SOZLU, "süt al".into()));
        app.on_event(finished(SOZLU, "drop", Ok(())));
        assert_eq!(app.pending_capture, None);
        assert_eq!(app.mode, Mode::Board);
    }

    #[test]
    fn reopened_capture_returns_to_the_zoom_it_came_from() {
        let mut app = loaded_app();
        app.mode = Mode::Zoom(SOZLU);
        app.focus = SOZLU;
        app.reopen_capture("not");
        app.on_key(code(KeyCode::Esc));
        assert_eq!(app.mode, Mode::Zoom(SOZLU));
    }

    fn plan_view() -> App {
        let mut app = loaded_app();
        app.focus = REDMINE;
        app.mode = Mode::Zoom(REDMINE);
        app.boxes[REDMINE].selected = 1;
        app
    }

    fn delete_t2() -> Effect {
        plain_action(SOZLU, "t2", "delete", json!({}))
    }

    #[test]
    fn first_space_on_a_planned_item_only_asks() {
        let mut app = plan_view();
        assert!(app.on_key(key(' ')).is_empty());
        assert_eq!(app.confirm, Some((SOZLU, "t2".into())));
        assert_eq!(
            app.flash.as_deref(),
            Some("space again deletes \"Depo hazırlığı\" from sozlu")
        );
    }

    #[test]
    fn second_space_deletes() {
        let mut app = plan_view();
        app.on_key(key(' '));
        assert_eq!(app.on_key(key(' ')), vec![delete_t2()]);
        assert_eq!(app.confirm, None);
    }

    #[test]
    fn another_key_cancels_the_delete() {
        let mut app = plan_view();
        app.on_key(key(' '));
        app.on_key(code(KeyCode::Down));
        assert_eq!(app.confirm, None);
        app.on_key(code(KeyCode::Up));
        assert!(app.on_key(key(' ')).is_empty());
        assert_eq!(app.confirm, Some((SOZLU, "t2".into())));
    }

    #[test]
    fn an_event_that_removes_the_item_cancels_the_delete() {
        let mut app = plan_view();
        app.on_key(key(' '));
        list(
            &mut app,
            SOZLU,
            View::Full,
            vec![item("t1", "birinci görev")],
        );
        assert_eq!(app.confirm, None);
    }

    #[test]
    fn refresh_keeps_the_selection_on_the_same_item() {
        let mut app = loaded_app();
        app.focus = ZULIP;
        app.boxes[ZULIP].selected = 1;
        list(
            &mut app,
            ZULIP,
            View::Full,
            vec![
                item("z0", "yeni"),
                item("z1", "zulip mesajı"),
                item("z2", "ikinci mesaj"),
            ],
        );
        assert_eq!(app.boxes[ZULIP].selected, 2);
        assert_eq!(app.selected_item(ZULIP).unwrap().id, "z2");
        list(&mut app, ZULIP, View::Full, vec![item("z0", "yeni")]);
        assert_eq!(app.boxes[ZULIP].selected, 0);
    }

    #[test]
    fn refresh_during_drag_drops_the_item_that_was_grabbed() {
        let mut app = loaded_app();
        let t = Instant::now();
        down(&mut app, 21, 1, t);
        drag_to(&mut app, 3, 3, t);
        list(
            &mut app,
            ZULIP,
            View::Summary,
            vec![item("z0", "yeni mesaj"), item("z1", "zulip mesajı")],
        );
        assert_eq!(
            up(&mut app, 1, 3, t),
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
    fn release_after_the_grabbed_item_vanished_does_nothing() {
        let mut app = loaded_app();
        let t = Instant::now();
        down(&mut app, 21, 1, t);
        list(&mut app, ZULIP, View::Summary, vec![item("z9", "başka")]);
        assert!(up(&mut app, 1, 3, t).is_empty());
    }

    fn filtered_sozlu() -> App {
        let mut app = loaded_app();
        app.boxes[SOZLU].items = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|id| {
                let title = if "ace".contains(id) {
                    "deploy"
                } else {
                    "plain"
                };
                item(id, &format!("{title} {id}"))
            })
            .collect();
        app.mode = Mode::Zoom(SOZLU);
        app.focus = SOZLU;
        app.filter = "deploy".into();
        app
    }

    #[test]
    fn reorder_in_a_filtered_list_sends_the_full_list_position() {
        let mut app = filtered_sozlu();
        assert_eq!(ids(&app, SOZLU), vec!["a", "c", "e"]);
        let t = Instant::now();
        down(&mut app, 1, 3, t);
        assert_eq!(
            up(&mut app, 1, 2, t),
            vec![plain_action(SOZLU, "e", "reorder", json!({"position": 2}))]
        );
        down(&mut app, 1, 1, t);
        assert_eq!(
            up(&mut app, 1, 3, t),
            vec![plain_action(SOZLU, "a", "reorder", json!({"position": 4}))]
        );
    }

    fn shift(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::SHIFT)
    }

    #[test]
    fn shift_arrows_reorder_the_selected_item() {
        let mut app = loaded_app();
        app.focus = SOZLU;
        app.mode = Mode::Zoom(SOZLU);
        assert!(app.on_key(shift(KeyCode::Up)).is_empty());
        assert_eq!(
            app.on_key(shift(KeyCode::Down)),
            vec![plain_action(SOZLU, "t1", "reorder", json!({"position": 1}))]
        );
        list(
            &mut app,
            SOZLU,
            View::Full,
            vec![
                linked("t2", "Depo hazırlığı", "redmine", "2"),
                item("t1", "birinci görev"),
                item("t3", "üçüncü görev"),
            ],
        );
        assert_eq!(app.selected_item(SOZLU).unwrap().id, "t1");
        assert_eq!(
            app.on_key(shift(KeyCode::Up)),
            vec![plain_action(SOZLU, "t1", "reorder", json!({"position": 0}))]
        );
        app.boxes[SOZLU].selected = 2;
        assert!(app.on_key(shift(KeyCode::Down)).is_empty());
        app.focus = ZULIP;
        app.mode = Mode::Zoom(ZULIP);
        assert!(app.on_key(shift(KeyCode::Down)).is_empty());
    }

    fn long_zoom() -> App {
        let mut app = loaded_app();
        app.focus = ZULIP;
        app.mode = Mode::Zoom(ZULIP);
        let items = (0..30).map(|i| item(&format!("n{i}"), "öğe")).collect();
        list(&mut app, ZULIP, View::Full, items);
        app
    }

    #[test]
    fn page_keys_move_by_a_page() {
        let mut app = long_zoom();
        app.on_key(code(KeyCode::PageDown));
        assert_eq!(app.boxes[ZULIP].selected, 10);
        app.boxes[ZULIP].rows = 4;
        app.on_key(code(KeyCode::PageDown));
        assert_eq!(app.boxes[ZULIP].selected, 14);
        app.on_key(code(KeyCode::PageUp));
        app.on_key(code(KeyCode::PageUp));
        app.on_key(code(KeyCode::PageUp));
        app.on_key(code(KeyCode::PageUp));
        assert_eq!(app.boxes[ZULIP].selected, 0);
        app.boxes[ZULIP].selected = 28;
        app.on_key(code(KeyCode::PageDown));
        assert_eq!(app.boxes[ZULIP].selected, 29);
    }

    #[test]
    fn home_and_end_jump_to_the_ends() {
        let mut app = long_zoom();
        app.on_key(code(KeyCode::End));
        assert_eq!(app.boxes[ZULIP].selected, 29);
        app.on_key(code(KeyCode::Home));
        assert_eq!(app.boxes[ZULIP].selected, 0);
    }

    #[test]
    fn backtab_goes_to_the_previous_box() {
        let mut app = loaded_app();
        app.on_key(code(KeyCode::BackTab));
        assert_eq!(app.focus, ZULIP);
        app.on_key(code(KeyCode::BackTab));
        assert_eq!(app.focus, REDMINE);
        app.mode = Mode::Zoom(REDMINE);
        app.on_key(code(KeyCode::BackTab));
        assert_eq!(app.focus, REDMINE);
    }

    #[test]
    fn m_without_a_target_flashes_and_opens_nothing() {
        let mut app = loaded_app();
        app.focus = SOZLU;
        assert!(app.on_key(key('m')).is_empty());
        assert_eq!(app.mode, Mode::Board);
        assert_eq!(
            app.flash.as_deref(),
            Some("no box here accepts moved items")
        );
    }

    #[test]
    fn empty_profile_still_quits_and_shows_help() {
        let mut app = App::new(&Profile {
            boxes: vec![],
            ..profile()
        });
        app.on_key(key('?'));
        assert_eq!(app.mode, Mode::Help);
        app.on_key(key('?'));
        assert_eq!(app.on_key(key('q')), vec![Effect::Quit]);
        assert!(app.on_key(code(KeyCode::Tab)).is_empty());
    }

    #[test]
    fn fold_treats_every_i_alike() {
        assert_eq!(fold("IŞIK"), fold("ışık"));
        assert_eq!(fold("İstanbul"), fold("istanbul"));
        assert_eq!(fold("Iİıi"), "iiii");
        assert_eq!(fold("i\u{307}"), "i");
        assert_eq!(fold("ÇĞÖÜ"), "çğöü");
    }

    #[test]
    fn search_finds_titles_across_both_languages() {
        let mut app = loaded_app();
        app.boxes[REDMINE].items = vec![item("1", "issue 7"), item("2", "ISI sorunu")];
        for (query, expected) in [("ISSUE", "1"), ("ısı", "2")] {
            app.mode = Mode::Search {
                zoom: REDMINE,
                query: query.into(),
            };
            assert_eq!(ids(&app, REDMINE), vec![expected], "query {query}");
        }
    }

    #[test]
    fn active_filter_is_the_live_query_then_the_kept_filter() {
        let mut app = loaded_app();
        app.focus = REDMINE;
        assert_eq!(app.active_filter(REDMINE), None);
        app.on_key(code(KeyCode::Enter));
        assert_eq!(app.active_filter(REDMINE), None);
        app.on_key(key('/'));
        assert_eq!(app.active_filter(REDMINE), None);
        app.on_key(key('d'));
        assert_eq!(app.active_filter(REDMINE), Some("d"));
        assert_eq!(app.active_filter(SOZLU), None);
        app.on_key(code(KeyCode::Enter));
        assert_eq!(app.active_filter(REDMINE), Some("d"));
        app.on_key(code(KeyCode::Esc));
        assert_eq!(app.active_filter(REDMINE), None);
    }

    #[test]
    fn backspace_removes_a_whole_grapheme() {
        let mut app = loaded_app();
        paste_into_capture(&mut app, "ae\u{301}");
        app.on_key(code(KeyCode::Backspace));
        assert_eq!(app.mode, Mode::Capture { input: "a".into() });
        app.on_paste("\u{1F1F9}\u{1F1F7}");
        app.on_key(code(KeyCode::Backspace));
        assert_eq!(app.mode, Mode::Capture { input: "a".into() });
        app.on_key(code(KeyCode::Esc));
        app.focus = REDMINE;
        app.on_key(code(KeyCode::Enter));
        app.on_key(key('/'));
        app.on_paste("e\u{301}");
        app.on_key(code(KeyCode::Backspace));
        assert_eq!(app.active_filter(REDMINE), None);
    }

    #[test]
    fn double_space_plans_once() {
        let mut app = loaded_app();
        app.focus = REDMINE;
        app.mode = Mode::Zoom(REDMINE);
        app.boxes[SOZLU].pending = 1;
        assert!(app.on_key(key(' ')).is_empty());
        app.on_event(finished(SOZLU, "drop", Ok(())));
        assert_eq!(app.boxes[SOZLU].pending, 0);
        assert_eq!(app.on_key(key(' ')).len(), 1);
    }

    #[test]
    fn pending_drops_by_one_per_result_and_never_below_zero() {
        let mut app = loaded_app();
        app.boxes[ZULIP].pending = 2;
        list(&mut app, ZULIP, View::Summary, vec![]);
        assert_eq!(app.boxes[ZULIP].pending, 1);
        app.on_event(BoxEvent::Listed {
            idx: ZULIP,
            view: View::Summary,
            result: Err("x".into()),
        });
        assert_eq!(app.boxes[ZULIP].pending, 0);
        app.on_event(finished(ZULIP, "done", Ok(())));
        assert_eq!(app.boxes[ZULIP].pending, 0);
    }

    #[test]
    fn one_empty_list_does_not_close_linked_items_in_the_app() {
        let mut app = loaded_app();
        assert!(list(&mut app, REDMINE, View::Full, vec![]).is_empty());
        assert_eq!(
            list(&mut app, REDMINE, View::Full, vec![]),
            vec![plain_action(SOZLU, "t2", "done", json!({}))]
        );
    }
}

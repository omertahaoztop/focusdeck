//! Draws the board, zoomed box, overlays and hint line.

use chrono::{DateTime, Local};
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use std::sync::LazyLock;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::{App, Drag, DropPlan, Mode};
use super::{hint, layout};
use crate::cli::display_safe;
use crate::i18n::{msg, t, t_args};
use seenboard_protocol::Item;

const HELP: [&str; 13] = [
    msg::HELP_SELECT,
    msg::HELP_REORDER,
    msg::HELP_MOUSE,
    msg::HELP_NOTE,
    msg::HELP_DONE,
    msg::HELP_UNSTAR,
    msg::HELP_MOVE,
    msg::HELP_PLAN,
    msg::HELP_OPEN,
    msg::HELP_SEARCH,
    msg::HELP_NEXT,
    msg::HELP_ZOOM,
    msg::HELP_QUIT,
];

/// Cuts to a display width, ending in `…` when anything was removed.
pub fn truncate(s: &str, width: usize) -> String {
    if s.width() <= width {
        return s.to_owned();
    }
    let budget = width.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0;
    for g in s.graphemes(true) {
        let w = g.width();
        if used + w > budget {
            break;
        }
        used += w;
        out.push_str(g);
    }
    if width > 0 {
        out.push('…');
    }
    out
}

static COLOUR: LazyLock<bool> =
    LazyLock::new(|| std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty()));

/// Sets a foreground only when colour is on; crossterm turns colour changes under NO_COLOR into an SGR reset that cancels REVERSED.
fn colour(style: Style, c: Color, on: bool) -> Style {
    if on { style.fg(c) } else { style }
}

pub fn draw(frame: &mut Frame, app: &App) {
    draw_at(frame, app, Local::now(), *COLOUR);
}

pub fn draw_at(frame: &mut Frame, app: &App, now: DateTime<Local>, on: bool) {
    let area = frame.area();
    let [top, body, bottom] = layout::areas(area);
    let scene = Scene {
        app,
        now,
        on,
        drag: app.drag(),
    };
    frame.render_widget(Paragraph::new(status_line(app, now, top.width, on)), top);
    let hint_line = truncate(&hint::text(app), usize::from(bottom.width));
    frame.render_widget(Paragraph::new(hint_line), bottom);
    let focus = app.zoomed_box().unwrap_or(app.focus);
    for (idx, rect) in layout::frame_rects(area, app).into_iter().enumerate() {
        if !rect.is_empty() {
            draw_box(frame, &scene, idx, rect, idx == focus);
        }
    }
    if let Some(drag) = &scene.drag {
        draw_drag(frame, area, drag, &scene);
    }
    match &app.mode {
        Mode::Capture { input } => draw_capture(frame, body, input),
        Mode::MovePicker { from, .. } => draw_picker(frame, app, body, *from),
        Mode::Help => draw_help(frame, body),
        _ => {}
    }
}

fn status_line(app: &App, now: DateTime<Local>, width: u16, on: bool) -> Line<'static> {
    let refreshed = app.refreshed_at.map_or_else(
        || t(msg::STATUS_NOT_REFRESHED),
        |at| {
            let time = at.format("%H:%M:%S").to_string();
            t_args(msg::STATUS_REFRESHED, &[("time", time.as_str().into())])
        },
    );
    let head = format!(
        "Seenboard · {} · {} · {refreshed}",
        display_safe(&app.profile_name),
        now.format("%H:%M")
    );
    let mut spans = vec![Span::styled(
        head,
        Style::new().add_modifier(Modifier::BOLD),
    )];
    if let Some(flash) = &app.flash {
        spans.push(Span::styled(
            format!(" · {}", display_safe(flash)),
            colour(Style::new(), Color::Red, on),
        ));
    }
    clip_line(spans, usize::from(width))
}

fn clip_line(spans: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let mut left = width;
    let mut out = Vec::new();
    for span in spans {
        if left == 0 {
            break;
        }
        let text = truncate(&span.content, left);
        left -= text.width().min(left);
        out.push(Span::styled(text, span.style));
    }
    Line::from(out)
}

fn is_overdue(item: &Item, now: DateTime<Local>) -> bool {
    item.overdue || item.due.is_some_and(|due| due < now)
}

/// Everything one frame needs besides the frame itself.
struct Scene<'a> {
    app: &'a App,
    now: DateTime<Local>,
    on: bool,
    drag: Option<Drag>,
}

impl Scene<'_> {
    fn paint(&self, c: Color) -> Style {
        colour(Style::new(), c, self.on)
    }
}

/// The title text and, in search mode, the column right after the typed query.
fn box_title(app: &App, idx: usize, now: DateTime<Local>) -> (String, Option<usize>) {
    let state = &app.boxes[idx];
    let visible = app.visible(idx);
    let overdue = visible.iter().filter(|i| is_overdue(i, now)).count();
    let counts = match app.planned_count(idx) {
        Some((planned, total)) if app.zoomed_box() != Some(idx) => t_args(
            msg::BOX_PLANNED,
            &[("planned", planned.into()), ("total", total.into())],
        ),
        _ => t_args(msg::BOX_COUNT, &[("total", visible.len().into())]),
    };
    let mut title = format!("{} {counts}", display_safe(&state.spec.title));
    if overdue > 0 {
        title.push_str(&format!(
            " · {}",
            t_args(msg::BOX_OVERDUE, &[("count", overdue.into())])
        ));
    }
    let searching = match &app.mode {
        Mode::Search { zoom, query } if *zoom == idx => Some(query.as_str()),
        _ => None,
    };
    let mut cursor = None;
    if let Some(query) = searching.or_else(|| app.active_filter(idx)) {
        title.push_str(&format!(" · /{}", display_safe(query)));
        if searching.is_some() {
            cursor = Some(title.width());
        }
    }
    if state.pending > 0 {
        title.push_str(" …");
    }
    (title, cursor)
}

fn accepts_drop(app: &App, drag: &Drag, idx: usize) -> bool {
    let describe = app.boxes[idx].describe.as_ref();
    if idx == drag.from {
        describe.is_some_and(|d| d.actions.iter().any(|a| a == "reorder"))
    } else {
        describe.is_some_and(|d| d.accepts_drop)
    }
}

fn draw_drag(frame: &mut Frame, area: Rect, drag: &Drag, scene: &Scene) {
    let title = display_safe(&drag.item.title);
    let width = (title.width() as u16)
        .saturating_add(2)
        .clamp(8, 32)
        .min(area.width);
    let height = 3.min(area.height);
    let x = drag
        .x
        .saturating_add(1)
        .min(area.right().saturating_sub(width));
    let y = drag
        .y
        .saturating_add(1)
        .min(area.bottom().saturating_sub(height));
    let rect = Rect::new(x, y, width, height);
    frame.render_widget(Clear, rect);
    let block = Block::bordered()
        .border_type(BorderType::LightDoubleDashed)
        .border_style(scene.paint(Color::Green));
    let text = truncate(&title, usize::from(width.saturating_sub(2)));
    frame.render_widget(Paragraph::new(text).block(block), rect);
}

fn draw_box(frame: &mut Frame, scene: &Scene, idx: usize, rect: Rect, focused: bool) {
    let app = scene.app;
    let state = &app.boxes[idx];
    let drag = scene.drag.as_ref();
    let plan = drag.map(|d| d.plan);
    let hovered =
        matches!(plan, Some(DropPlan::Into(i) | DropPlan::Reorder { idx: i, .. }) if i == idx);
    let mark = match plan {
        Some(DropPlan::Reorder { idx: i, row }) if i == idx => Some(row),
        _ => None,
    };
    let border = if hovered {
        scene.paint(Color::Green).add_modifier(Modifier::BOLD)
    } else if plan == Some(DropPlan::Rejected(idx)) {
        scene.paint(Color::Red)
    } else if drag.is_some_and(|d| accepts_drop(app, d, idx)) {
        scene.paint(Color::Green)
    } else if focused {
        scene.paint(Color::Cyan)
    } else {
        Style::new()
    };
    let border_type = if hovered {
        BorderType::Thick
    } else if focused {
        BorderType::Double
    } else {
        BorderType::Plain
    };
    let (title, cursor) = box_title(app, idx, scene.now);
    let title = truncate(&title, usize::from(rect.width.saturating_sub(2)));
    // the title inherits the border colour unless it is reset
    let title_style = if focused {
        Style::new().fg(Color::Reset).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(Color::Reset)
    };
    if let Some(col) = cursor {
        let col = col
            .min(title.width())
            .min(usize::from(rect.width.saturating_sub(2)));
        frame.set_cursor_position((rect.x + 1 + col as u16, rect.y));
    }
    let block = Block::bordered()
        .border_type(border_type)
        .border_style(border)
        .title(Span::styled(title, title_style));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    if let Some(error) = &state.error {
        let message = t_args(
            msg::BOX_ERROR,
            &[("message", display_safe(error).as_str().into())],
        );
        let text = truncate(&message, usize::from(inner.width));
        let row = Rect::new(inner.x, inner.y, inner.width, inner.height.min(1));
        frame.render_widget(Paragraph::new(text).style(scene.paint(Color::Red)), row);
    }
    let list = layout::list_area(rect, state.error.is_some());
    let scroll = layout::effective_scroll(state, list.height);
    let zoomed = app.zoomed_box() == Some(idx);
    let visible = app.visible(idx);
    let mut lines: Vec<Line> = visible
        .iter()
        .enumerate()
        .skip(scroll)
        .take(usize::from(list.height))
        .map(|(row, item)| {
            let selected = focused && row == state.selected;
            let marked = mark == Some(row);
            let width = usize::from(list.width).saturating_sub(if marked { 2 } else { 0 });
            let mut line = item_line(scene, idx, item, width, zoomed, selected);
            if marked {
                let arrow = scene.paint(Color::Green).add_modifier(Modifier::BOLD);
                line.spans.insert(0, Span::styled("▶ ", arrow));
            }
            line
        })
        .collect();
    if let Some(query) = app.active_filter(idx)
        && visible.is_empty()
    {
        let text = t_args(
            msg::BOX_NO_MATCHES,
            &[("query", display_safe(query).as_str().into())],
        );
        lines.push(Line::from(truncate(&text, usize::from(list.width))));
    }
    frame.render_widget(Paragraph::new(lines), list);
}

fn item_line(
    scene: &Scene,
    idx: usize,
    item: &Item,
    width: usize,
    zoomed: bool,
    selected: bool,
) -> Line<'static> {
    let overdue = is_overdue(item, scene.now);
    let mut style = Style::new();
    if overdue {
        style = scene.paint(Color::Red);
    }
    if scene.app.boxes[idx].error.is_some() {
        style = style.add_modifier(Modifier::DIM);
    }
    if selected {
        style = style.add_modifier(Modifier::REVERSED);
    }
    let flag = if overdue { "! " } else { "" };
    let star = if zoomed && scene.app.is_planned(idx, item) {
        "★ "
    } else {
        ""
    };
    let head = truncate(&format!("{flag}{star}{}", display_safe(&item.title)), width);
    let mut spans = vec![Span::raw(head.clone())];
    let left = width.saturating_sub(head.width());
    if let Some(sub) = item.subtitle.as_deref().filter(|s| !s.is_empty())
        && left > 3
    {
        let text = truncate(&format!("  {}", display_safe(sub)), left);
        spans.push(Span::styled(text, Style::new().add_modifier(Modifier::DIM)));
    }
    Line::from(spans).style(style)
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [row] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    let [cell] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(row);
    cell
}

fn draw_overlay(frame: &mut Frame, area: Rect, title: &str, lines: Vec<Line<'static>>) {
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Double)
        .title(truncate(title, usize::from(area.width.saturating_sub(2))));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_capture(frame: &mut Frame, body: Rect, input: &str) {
    let area = centered(body, body.width.min(70), 3);
    let inner = usize::from(area.width.saturating_sub(2));
    let tail_start = input
        .char_indices()
        .rev()
        .scan(0, |used, (at, c)| {
            *used += c.width().unwrap_or(0);
            Some((at, *used))
        })
        .take_while(|(_, used)| *used < inner.max(1))
        .last()
        .map_or(input.len(), |(at, _)| at);
    let shown = display_safe(&input[tail_start..]);
    let col = u16::try_from(shown.width()).unwrap_or(u16::MAX);
    draw_overlay(frame, area, &t(msg::CAPTURE_TITLE), vec![Line::from(shown)]);
    let x = area.x.saturating_add(1).saturating_add(col);
    frame.set_cursor_position((x.min(area.right().saturating_sub(2)), area.y + 1));
}

fn draw_picker(frame: &mut Frame, app: &App, body: Rect, from: usize) {
    let lines: Vec<Line> = app
        .picker_targets(from)
        .into_iter()
        .take(9)
        .enumerate()
        .map(|(n, i)| {
            Line::from(format!(
                "{} {}",
                n + 1,
                display_safe(&app.boxes[i].spec.title)
            ))
        })
        .collect();
    let area = centered(body, body.width.min(40), lines.len() as u16 + 2);
    draw_overlay(frame, area, &t(msg::PICKER_TITLE), lines);
}

fn draw_help(frame: &mut Frame, body: Rect) {
    let lines: Vec<Line> = HELP.iter().map(|id| Line::from(t(id))).collect();
    let widest = lines.iter().map(Line::width).max().unwrap_or(0);
    let width = u16::try_from(widest + 2).unwrap_or(u16::MAX);
    let area = centered(body, body.width.min(width), HELP.len() as u16 + 2);
    draw_overlay(frame, area, &t(msg::HELP_TITLE), lines);
}

#[cfg(test)]
mod tests {
    use super::super::app::Mode;
    use super::super::app::testkit::*;
    use super::super::hint;
    use super::*;
    use crate::i18n::{self, msg};
    use chrono::{Duration, TimeZone};
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Modifier};
    use std::time::Instant;
    use unicode_width::UnicodeWidthStr;

    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, 12, 14, 32, 0).unwrap()
    }

    fn board() -> App {
        let mut app = loaded_app();
        for (i, b) in app.boxes.iter_mut().enumerate() {
            b.spec.col = i as u16;
        }
        app.boxes[SOZLU].spec.title = "Sözlü işler".into();
        app.boxes[REDMINE].spec.title = "Redmine planım".into();
        app
    }

    fn render(app: &App, w: u16, h: u16) -> Terminal<TestBackend> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw_at(f, app, now(), true)).unwrap();
        term
    }

    fn render_plain(app: &App, w: u16, h: u16) -> Terminal<TestBackend> {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw_at(f, app, now(), false)).unwrap();
        term
    }

    fn cursor(term: &mut Terminal<TestBackend>) -> (u16, u16) {
        let pos = term.get_cursor_position().unwrap();
        (pos.x, pos.y)
    }

    fn rows(term: &Terminal<TestBackend>) -> Vec<String> {
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect()
    }

    fn find(term: &Terminal<TestBackend>, needle: &str) -> (u16, u16) {
        for (y, row) in rows(term).iter().enumerate() {
            if let Some(byte) = row.find(needle) {
                return (row[..byte].chars().count() as u16, y as u16);
            }
        }
        panic!("{needle:?} ekranda yok:\n{}", rows(term).join("\n"));
    }

    #[test]
    fn truncate_respects_display_width() {
        let cut = truncate("İğüşçö 🚀 çok uzun başlık", 10);
        assert!(cut.width() <= 10, "{cut:?}");
        assert!(cut.ends_with('…'));
        assert_eq!(truncate("kısa", 10), "kısa");
        assert_eq!(truncate("tam", 3), "tam");
        assert_eq!(truncate("dört", 0), "");
        assert_eq!(truncate("🚀🚀🚀", 4), "🚀…");
    }

    #[test]
    fn truncate_never_splits_a_grapheme_cluster() {
        assert_eq!(truncate("xe\u{301}yzw", 3), "xe\u{301}…");
        let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
        let cut = truncate(&format!("ab{family}cd"), 5);
        assert_eq!(cut, format!("ab{family}…"));
        let cut = truncate(&format!("ab{family}cd"), 4);
        assert_eq!(cut, "ab…");
    }

    #[test]
    fn no_color_leaves_every_cell_at_the_default_colours_and_reverses_the_selection() {
        let mut app = board();
        app.boxes[SOZLU].items[0].due = Some((now() - Duration::days(1)).fixed_offset());
        app.boxes[ZULIP].error = Some("ağ yok".into());
        let term = render_plain(&app, 120, 40);
        let buf = term.backend().buffer();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let cell = &buf[(x, y)];
                assert_eq!((cell.fg, cell.bg), (Color::Reset, Color::Reset), "{x},{y}");
            }
        }
        let (x, y) = find(&term, "birinci görev");
        assert!(buf[(x, y)].modifier.contains(Modifier::REVERSED));
        let (x, y) = find(&term, "üçüncü görev");
        assert!(!buf[(x, y)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn overdue_rows_start_with_a_bang_with_or_without_colour() {
        let mut app = board();
        app.boxes[ZULIP].items[1].due = Some((now() - Duration::days(1)).fixed_offset());
        for term in [render(&app, 120, 40), render_plain(&app, 120, 40)] {
            find(&term, "! ikinci mesaj");
            assert!(!rows(&term).iter().any(|r| r.contains("! zulip mesajı")));
        }
    }

    #[test]
    fn focused_box_and_overlays_use_double_borders() {
        let mut app = board();
        let term = render(&app, 120, 40);
        let buf = term.backend().buffer();
        assert_eq!(buf[(0, 1)].symbol(), "╔");
        assert_eq!(buf[(40, 1)].symbol(), "┌");
        app.mode = Mode::Help;
        let term = render(&app, 120, 40);
        let (x, y) = find(&term, "Keys");
        assert_eq!(term.backend().buffer()[(x - 1, y)].symbol(), "╔");
        app.mode = Mode::Capture {
            input: String::new(),
        };
        let term = render(&app, 120, 40);
        let (x, y) = find(&term, "Note");
        assert_eq!(term.backend().buffer()[(x - 1, y)].symbol(), "╔");
        app.mode = Mode::MovePicker {
            from: ZULIP,
            item: "z1".into(),
        };
        let term = render(&app, 120, 40);
        let (x, y) = find(&term, "Move to");
        assert_eq!(term.backend().buffer()[(x - 1, y)].symbol(), "╔");
    }

    #[test]
    fn titles_hint_and_subtitles_keep_the_default_foreground() {
        let mut app = board();
        app.boxes[ZULIP].items[0].subtitle = Some("alt satır".into());
        let term = render(&app, 120, 40);
        let buf = term.backend().buffer();
        let (x, y) = find(&term, "zulip 2");
        assert_eq!(buf[(x, y)].fg, Color::Reset);
        let (x, y) = find(&term, "Redmine planım");
        assert_eq!(buf[(x, y)].fg, Color::Reset);
        let (x, y) = find(&term, "? help");
        assert_eq!(buf[(x, y)].fg, Color::Reset);
        let (x, y) = find(&term, "alt satır");
        assert_eq!(buf[(x, y)].fg, Color::Reset);
        assert!(buf[(x, y)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn board_hint_at_width_40_still_names_help_and_quit() {
        let term = render(&board(), 40, 20);
        let last = rows(&term).pop().unwrap();
        assert!(last.contains("? help") && last.contains("q quit"), "{last}");
    }

    #[test]
    fn kept_filter_stays_in_the_zoomed_title_and_empty_result_says_so() {
        let mut app = board();
        app.focus = REDMINE;
        app.mode = Mode::Zoom(REDMINE);
        app.mode = Mode::Search {
            zoom: REDMINE,
            query: "ışık".into(),
        };
        app.on_key(crossterm::event::KeyEvent::from(
            crossterm::event::KeyCode::Enter,
        ));
        let term = render(&app, 120, 40);
        find(&term, "Redmine planım 1 · /ışık");
        app.mode = Mode::Search {
            zoom: REDMINE,
            query: "yok böyle".into(),
        };
        app.on_key(crossterm::event::KeyEvent::from(
            crossterm::event::KeyCode::Enter,
        ));
        let term = render(&app, 120, 40);
        find(&term, "Redmine planım 0 · /yok böyle");
        find(&term, "no matches for \"yok böyle\"");
    }

    #[test]
    fn unfiltered_zoom_has_no_filter_suffix() {
        let mut app = board();
        app.focus = REDMINE;
        app.mode = Mode::Zoom(REDMINE);
        let term = render(&app, 120, 40);
        assert!(!rows(&term)[1].contains(" · /"), "{}", rows(&term)[1]);
    }

    #[test]
    fn title_ends_with_an_ellipsis_while_commands_are_pending() {
        let mut app = board();
        app.boxes[ZULIP].pending = 1;
        let term = render(&app, 120, 40);
        find(&term, "zulip 2 …");
        app.boxes[ZULIP].pending = 0;
        let term = render(&app, 120, 40);
        assert!(!rows(&term).iter().any(|r| r.contains("zulip 2 …")));
    }

    #[test]
    fn capture_places_the_terminal_cursor_after_the_input() {
        let mut app = board();
        app.mode = Mode::Capture {
            input: "fikir".into(),
        };
        let mut term = render(&app, 120, 40);
        let (x, y) = find(&term, "fikir");
        assert_eq!(cursor(&mut term), (x + 5, y));
        assert!(!rows(&term).iter().any(|r| r.contains('▏')));
    }

    #[test]
    fn capture_cursor_stays_inside_the_frame_for_scrolled_input() {
        let mut app = board();
        app.mode = Mode::Capture {
            input: format!("{} sonuncu", "çok uzun not ".repeat(20)),
        };
        let mut term = render(&app, 60, 10);
        let (x, y) = find(&term, "sonuncu");
        let (cx, cy) = cursor(&mut term);
        assert_eq!(cy, y);
        assert_eq!(cx, x + 7);
        let right = rows(&term)[usize::from(y)].chars().count() as u16 - 1;
        assert!(cx < right, "{cx} {right}");
    }

    #[test]
    fn search_places_the_cursor_after_the_query_in_the_title() {
        let mut app = board();
        app.focus = REDMINE;
        app.mode = Mode::Search {
            zoom: REDMINE,
            query: "ışı".into(),
        };
        app.boxes[REDMINE].pending = 1;
        let mut term = render(&app, 120, 40);
        let (x, y) = find(&term, "/ışı");
        assert_eq!(cursor(&mut term), (x + 4, y));
    }

    #[test]
    fn renders_in_tiny_terminal() {
        let mut app = board();
        app.boxes[ZULIP].error = Some("ağ yok".into());
        for (w, h) in [(20, 5), (1, 1), (3, 2), (0, 0), (2, 40)] {
            render(&app, w, h);
        }
        app.mode = Mode::Zoom(SOZLU);
        render(&app, 20, 5);
        app.mode = Mode::Help;
        render(&app, 20, 5);
        app.mode = Mode::Capture {
            input: "not".into(),
        };
        render(&app, 20, 5);
    }

    #[test]
    fn header_counts_items_and_overdue() {
        let mut app = board();
        app.boxes[SOZLU].items[0].due = Some((now() - Duration::hours(3)).fixed_offset());
        let term = render(&app, 120, 40);
        find(&term, "Sözlü işler 3 · 1 overdue");
    }

    #[test]
    fn header_omits_overdue_part_when_none() {
        let term = render(&board(), 120, 40);
        let (x, y) = find(&term, "Sözlü işler 3");
        let row: String = rows(&term)[usize::from(y)]
            .chars()
            .skip(usize::from(x))
            .collect();
        assert!(!row.starts_with("Sözlü işler 3 · "), "{row}");
    }

    #[test]
    fn plan_box_header_shows_planned_over_total_on_board() {
        let term = render(&board(), 120, 40);
        find(&term, "Redmine planım 1 / 3");
    }

    #[test]
    fn error_row_is_red_and_old_items_are_dim() {
        let mut app = board();
        app.boxes[ZULIP].error = Some("plugin kapandı".into());
        let term = render(&app, 120, 40);
        let (x, y) = find(&term, "error: plugin kapandı");
        let buf = term.backend().buffer();
        assert_eq!(buf[(x, y)].fg, Color::Red);
        let (ix, iy) = find(&term, "zulip mesajı");
        assert!(buf[(ix, iy)].modifier.contains(Modifier::DIM));
        let (ox, oy) = find(&term, "birinci görev");
        assert!(!buf[(ox, oy)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn overdue_item_row_is_red() {
        let mut app = board();
        app.boxes[ZULIP].items[1].due = Some((now() - Duration::days(1)).fixed_offset());
        let term = render(&app, 120, 40);
        let (x, y) = find(&term, "ikinci mesaj");
        assert_eq!(term.backend().buffer()[(x, y)].fg, Color::Red);
        let (x, y) = find(&term, "zulip mesajı");
        assert_ne!(term.backend().buffer()[(x, y)].fg, Color::Red);
    }

    #[test]
    fn plugin_overdue_flag_counts_without_due() {
        let mut app = board();
        app.boxes[ZULIP].items[0].overdue = true;
        let term = render(&app, 120, 40);
        find(&term, "zulip 2 · 1 overdue");
    }

    #[test]
    fn zoom_marks_planned_items_with_a_star() {
        let mut app = board();
        app.focus = REDMINE;
        app.mode = Mode::Zoom(REDMINE);
        let term = render(&app, 120, 40);
        find(&term, "★ DEPO hazırlığı");
        find(&term, "İstanbul sunucusu");
        assert!(!rows(&term).iter().any(|r| r.contains("★ İstanbul")));
        assert!(!rows(&term).iter().any(|r| r.contains("sozlu")));
    }

    #[test]
    fn last_row_is_the_hint_and_first_row_names_the_profile() {
        let app = board();
        let term = render(&app, 120, 40);
        let rows = rows(&term);
        assert_eq!(rows[39].trim_end(), hint::text(&app));
        assert!(rows[0].starts_with("Seenboard · iş · "), "{}", rows[0]);
        assert!(rows[0].contains("14:32"));
        assert!(rows[0].contains(" · refreshed "), "{}", rows[0]);
    }

    #[test]
    fn status_line_says_when_nothing_was_loaded_yet() {
        let term = render(&App::new(&profile()), 120, 40);
        assert!(rows(&term)[0].contains("not refreshed yet"));
    }

    #[test]
    fn overlays_carry_their_titles() {
        let mut app = board();
        app.mode = Mode::Help;
        find(&render(&app, 120, 40), "Keys");
        let help = rows(&render(&app, 120, 40)).join("\n");
        assert!(help.contains("a  take a note (into the capture_into box)"));
        assert!(help.contains("q  quit"));
        app.mode = Mode::Capture {
            input: String::new(),
        };
        find(&render(&app, 120, 40), "Note");
        app.mode = Mode::MovePicker {
            from: ZULIP,
            item: "z1".into(),
        };
        find(&render(&app, 120, 40), "Move to");
    }

    #[test]
    fn help_lists_selection_reorder_mouse_and_the_second_space() {
        let mut app = board();
        app.mode = Mode::Help;
        let help = rows(&render(&app, 120, 40)).join("\n");
        for needle in [
            "PageUp/PageDown, Home/End  select item",
            "Shift+↑↓  move the item up or down",
            "click select · double click zoom · drag move or reorder",
            "a second space deletes it",
        ] {
            assert!(help.contains(needle), "{needle}\n{help}");
        }
    }

    #[test]
    fn help_overlay_is_as_wide_as_its_longest_line_plus_borders() {
        let mut app = board();
        app.mode = Mode::Help;
        let longest = HELP.iter().map(|id| t(id).width()).max().unwrap();
        let term = render(&app, 120, 40);
        let (_, y) = find(&term, "Keys");
        let top = &rows(&term)[y as usize];
        let left = top.chars().position(|c| c == '╔').unwrap();
        let right = top.chars().position(|c| c == '╗').unwrap();
        assert_eq!(right - left + 1, longest + 2);
    }

    #[test]
    fn tr_ui_texts_unchanged() {
        let tr = i18n::localizer(Some("tr"));
        assert_eq!(tr.t(msg::HINT_BOARD_NOTE), "a not al");
        assert_eq!(tr.t(msg::PICKER_TITLE), "Nereye taşınsın");
        assert_eq!(
            tr.t_args(msg::BOX_OVERDUE, &[("count", 2.into())]),
            "2 gecikmiş"
        );
        assert_eq!(tr.t(msg::HELP_TITLE), "Tuşlar");
    }

    #[test]
    fn en_overdue_plural() {
        let en = i18n::localizer(None);
        assert_eq!(
            en.t_args(msg::BOX_OVERDUE, &[("count", 1.into())]),
            "1 overdue"
        );
        assert_eq!(
            en.t_args(msg::BOX_OVERDUE, &[("count", 3.into())]),
            "3 overdue"
        );
    }

    #[test]
    fn flash_shows_on_the_top_row() {
        let mut app = board();
        app.flash = Some("this item has no link".into());
        let term = render(&app, 120, 40);
        assert!(rows(&term)[0].contains("this item has no link"));
    }

    #[test]
    fn control_characters_in_titles_are_neutralised() {
        let mut app = board();
        app.boxes[ZULIP].items[0].title = "kötü\u{1b}[31m başlık".into();
        let term = render(&app, 120, 40);
        assert!(!rows(&term).iter().any(|r| r.contains('\u{1b}')));
        find(&term, "kötü?[31m başlık");
    }

    #[test]
    fn capture_overlay_shows_input() {
        let mut app = board();
        app.mode = Mode::Capture {
            input: "yeni fikir".into(),
        };
        let term = render(&app, 120, 40);
        find(&term, "yeni fikir");
    }

    #[test]
    fn capture_overlay_keeps_the_end_of_long_input_visible() {
        let mut app = board();
        app.mode = Mode::Capture {
            input: format!("{} sonuncu", "çok uzun not ".repeat(20)),
        };
        let term = render(&app, 60, 10);
        find(&term, "sonuncu");
    }

    fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn pointer(app: &mut App, kind: MouseEventKind, x: u16, y: u16) {
        let rects = layout::frame_rects(Rect::new(0, 0, 120, 40), app);
        let target = layout::hit(&rects, app, x, y);
        app.on_mouse(mouse(kind, x, y), Instant::now(), |_, _| target);
    }

    fn press_zulip_first_item(app: &mut App) {
        pointer(app, MouseEventKind::Down(MouseButton::Left), 81, 2);
    }

    #[test]
    fn drag_draws_a_dashed_frame_with_the_item_title_at_the_pointer() {
        let mut app = board();
        press_zulip_first_item(&mut app);
        pointer(&mut app, MouseEventKind::Drag(MouseButton::Left), 10, 10);
        let term = render(&app, 120, 40);
        let (x, y) = find(&term, "╌");
        assert!((11..14).contains(&x) && (10..13).contains(&y), "{x},{y}");
        let rows = rows(&term);
        assert!(rows[usize::from(y) + 1].contains("zulip mesajı"));
    }

    #[test]
    fn drag_frame_is_clamped_inside_the_frame() {
        let mut app = board();
        press_zulip_first_item(&mut app);
        pointer(&mut app, MouseEventKind::Drag(MouseButton::Left), 119, 39);
        let term = render(&app, 120, 40);
        find(&term, "zulip mesajı");
    }

    #[test]
    fn hovered_target_gets_a_thick_border_others_a_thin_one() {
        let mut app = board();
        press_zulip_first_item(&mut app);
        pointer(&mut app, MouseEventKind::Drag(MouseButton::Left), 10, 10);
        let term = render(&app, 120, 40);
        let corner = &term.backend().buffer()[(0, 1)];
        assert_eq!((corner.symbol(), corner.fg), ("┏", Color::Green));

        pointer(&mut app, MouseEventKind::Drag(MouseButton::Left), 50, 10);
        let term = render(&app, 120, 40);
        let buf = term.backend().buffer();
        assert_eq!((buf[(0, 1)].symbol(), buf[(0, 1)].fg), ("┌", Color::Green));
        assert_eq!(buf[(40, 1)].fg, Color::Red, "redmine kabul etmez");
        assert_ne!(
            buf[(80, 1)].fg,
            Color::Green,
            "zulip kaynak ve sıralama yok"
        );
    }

    #[test]
    fn reorder_marks_the_row_it_lands_on() {
        let mut app = board();
        pointer(&mut app, MouseEventKind::Down(MouseButton::Left), 1, 2);
        pointer(&mut app, MouseEventKind::Drag(MouseButton::Left), 20, 4);
        let term = render(&app, 120, 40);
        assert!(
            rows(&term)[4].starts_with("┃▶ üçüncü görev"),
            "{}",
            rows(&term)[4]
        );
        assert!(!rows(&term)[2].contains('▶'));
    }

    #[test]
    fn source_box_is_highlighted_when_it_can_reorder() {
        let mut app = board();
        pointer(&mut app, MouseEventKind::Down(MouseButton::Left), 1, 2);
        pointer(&mut app, MouseEventKind::Drag(MouseButton::Left), 20, 8);
        let term = render(&app, 120, 40);
        let cell = &term.backend().buffer()[(0, 1)];
        assert_eq!(cell.fg, Color::Green);
    }

    #[test]
    fn plain_click_shows_no_drag_frame_or_highlight() {
        let mut app = board();
        press_zulip_first_item(&mut app);
        pointer(&mut app, MouseEventKind::Drag(MouseButton::Left), 82, 2);
        pointer(&mut app, MouseEventKind::Up(MouseButton::Left), 82, 2);
        let term = render(&app, 120, 40);
        assert!(!rows(&term).iter().any(|r| r.contains('╌')));
        assert_ne!(term.backend().buffer()[(0, 1)].fg, Color::Green);
    }
}

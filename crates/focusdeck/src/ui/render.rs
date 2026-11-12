//! Draws the board, zoomed box, overlays and hint line.

use chrono::{DateTime, Local};
use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::{App, BoxState, Drag, Mode};
use super::{hint, layout};
use crate::cli::display_safe;
use focusdeck_protocol::Item;

const HELP: [&str; 10] = [
    "a  not al (capture_into kutusuna)",
    "x  seçili öğe bitti",
    "u  yıldızı kaldır",
    "m  başka kutuya taşı",
    "boşluk  planıma al / çıkar (büyütülmüş kutu)",
    "o  linki tarayıcıda aç",
    "/  ara (büyütülmüş kutu)",
    "Tab  sonraki kutu",
    "Enter  kutuyu büyüt · Esc geri",
    "q  çık",
];

/// Cuts to a display width, ending in `…` when anything was removed.
pub fn truncate(s: &str, width: usize) -> String {
    if s.width() <= width {
        return s.to_owned();
    }
    let budget = width.saturating_sub(1);
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > budget {
            break;
        }
        used += w;
        out.push(c);
    }
    if width > 0 {
        out.push('…');
    }
    out
}

pub fn draw(frame: &mut Frame, app: &App) {
    draw_at(frame, app, Local::now());
}

pub fn draw_at(frame: &mut Frame, app: &App, now: DateTime<Local>) {
    let area = frame.area();
    let [top, body, bottom] = layout::areas(area);
    frame.render_widget(Paragraph::new(status_line(app, now, top.width)), top);
    let hint_line = truncate(&hint::text(app), usize::from(bottom.width));
    frame.render_widget(
        Paragraph::new(hint_line).style(Style::new().fg(Color::Gray)),
        bottom,
    );
    let focus = app.zoomed_box().unwrap_or(app.focus);
    let drag = app.drag();
    for (idx, rect) in layout::frame_rects(area, app).into_iter().enumerate() {
        if !rect.is_empty() {
            let target = drag.as_ref().is_some_and(|d| accepts_drop(app, d, idx));
            draw_box(frame, app, idx, rect, idx == focus, target, now);
        }
    }
    if let Some(drag) = &drag {
        draw_drag(frame, area, drag);
    }
    match &app.mode {
        Mode::Capture { input } => draw_capture(frame, body, input),
        Mode::MovePicker { from, .. } => draw_picker(frame, app, body, *from),
        Mode::Help => draw_help(frame, body),
        _ => {}
    }
}

fn status_line(app: &App, now: DateTime<Local>, width: u16) -> Line<'static> {
    let refreshed = app.refreshed_at.map_or_else(
        || "yenilenmedi".to_owned(),
        |t| format!("yenilendi {}", t.format("%H:%M:%S")),
    );
    let head = format!(
        "focusdeck · {} · {} · {refreshed}",
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
            Style::new().fg(Color::Red),
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

fn box_title(app: &App, idx: usize, now: DateTime<Local>) -> String {
    let state = &app.boxes[idx];
    let visible = app.visible(idx);
    let overdue = visible.iter().filter(|i| is_overdue(i, now)).count();
    let counts = match app.planned_count(idx) {
        Some((planned, total)) if app.zoomed_box() != Some(idx) => format!("{planned} / {total}"),
        _ => visible.len().to_string(),
    };
    let mut title = format!("{} {counts}", display_safe(&state.spec.title));
    if overdue > 0 {
        title.push_str(&format!(" · {overdue} gecikmiş"));
    }
    if let Mode::Search { zoom, query } = &app.mode
        && *zoom == idx
    {
        title.push_str(&format!(" · /{}", display_safe(query)));
    }
    title
}

fn accepts_drop(app: &App, drag: &Drag, idx: usize) -> bool {
    let describe = app.boxes[idx].describe.as_ref();
    if idx == drag.from {
        describe.is_some_and(|d| d.actions.iter().any(|a| a == "reorder"))
    } else {
        describe.is_some_and(|d| d.accepts_drop)
    }
}

fn draw_drag(frame: &mut Frame, area: Rect, drag: &Drag) {
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
        .border_style(Style::new().fg(Color::Yellow));
    let text = truncate(&title, usize::from(width.saturating_sub(2)));
    frame.render_widget(Paragraph::new(text).block(block), rect);
}

fn draw_box(
    frame: &mut Frame,
    app: &App,
    idx: usize,
    rect: Rect,
    focused: bool,
    drop_target: bool,
    now: DateTime<Local>,
) {
    let state = &app.boxes[idx];
    let border = if drop_target {
        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else if focused {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().fg(Color::DarkGray)
    };
    let title = truncate(
        &box_title(app, idx, now),
        usize::from(rect.width.saturating_sub(2)),
    );
    let title_style = if focused {
        Style::new().add_modifier(Modifier::BOLD)
    } else {
        Style::new()
    };
    let block = Block::bordered()
        .border_style(border)
        .title(Span::styled(title, title_style));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    if let Some(error) = &state.error {
        let text = truncate(
            &format!("hata: {}", display_safe(error)),
            usize::from(inner.width),
        );
        let row = Rect::new(inner.x, inner.y, inner.width, inner.height.min(1));
        frame.render_widget(Paragraph::new(text).style(Style::new().fg(Color::Red)), row);
    }
    let list = layout::list_area(rect, state.error.is_some());
    let scroll = layout::effective_scroll(state, list.height);
    let zoomed = app.zoomed_box() == Some(idx);
    let lines: Vec<Line> = app
        .visible(idx)
        .into_iter()
        .enumerate()
        .skip(scroll)
        .take(usize::from(list.height))
        .map(|(row, item)| {
            let selected = focused && row == state.selected;
            item_line(
                app,
                idx,
                state,
                item,
                usize::from(list.width),
                zoomed,
                selected,
                now,
            )
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), list);
}

#[allow(clippy::too_many_arguments)]
fn item_line(
    app: &App,
    idx: usize,
    state: &BoxState,
    item: &Item,
    width: usize,
    zoomed: bool,
    selected: bool,
    now: DateTime<Local>,
) -> Line<'static> {
    let mut style = Style::new();
    if is_overdue(item, now) {
        style = style.fg(Color::Red);
    }
    if state.error.is_some() {
        style = style.add_modifier(Modifier::DIM);
    }
    if selected {
        style = style.add_modifier(Modifier::REVERSED);
    }
    let star = if zoomed && app.is_planned(idx, item) {
        "★ "
    } else {
        ""
    };
    let head = truncate(&format!("{star}{}", display_safe(&item.title)), width);
    let mut spans = vec![Span::raw(head.clone())];
    let left = width.saturating_sub(head.width());
    if let Some(sub) = item.subtitle.as_deref().filter(|s| !s.is_empty())
        && left > 3
    {
        let text = truncate(&format!("  {}", display_safe(sub)), left);
        let sub_style = if style.fg.is_some() {
            Style::new()
        } else {
            Style::new().fg(Color::DarkGray)
        };
        spans.push(Span::styled(text, sub_style.add_modifier(Modifier::DIM)));
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
        .border_style(Style::new().fg(Color::Yellow))
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
    let shown = format!("{}▏", display_safe(&input[tail_start..]));
    draw_overlay(frame, area, "Not al", vec![Line::from(shown)]);
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
    draw_overlay(frame, area, "Nereye taşınsın", lines);
}

fn draw_help(frame: &mut Frame, body: Rect) {
    let lines: Vec<Line> = HELP.iter().map(|l| Line::from(*l)).collect();
    let area = centered(body, body.width.min(60), HELP.len() as u16 + 2);
    draw_overlay(frame, area, "Tuşlar", lines);
}

#[cfg(test)]
mod tests {
    use super::super::app::Mode;
    use super::super::app::testkit::*;
    use super::super::hint;
    use super::*;
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
        term.draw(|f| draw_at(f, app, now())).unwrap();
        term
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
        find(&term, "Sözlü işler 3 · 1 gecikmiş");
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
        let (x, y) = find(&term, "hata: plugin kapandı");
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
        find(&term, "zulip 2 · 1 gecikmiş");
    }

    #[test]
    fn zoom_marks_planned_items_with_a_star() {
        let mut app = board();
        app.focus = REDMINE;
        app.mode = Mode::Zoom(REDMINE);
        let term = render(&app, 120, 40);
        find(&term, "★ TÜYAP hazırlığı");
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
        assert!(rows[0].starts_with("focusdeck · iş · "), "{}", rows[0]);
        assert!(rows[0].contains("14:32"));
    }

    #[test]
    fn flash_shows_on_the_top_row() {
        let mut app = board();
        app.flash = Some("bu öğenin linki yok".into());
        let term = render(&app, 120, 40);
        assert!(rows(&term)[0].contains("bu öğenin linki yok"));
    }

    #[test]
    fn control_characters_in_titles_are_neutralised() {
        let mut app = board();
        app.boxes[ZULIP].items[0].title = "kötü\u{1b}[31m başlık".into();
        let term = render(&app, 120, 40);
        assert!(!rows(&term).iter().any(|r| r.contains('\u{1b}')));
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
        find(&term, "sonuncu▏");
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
    fn drop_targets_get_the_highlight_border() {
        let mut app = board();
        press_zulip_first_item(&mut app);
        pointer(&mut app, MouseEventKind::Drag(MouseButton::Left), 10, 10);
        let term = render(&app, 120, 40);
        let buf = term.backend().buffer();
        let highlighted = |x: u16| {
            let cell = &buf[(x, 1)];
            cell.fg == Color::Yellow && cell.modifier.contains(Modifier::BOLD)
        };
        assert!(highlighted(0), "sozlu kabul eder");
        assert!(!highlighted(40), "redmine kabul etmez");
        assert!(!highlighted(80), "zulip kaynak ve sıralama yok");
    }

    #[test]
    fn source_box_is_highlighted_when_it_can_reorder() {
        let mut app = board();
        pointer(&mut app, MouseEventKind::Down(MouseButton::Left), 1, 2);
        pointer(&mut app, MouseEventKind::Drag(MouseButton::Left), 20, 8);
        let term = render(&app, 120, 40);
        let cell = &term.backend().buffer()[(0, 1)];
        assert_eq!(cell.fg, Color::Yellow);
    }

    #[test]
    fn plain_click_shows_no_drag_frame_or_highlight() {
        let mut app = board();
        press_zulip_first_item(&mut app);
        pointer(&mut app, MouseEventKind::Drag(MouseButton::Left), 82, 2);
        pointer(&mut app, MouseEventKind::Up(MouseButton::Left), 82, 2);
        let term = render(&app, 120, 40);
        assert!(!rows(&term).iter().any(|r| r.contains('╌')));
        assert_ne!(term.backend().buffer()[(0, 1)].fg, Color::Yellow);
    }
}

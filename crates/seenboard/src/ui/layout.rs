//! Maps boxes onto the terminal grid and screen coordinates back to boxes and rows.

use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::widgets::Block;

use super::app::{App, BoxState, Hit};
use crate::profile::BoxSpec;

/// Top status row, box body, bottom hint row.
pub fn areas(area: Rect) -> [Rect; 3] {
    Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area)
}

pub fn grid(area: Rect, boxes: &[BoxSpec]) -> Vec<Rect> {
    place(area, boxes.iter())
}

fn place<'a>(area: Rect, boxes: impl Iterator<Item = &'a BoxSpec> + Clone) -> Vec<Rect> {
    let extent = |end: fn(&BoxSpec) -> u32| boxes.clone().map(end).max().unwrap_or(0).max(1);
    let rows = extent(|b| u32::from(b.row) + u32::from(b.height));
    let cols = extent(|b| u32::from(b.col) + u32::from(b.width));
    let edge = |origin: u16, len: u16, cells: u32, i: u32| {
        let offset = u32::from(len) * i.min(cells) / cells;
        (u32::from(origin) + offset).min(u32::from(u16::MAX)) as u16
    };
    boxes
        .map(|b| {
            let x0 = edge(area.x, area.width, cols, u32::from(b.col));
            let x1 = edge(
                area.x,
                area.width,
                cols,
                u32::from(b.col) + u32::from(b.width),
            );
            let y0 = edge(area.y, area.height, rows, u32::from(b.row));
            let y1 = edge(
                area.y,
                area.height,
                rows,
                u32::from(b.row) + u32::from(b.height),
            );
            Rect::new(x0, y0, x1 - x0, y1 - y0)
        })
        .collect()
}

/// One rect per box; while a box is zoomed it owns the body and the others get empty rects.
pub fn frame_rects(area: Rect, app: &App) -> Vec<Rect> {
    let body = areas(area)[1];
    match app.zoomed_box() {
        Some(zoomed) => (0..app.boxes.len())
            .map(|i| if i == zoomed { body } else { Rect::ZERO })
            .collect(),
        None => place(body, app.boxes.iter().map(|b| &b.spec)),
    }
}

/// Item rows of a box: its inner area minus the error row.
pub fn list_area(rect: Rect, has_error: bool) -> Rect {
    let inner = Block::bordered().inner(rect);
    if has_error {
        Rect::new(
            inner.x,
            inner.y.saturating_add(1).min(inner.bottom()),
            inner.width,
            inner.height.saturating_sub(1),
        )
    } else {
        inner
    }
}

pub fn keep_visible(scroll: usize, selected: usize, height: usize) -> usize {
    if height == 0 || selected < scroll {
        selected
    } else if selected >= scroll + height {
        selected + 1 - height
    } else {
        scroll
    }
}

pub fn effective_scroll(state: &BoxState, height: u16) -> usize {
    keep_visible(state.scroll, state.selected, usize::from(height))
}

/// App does not know box heights, so the loop calls this before drawing to keep the selection on screen.
pub fn fit_scroll(app: &mut App, area: Rect) {
    let rects = frame_rects(area, app);
    for (state, rect) in app.boxes.iter_mut().zip(rects) {
        if rect.is_empty() {
            continue;
        }
        let list = list_area(rect, state.error.is_some());
        state.scroll = effective_scroll(state, list.height);
    }
}

pub fn hit(rects: &[Rect], app: &App, x: u16, y: u16) -> Option<Hit> {
    let at = Position::new(x, y);
    let (box_idx, rect) = rects.iter().enumerate().find(|(_, r)| r.contains(at))?;
    let state = app.boxes.get(box_idx)?;
    let list = list_area(*rect, state.error.is_some());
    let row = list
        .contains(at)
        .then(|| effective_scroll(state, list.height) + usize::from(y - list.y));
    Some(Hit { box_idx, row })
}

#[cfg(test)]
mod tests {
    use super::super::app::Mode;
    use super::super::app::testkit::*;
    use super::*;
    use std::time::Duration;

    fn spec(id: &str, row: u16, col: u16, width: u16, height: u16) -> BoxSpec {
        BoxSpec {
            id: id.into(),
            title: id.into(),
            plugin: "fake".into(),
            row,
            col,
            width,
            height,
            refresh: Duration::from_secs(60),
            plan_into: None,
            config: toml::Table::new(),
        }
    }

    fn overlaps(a: Rect, b: Rect) -> bool {
        a.intersects(b)
    }

    #[test]
    fn grid_two_by_three_fills_the_area_without_overlap() {
        let boxes: Vec<BoxSpec> = (0..6)
            .map(|i| spec(&format!("b{i}"), i / 3, i % 3, 1, 1))
            .collect();
        let rects = grid(Rect::new(0, 0, 120, 40), &boxes);
        assert_eq!(rects.len(), 6);
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!overlaps(*a, *b), "{a:?} {b:?}");
            }
        }
        let top_width: u16 = rects[..3].iter().map(|r| r.width).sum();
        assert_eq!(top_width, 120);
        let left_height: u16 = [rects[0], rects[3]].iter().map(|r| r.height).sum();
        assert_eq!(left_height, 40);
    }

    #[test]
    fn grid_wide_box_spans_two_columns() {
        let boxes = vec![
            spec("a", 0, 0, 2, 1),
            spec("b", 0, 2, 1, 1),
            spec("c", 1, 0, 3, 1),
        ];
        let rects = grid(Rect::new(0, 0, 90, 20), &boxes);
        assert_eq!(rects[0], Rect::new(0, 0, 60, 10));
        assert_eq!(rects[1], Rect::new(60, 0, 30, 10));
        assert_eq!(rects[2], Rect::new(0, 10, 90, 10));
    }

    #[test]
    fn grid_of_nothing_is_empty() {
        assert!(grid(Rect::new(0, 0, 10, 10), &[]).is_empty());
    }

    #[test]
    fn keep_visible_keeps_selection_in_window() {
        assert_eq!(keep_visible(0, 0, 5), 0);
        assert_eq!(keep_visible(0, 4, 5), 0);
        assert_eq!(keep_visible(0, 5, 5), 1);
        assert_eq!(keep_visible(0, 12, 5), 8);
        assert_eq!(keep_visible(10, 3, 5), 3);
        assert_eq!(keep_visible(3, 4, 0), 4);
    }

    #[test]
    fn fit_scroll_scrolls_selected_row_into_view() {
        let mut app = loaded_app();
        for (i, b) in app.boxes.iter_mut().enumerate() {
            b.spec.col = i as u16;
        }
        app.boxes[SOZLU].items = (0..30).map(|i| item(&i.to_string(), "öğe")).collect();
        app.boxes[SOZLU].selected = 20;
        let area = Rect::new(0, 0, 90, 12);
        fit_scroll(&mut app, area);
        let rects = frame_rects(area, &app);
        let list = list_area(rects[SOZLU], false);
        let scroll = app.boxes[SOZLU].scroll;
        assert!(scroll <= 20 && 20 < scroll + usize::from(list.height));
    }

    fn board_app() -> App {
        let mut app = loaded_app();
        for (i, b) in app.boxes.iter_mut().enumerate() {
            b.spec.col = i as u16;
        }
        app
    }

    #[test]
    fn hit_maps_rows_with_scroll_and_ignores_borders() {
        let mut app = board_app();
        app.boxes[SOZLU].scroll = 1;
        app.boxes[SOZLU].selected = 2;
        let area = Rect::new(0, 0, 90, 12);
        let rects = frame_rects(area, &app);
        let r = rects[SOZLU];
        let first_row = list_area(r, false).y;
        assert_eq!(
            hit(&rects, &app, r.x + 2, first_row),
            Some(Hit {
                box_idx: SOZLU,
                row: Some(1)
            })
        );
        assert_eq!(
            hit(&rects, &app, r.x + 2, first_row + 1),
            Some(Hit {
                box_idx: SOZLU,
                row: Some(2)
            })
        );
        assert_eq!(
            hit(&rects, &app, r.x + 2, r.y),
            Some(Hit {
                box_idx: SOZLU,
                row: None
            })
        );
        assert_eq!(hit(&rects, &app, 0, 0), None);
    }

    #[test]
    fn hit_in_zoom_only_sees_the_zoomed_box() {
        let mut app = board_app();
        app.mode = Mode::Zoom(REDMINE);
        let area = Rect::new(0, 0, 90, 12);
        let rects = frame_rects(area, &app);
        let h = hit(&rects, &app, 3, 3).unwrap();
        assert_eq!(h.box_idx, REDMINE);
        assert_eq!(hit(&rects, &app, 3, 11), None);
    }

    #[test]
    fn error_row_shifts_list_area_down() {
        let r = Rect::new(0, 0, 30, 10);
        assert_eq!(list_area(r, false), Rect::new(1, 1, 28, 8));
        assert_eq!(list_area(r, true), Rect::new(1, 2, 28, 7));
    }
}

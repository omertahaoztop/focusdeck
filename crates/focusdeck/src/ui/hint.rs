use super::app::{App, BoxState, DropPlan, Mode};
use crate::i18n::{msg, t, t_args};

fn has_action(state: &BoxState, action: &str) -> bool {
    state
        .describe
        .as_ref()
        .is_some_and(|d| d.actions.iter().any(|a| a == action))
}

fn join(parts: Vec<String>) -> String {
    parts.join(" · ")
}

pub fn text(app: &App) -> String {
    if let Some(drag) = app.drag() {
        return dragging(app, drag.plan);
    }
    match &app.mode {
        Mode::Board => board(app),
        Mode::Zoom(idx) => zoom(app, *idx),
        Mode::Capture { .. } => t(msg::HINT_CAPTURE),
        Mode::MovePicker { .. } => t(msg::HINT_PICKER),
        Mode::Search { .. } => t(msg::HINT_SEARCH),
        Mode::Help => t(msg::HINT_HELP),
    }
}

fn dragging(app: &App, plan: DropPlan) -> String {
    let title = |idx: usize| app.boxes[idx].spec.title.as_str();
    let what = match plan {
        DropPlan::Into(idx) => t_args(msg::DRAG_INTO, &[("title", title(idx).into())]),
        DropPlan::Reorder { row, .. } => t_args(msg::DRAG_REORDER, &[("row", (row + 1).into())]),
        DropPlan::Rejected(idx) => t_args(msg::DRAG_REJECTED, &[("title", title(idx).into())]),
        DropPlan::Nothing => t(msg::DRAG_NOTHING),
    };
    t_args(msg::DRAG_CANCEL, &[("what", what.as_str().into())])
}

fn action_hints(state: &BoxState, parts: &mut Vec<String>) {
    if has_action(state, "done") {
        parts.push(t(msg::HINT_ACTION_DONE));
    }
    if has_action(state, "unstar") {
        parts.push(t(msg::HINT_ACTION_UNSTAR));
    }
}

fn board(app: &App) -> String {
    let mut parts = vec![
        t(msg::HINT_BOARD_NOTE),
        t(msg::HINT_BOARD_SELECT),
        t(msg::HINT_BOARD_ZOOM),
        t(msg::HINT_BOARD_DRAG),
    ];
    if let Some(state) = app.boxes.get(app.focus) {
        action_hints(state, &mut parts);
    }
    parts.extend([
        t(msg::HINT_OPEN),
        t(msg::HINT_MOVE),
        t(msg::HINT_BOARD_BOX),
        t(msg::HINT_BOARD_HELP),
        t(msg::HINT_BOARD_QUIT),
    ]);
    join(parts)
}

fn zoom(app: &App, idx: usize) -> String {
    let mut parts = Vec::new();
    if let Some(state) = app.boxes.get(idx) {
        if state.spec.plan_into.is_some() {
            parts.push(t(msg::HINT_ZOOM_PLAN));
        }
        action_hints(state, &mut parts);
    }
    parts.extend([
        t(msg::HINT_OPEN),
        t(msg::HINT_ZOOM_SEARCH),
        t(msg::HINT_MOVE),
        t(msg::HINT_ZOOM_BACK),
    ]);
    join(parts)
}

#[cfg(test)]
mod tests {
    use super::super::app::Hit;
    use super::super::app::testkit::*;
    use super::*;
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    use std::time::Instant;

    const TAIL: &str = "o open · m move · Tab box · ? help · q quit";

    #[test]
    fn board_hint_per_focused_box() {
        let mut app = loaded_app();
        app.focus = SOZLU;
        assert_eq!(
            text(&app),
            format!("a note · click select · double click zoom · drag move · x done · {TAIL}")
        );
        app.focus = REDMINE;
        assert_eq!(
            text(&app),
            format!("a note · click select · double click zoom · drag move · {TAIL}")
        );
        app.focus = ZULIP;
        assert_eq!(
            text(&app),
            format!("a note · click select · double click zoom · drag move · u unstar · {TAIL}")
        );
    }

    #[test]
    fn zoom_hint_shows_plan_key_only_for_plan_into_box() {
        let mut app = loaded_app();
        app.mode = Mode::Zoom(REDMINE);
        assert_eq!(
            text(&app),
            "space add to plan · o open · / search · m move · Esc back"
        );
        app.mode = Mode::Zoom(ZULIP);
        assert_eq!(
            text(&app),
            "u unstar · o open · / search · m move · Esc back"
        );
    }

    #[test]
    fn modal_hints() {
        let mut app = loaded_app();
        app.mode = Mode::Capture {
            input: String::new(),
        };
        assert_eq!(
            text(&app),
            "Enter save · Esc cancel · @person #label R123 · at the end 14:30, tomorrow 09:00, 30min"
        );
        app.mode = Mode::MovePicker {
            from: ZULIP,
            item: "z1".into(),
        };
        assert_eq!(text(&app), "1-9 target box · Esc cancel");
        app.mode = Mode::Search {
            zoom: REDMINE,
            query: String::new(),
        };
        assert_eq!(text(&app), "type to search · Enter done · Esc clear");
    }

    #[test]
    fn hint_before_describe_has_no_action_keys() {
        let app = App::new(&profile());
        assert_eq!(
            text(&app),
            format!("a note · click select · double click zoom · drag move · {TAIL}")
        );
    }

    fn hover(app: &mut App, kind: MouseEventKind, at: Hit) {
        let ev = MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        app.on_mouse(ev, Instant::now(), |_, _| Some(at));
    }

    #[test]
    fn drag_hint_says_what_release_does() {
        let mut app = loaded_app();
        let press = MouseEventKind::Down(MouseButton::Left);
        let drag = MouseEventKind::Drag(MouseButton::Left);
        hover(
            &mut app,
            press,
            Hit {
                box_idx: ZULIP,
                row: Some(0),
            },
        );
        hover(
            &mut app,
            drag,
            Hit {
                box_idx: SOZLU,
                row: Some(1),
            },
        );
        assert_eq!(text(&app), "drop: sozlu · Esc cancel");
        hover(
            &mut app,
            drag,
            Hit {
                box_idx: REDMINE,
                row: None,
            },
        );
        assert_eq!(text(&app), "redmine does not accept · Esc cancel");
        hover(
            &mut app,
            drag,
            Hit {
                box_idx: ZULIP,
                row: Some(1),
            },
        );
        assert_eq!(text(&app), "nothing happens if you drop · Esc cancel");

        let mut app = loaded_app();
        hover(
            &mut app,
            press,
            Hit {
                box_idx: SOZLU,
                row: Some(0),
            },
        );
        hover(
            &mut app,
            drag,
            Hit {
                box_idx: SOZLU,
                row: Some(2),
            },
        );
        assert_eq!(text(&app), "drop: row 3 · Esc cancel");
    }
}

use super::app::{App, BoxState, Mode};

fn has_action(state: &BoxState, action: &str) -> bool {
    state
        .describe
        .as_ref()
        .is_some_and(|d| d.actions.iter().any(|a| a == action))
}

fn join(parts: Vec<&str>) -> String {
    parts.join(" · ")
}

pub fn text(app: &App) -> String {
    match &app.mode {
        Mode::Board => board(app),
        Mode::Zoom(idx) => zoom(app, *idx),
        Mode::Capture { .. } => {
            "Enter kaydet · Esc vazgeç · @kişi #etiket R123 · sonda 14:30, yarın 09:00, 30dk".into()
        }
        Mode::MovePicker { .. } => "1-9 hedef kutu · Esc vazgeç".into(),
        Mode::Search { .. } => "yaz ara · Enter tamam · Esc temizle".into(),
        Mode::Help => "Esc kapat".into(),
    }
}

fn action_hints(state: &BoxState, parts: &mut Vec<&'static str>) {
    if has_action(state, "done") {
        parts.push("x bitti");
    }
    if has_action(state, "unstar") {
        parts.push("u yıldızı kaldır");
    }
}

fn board(app: &App) -> String {
    let mut parts = vec!["a not al", "tıkla seç", "çift tıkla büyüt", "sürükle taşı"];
    if let Some(state) = app.boxes.get(app.focus) {
        action_hints(state, &mut parts);
    }
    parts.extend(["o aç", "m taşı", "Tab kutu", "? yardım", "q çık"]);
    join(parts)
}

fn zoom(app: &App, idx: usize) -> String {
    let mut parts = Vec::new();
    if let Some(state) = app.boxes.get(idx) {
        if state.spec.plan_into.is_some() {
            parts.push("boşluk planıma al");
        }
        action_hints(state, &mut parts);
    }
    parts.extend(["o aç", "/ ara", "m taşı", "Esc geri"]);
    join(parts)
}

#[cfg(test)]
mod tests {
    use super::super::app::testkit::*;
    use super::*;

    const TAIL: &str = "o aç · m taşı · Tab kutu · ? yardım · q çık";

    #[test]
    fn board_hint_per_focused_box() {
        let mut app = loaded_app();
        app.focus = SOZLU;
        assert_eq!(
            text(&app),
            format!("a not al · tıkla seç · çift tıkla büyüt · sürükle taşı · x bitti · {TAIL}")
        );
        app.focus = REDMINE;
        assert_eq!(
            text(&app),
            format!("a not al · tıkla seç · çift tıkla büyüt · sürükle taşı · {TAIL}")
        );
        app.focus = ZULIP;
        assert_eq!(
            text(&app),
            format!(
                "a not al · tıkla seç · çift tıkla büyüt · sürükle taşı · u yıldızı kaldır · {TAIL}"
            )
        );
    }

    #[test]
    fn zoom_hint_shows_plan_key_only_for_plan_into_box() {
        let mut app = loaded_app();
        app.mode = Mode::Zoom(REDMINE);
        assert_eq!(
            text(&app),
            "boşluk planıma al · o aç · / ara · m taşı · Esc geri"
        );
        app.mode = Mode::Zoom(ZULIP);
        assert_eq!(
            text(&app),
            "u yıldızı kaldır · o aç · / ara · m taşı · Esc geri"
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
            "Enter kaydet · Esc vazgeç · @kişi #etiket R123 · sonda 14:30, yarın 09:00, 30dk"
        );
        app.mode = Mode::MovePicker {
            from: ZULIP,
            item: "z1".into(),
        };
        assert_eq!(text(&app), "1-9 hedef kutu · Esc vazgeç");
        app.mode = Mode::Search {
            zoom: REDMINE,
            query: String::new(),
        };
        assert_eq!(text(&app), "yaz ara · Enter tamam · Esc temizle");
    }

    #[test]
    fn hint_before_describe_has_no_action_keys() {
        let app = App::new(&profile());
        assert_eq!(
            text(&app),
            format!("a not al · tıkla seç · çift tıkla büyüt · sürükle taşı · {TAIL}")
        );
    }
}

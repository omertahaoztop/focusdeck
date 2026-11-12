use std::collections::HashMap;
use std::fs;

use focusdeck_i18n::{Localizer, Sources, check_locales, normalize, requested};

const SOURCES: Sources = &[
    ("en", include_str!("fixtures/en.ftl")),
    ("tr", include_str!("fixtures/tr.ftl")),
    ("broken", include_str!("fixtures/broken.ftl")),
];

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |name| map.get(name).cloned()
}

#[test]
fn normalize_maps_posix_locales() {
    assert_eq!(normalize("tr_TR.UTF-8").as_deref(), Some("tr-TR"));
    assert_eq!(normalize("de_DE@euro").as_deref(), Some("de-DE"));
    assert_eq!(normalize("tr").as_deref(), Some("tr"));
    assert_eq!(normalize("pt-BR").as_deref(), Some("pt-BR"));
    for raw in ["", "C", "C.UTF-8", "POSIX"] {
        assert_eq!(normalize(raw), None, "{raw:?}");
    }
}

#[test]
fn requested_prefers_config_then_env_order() {
    let vars = ["LC_ALL", "LC_MESSAGES", "LANG"];

    let got = requested(Some("tr"), env(&[("LANG", "de_DE.UTF-8")]), &vars);
    assert_eq!(got.as_deref(), Some("tr"));

    let got = requested(None, env(&[("LANG", "tr_TR.UTF-8")]), &vars);
    assert_eq!(got.as_deref(), Some("tr-TR"));

    let got = requested(
        None,
        env(&[("LC_ALL", "C"), ("LANG", "tr_TR.UTF-8")]),
        &vars,
    );
    assert_eq!(got.as_deref(), Some("tr-TR"));

    assert_eq!(requested(None, env(&[]), &vars), None);
}

#[test]
fn falls_back_to_language_then_english() {
    assert_eq!(Localizer::new(Some("tr-TR"), SOURCES).lang(), "tr");
    assert_eq!(Localizer::new(Some("pt-BR"), SOURCES).lang(), "en");
    assert_eq!(Localizer::new(None, SOURCES).lang(), "en");
}

#[test]
fn missing_message_uses_english() {
    let loc = Localizer::new(Some("tr"), SOURCES);
    assert_eq!(loc.t("hello"), "Merhaba");
    assert_eq!(loc.t("only-en"), "Only in English");
}

#[test]
fn plural_follows_language() {
    let loc = Localizer::new(Some("en"), SOURCES);
    assert_eq!(loc.t_args("items", &[("count", 1.into())]), "one item");
    let five = loc.t_args("items", &[("count", 5.into())]);
    assert_eq!(five, "5 items");
    assert!(!five.contains('\u{2068}') && !five.contains('\u{2069}'));
}

#[test]
fn words_split_and_lowercase() {
    let loc = Localizer::new(Some("tr"), SOURCES);
    assert_eq!(loc.words("capture-words-tomorrow"), ["yarın", "yarin"]);
}

#[test]
fn broken_file_keeps_valid_messages() {
    let loc = Localizer::new(Some("broken"), SOURCES);
    assert_eq!(loc.t("good"), "Broken good");
    assert_eq!(loc.t("hello"), "Broken hello");
    assert_eq!(loc.t("bad"), "English bad");
}

#[test]
fn message_with_resolver_errors_falls_back_to_next_language() {
    let loc = Localizer::new(Some("tr"), SOURCES);
    assert_eq!(loc.t("term-ref"), "English term");
}

fn write_locales(dir: &std::path::Path, files: &[(&str, &str)]) {
    for (name, body) in files {
        fs::write(dir.join(name), body).unwrap();
    }
}

#[test]
fn check_locales_accepts_matching_files() {
    let dir = tempfile::tempdir().unwrap();
    write_locales(
        dir.path(),
        &[
            ("en.ftl", "a = A { $n }\n-brand = Home\nb = B\n"),
            ("tr.ftl", "b = BB\na = AA { $n }\n-brand = Ev\n"),
        ],
    );
    assert_eq!(check_locales(dir.path()), Ok(()));
}

#[test]
fn check_locales_reports_missing_extra_variable_and_syntax() {
    let dir = tempfile::tempdir().unwrap();
    write_locales(
        dir.path(),
        &[
            (
                "en.ftl",
                "a = A\nb = B { $n }\nc =\n    .title = { $who ->\n        [x] X\n       *[y] Y\n    }\n",
            ),
            (
                "missing.ftl",
                "a = A\nc =\n    .title = { $who ->\n        [x] X\n       *[y] Y\n    }\n",
            ),
            (
                "extra.ftl",
                "a = A\nb = B { $n }\nz = Z\nc =\n    .title = { $who ->\n        [x] X\n       *[y] Y\n    }\n",
            ),
            (
                "vars.ftl",
                "a = A\nb = B { $count }\nc =\n    .title = Fixed\n",
            ),
            ("syntax.ftl", "a = A\nb = { $n\nc = C\n"),
        ],
    );
    let err = check_locales(dir.path()).unwrap_err();
    let lines: Vec<&str> = err.lines().collect();
    for expected in [
        "missing.ftl: missing message `b`",
        "extra.ftl: extra message `z`",
        "vars.ftl: message `b` variables {count} differ from en.ftl {n}",
        "vars.ftl: message `c` variables {} differ from en.ftl {who}",
    ] {
        assert!(lines.contains(&expected), "{expected:?} not in:\n{err}");
    }
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("syntax.ftl:2: syntax error")),
        "{err}"
    );
}

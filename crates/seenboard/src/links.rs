use std::collections::HashSet;

use seenboard_protocol::Item;

fn linked_to<'a>(item: &'a Item, source_plugin: &str) -> Option<&'a str> {
    item.source
        .as_ref()
        .filter(|s| s.plugin == source_plugin)
        .map(|s| s.id.as_str())
}

pub fn linked_ids(target_items: &[Item], source_plugin: &str) -> HashSet<String> {
    target_items
        .iter()
        .filter_map(|i| linked_to(i, source_plugin))
        .map(str::to_owned)
        .collect()
}

pub fn linked_item<'a>(
    target_items: &'a [Item],
    source_plugin: &str,
    source_id: &str,
) -> Option<&'a Item> {
    target_items
        .iter()
        .find(|i| linked_to(i, source_plugin) == Some(source_id))
}

/// Çağıran yalnız başarılı Full liste ile çağırır; boş başarılı liste bağlı her öğeyi kapatır.
pub fn items_to_close(
    source_plugin: &str,
    source_items: &[Item],
    target_items: &[Item],
) -> Vec<String> {
    let present: HashSet<&str> = source_items.iter().map(|i| i.id.as_str()).collect();
    target_items
        .iter()
        .filter(|i| linked_to(i, source_plugin).is_some_and(|id| !present.contains(id)))
        .map(|i| i.id.clone())
        .collect()
}

const EMPTY_LISTS_TO_CLOSE: u8 = 2;

/// Counts consecutive empty lists in `empty_lists`; one empty list may be a glitch, so only the second closes.
pub fn closures_after_list(
    empty_lists: &mut u8,
    source_plugin: &str,
    source_items: &[Item],
    target_items: &[Item],
) -> Vec<String> {
    if source_items.is_empty() {
        *empty_lists = empty_lists.saturating_add(1);
        if *empty_lists < EMPTY_LISTS_TO_CLOSE {
            return Vec::new();
        }
    } else {
        *empty_lists = 0;
    }
    items_to_close(source_plugin, source_items, target_items)
}

/// Only web and mail links go to the desktop opener, so a stored URL cannot smuggle in options or files.
pub fn openable(url: &str) -> bool {
    ["https://", "http://", "mailto:"].iter().any(|scheme| {
        url.get(..scheme.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(scheme))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use seenboard_protocol::{Item, Source};

    fn item(id: &str, source: Option<(&str, &str)>) -> Item {
        Item {
            id: id.into(),
            title: format!("öğe {id}"),
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

    fn source_list(ids: &[&str]) -> Vec<Item> {
        ids.iter().map(|id| item(id, None)).collect()
    }

    #[test]
    fn linked_ids_filters_by_plugin() {
        let target = vec![
            item("a", Some(("redmine", "1"))),
            item("b", Some(("redmine", "2"))),
            item("c", Some(("zulip", "9"))),
            item("d", None),
        ];
        let ids = linked_ids(&target, "redmine");
        assert_eq!(ids.len(), 2);
        assert!(ids.contains("1") && ids.contains("2"));
    }

    #[test]
    fn linked_item_matches_plugin_and_id() {
        let target = vec![
            item("a", Some(("zulip", "2"))),
            item("b", Some(("redmine", "2"))),
        ];
        assert_eq!(linked_item(&target, "redmine", "2").unwrap().id, "b");
        assert!(linked_item(&target, "redmine", "3").is_none());
        assert!(linked_item(&target, "gmail", "2").is_none());
    }

    #[test]
    fn closes_linked_item_missing_from_source() {
        let target = vec![
            item("a", Some(("redmine", "1"))),
            item("b", Some(("redmine", "2"))),
            item("c", None),
        ];
        assert_eq!(
            items_to_close("redmine", &source_list(&["1"]), &target),
            vec!["b".to_string()]
        );
    }

    #[test]
    fn empty_source_and_no_linked_items_closes_nothing() {
        let target = vec![item("c", None), item("z", Some(("zulip", "5")))];
        assert!(items_to_close("redmine", &[], &target).is_empty());
    }

    #[test]
    fn empty_successful_source_closes_every_linked_item() {
        let target = vec![item("a", Some(("redmine", "1"))), item("c", None)];
        assert_eq!(
            items_to_close("redmine", &[], &target),
            vec!["a".to_string()]
        );
    }

    #[test]
    fn other_plugin_links_never_close() {
        let target = vec![item("z", Some(("zulip", "5")))];
        assert!(items_to_close("redmine", &source_list(&["1"]), &target).is_empty());
    }

    #[test]
    fn matching_is_exact_not_prefix_or_substring() {
        let target = vec![
            item("a", Some(("redmine", "12"))),
            item("b", Some(("redmine", "1"))),
            item("c", Some(("redmine2", "7"))),
        ];
        assert_eq!(
            items_to_close("redmine", &source_list(&["1"]), &target),
            vec!["a".to_string()]
        );
    }

    #[test]
    fn source_id_is_taken_from_item_id_and_order_is_kept() {
        let target = vec![
            item("t3", Some(("redmine", "3"))),
            item("t1", Some(("redmine", "1"))),
            item("t2", Some(("redmine", "2"))),
        ];
        assert_eq!(
            items_to_close("redmine", &source_list(&["2"]), &target),
            vec!["t3".to_string(), "t1".to_string()]
        );
    }

    #[test]
    fn one_empty_list_closes_nothing() {
        let target = vec![item("a", Some(("redmine", "1")))];
        let mut empty_lists = 0;
        assert!(closures_after_list(&mut empty_lists, "redmine", &[], &target).is_empty());
        assert_eq!(empty_lists, 1);
    }

    #[test]
    fn two_empty_lists_close_the_linked_items() {
        let target = vec![item("a", Some(("redmine", "1"))), item("c", None)];
        let mut empty_lists = 0;
        closures_after_list(&mut empty_lists, "redmine", &[], &target);
        assert_eq!(
            closures_after_list(&mut empty_lists, "redmine", &[], &target),
            vec!["a".to_string()]
        );
    }

    #[test]
    fn a_non_empty_list_resets_the_empty_count() {
        let target = vec![item("a", Some(("redmine", "1")))];
        let mut empty_lists = 0;
        closures_after_list(&mut empty_lists, "redmine", &[], &target);
        let kept = source_list(&["1"]);
        assert!(closures_after_list(&mut empty_lists, "redmine", &kept, &target).is_empty());
        assert_eq!(empty_lists, 0);
        assert!(closures_after_list(&mut empty_lists, "redmine", &[], &target).is_empty());
    }

    #[test]
    fn a_non_empty_list_still_closes_what_left() {
        let target = vec![
            item("a", Some(("redmine", "1"))),
            item("b", Some(("redmine", "2"))),
        ];
        let mut empty_lists = 0;
        assert_eq!(
            closures_after_list(&mut empty_lists, "redmine", &source_list(&["1"]), &target),
            vec!["b".to_string()]
        );
    }

    #[test]
    fn only_web_and_mail_links_are_openable() {
        for (url, expected) in [
            ("https://example.test/a", true),
            ("http://example.test", true),
            ("HTTPS://EXAMPLE.TEST", true),
            ("MailTo:ada@example.test", true),
            ("file:///etc/passwd", false),
            ("javascript:alert(1)", false),
            ("-oProxyCommand=x", false),
            (" https://a", false),
            ("https:/", false),
            ("ftp://example.test", false),
            ("", false),
            ("é", false),
        ] {
            assert_eq!(openable(url), expected, "{url}");
        }
    }
}

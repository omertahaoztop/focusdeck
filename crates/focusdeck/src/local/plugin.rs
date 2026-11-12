//! The built-in `local` box: notes kept in the store file, served over the plugin protocol.

use std::io;

use chrono::{DateTime, FixedOffset};
use focusdeck_protocol::{
    ActionParams, Describe, DropParams, Item, ListParams, PROTOCOL_VERSION, Plugin, Reminder,
    RemindersParams,
};
use focusdeck_i18n::Localizer;
use serde_json::Value;

use super::store::{Record, Store, StoreError, new_id};
use crate::i18n::{self, msg, t, t_args};
use crate::paths::Paths;

const POSITION_STEP: f64 = 1024.0;

pub struct Local {
    store: Store,
    list: Option<String>,
}

fn now() -> DateTime<FixedOffset> {
    chrono::Local::now().fixed_offset()
}

fn flatten<T>(outcome: Result<Result<T, String>, StoreError>) -> Result<T, String> {
    outcome.map_err(|e| e.to_string())?
}

fn invalid_settings(l: &Localizer) -> String {
    let hint = l.t(msg::LOCAL_LIST_REQUIRED);
    l.t_args(msg::LOCAL_INVALID_SETTINGS, &[("error", hint.as_str().into())])
}

fn item_missing(id: &str) -> String {
    t_args(msg::LOCAL_ITEM_MISSING, &[("id", id.into())])
}

fn is_open_in(record: &Record, list: &str) -> bool {
    record.list == list && record.done_at.is_none()
}

fn open_in<'a>(items: &'a [Record], list: &'a str) -> impl Iterator<Item = &'a Record> {
    items.iter().filter(move |r| is_open_in(r, list))
}

fn in_order<'a>(items: &'a [Record], list: &'a str) -> Vec<&'a Record> {
    let mut open: Vec<&Record> = open_in(items, list).collect();
    open.sort_by(|a, b| {
        a.position
            .total_cmp(&b.position)
            .then_with(|| a.created.cmp(&b.created))
    });
    open
}

fn last_position(items: &[Record], list: &str) -> f64 {
    open_in(items, list)
        .map(|r| r.position)
        .max_by(f64::total_cmp)
        .unwrap_or(0.0)
}

fn to_item(record: &Record, now: DateTime<FixedOffset>) -> Item {
    Item {
        id: record.id.clone(),
        title: record.title.clone(),
        subtitle: None,
        url: None,
        due: record.due,
        overdue: record.due.is_some_and(|due| due < now),
        source: record.source.clone(),
        description: record.description.clone(),
        labels: record.labels.clone(),
        from: record.from.clone(),
        links: record.links.clone(),
    }
}

fn place_between(others: &[f64], wanted: usize) -> f64 {
    match (others.first(), others.last()) {
        (None, _) | (_, None) => POSITION_STEP,
        (Some(first), _) if wanted == 0 => {
            if *first > 0.0 {
                first / 2.0
            } else {
                first - POSITION_STEP
            }
        }
        (_, Some(last)) if wanted >= others.len() => last + POSITION_STEP,
        _ => (others[wanted - 1] + others[wanted]) / 2.0,
    }
}

impl Local {
    pub fn new(store: Store) -> Local {
        Local { store, list: None }
    }

    fn list_name(&self) -> Result<&str, String> {
        self.list
            .as_deref()
            .ok_or_else(|| invalid_settings(i18n::current()))
    }

    fn reorder(&self, id: &str, args: &Value) -> Result<(), String> {
        let list = self.list_name()?;
        let wanted = args
            .get("position")
            .and_then(Value::as_u64)
            .ok_or_else(|| t(msg::LOCAL_POSITION_REQUIRED))?;
        let wanted = usize::try_from(wanted).unwrap_or(usize::MAX);
        flatten(self.store.update(|items| {
            let index = items
                .iter()
                .position(|r| r.id == id && is_open_in(r, list))
                .ok_or_else(|| item_missing(id))?;
            let others: Vec<f64> = in_order(items, list)
                .into_iter()
                .filter(|r| r.id != id)
                .map(|r| r.position)
                .collect();
            items[index].position = place_between(&others, wanted);
            Ok(())
        }))
    }
}

impl Plugin for Local {
    fn describe(&self) -> Describe {
        Describe {
            name: "local".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            protocol: PROTOCOL_VERSION,
            actions: ["done", "delete", "move", "reorder"]
                .map(String::from)
                .to_vec(),
            accepts_drop: true,
            reminders: true,
        }
    }

    fn configure(&mut self, config: Value) -> Result<(), String> {
        let list = config
            .get("list")
            .and_then(Value::as_str)
            .filter(|l| !l.is_empty())
            .ok_or_else(|| invalid_settings(i18n::current()))?;
        self.list = Some(list.to_string());
        Ok(())
    }

    fn list(&mut self, _p: ListParams) -> Result<Vec<Item>, String> {
        let list = self.list_name()?;
        let items = self.store.read().map_err(|e| e.to_string())?;
        let now = now();
        Ok(in_order(&items, list)
            .into_iter()
            .map(|r| to_item(r, now))
            .collect())
    }

    fn action(&mut self, p: ActionParams) -> Result<Option<Item>, String> {
        let id = p.item.as_str();
        match p.action.as_str() {
            "done" | "delete" => {
                let list = self.list_name()?;
                let done = p.action == "done";
                flatten(self.store.update(|items| {
                    let index = items
                        .iter()
                        .position(|r| r.id == id && is_open_in(r, list))
                        .ok_or_else(|| item_missing(id))?;
                    if done {
                        items[index].done_at = Some(now());
                    } else {
                        items.remove(index);
                    }
                    Ok(())
                }))?;
            }
            "reorder" | "move" => self.reorder(id, &p.args)?,
            other => {
                return Err(t_args(
                    msg::LOCAL_UNKNOWN_ACTION,
                    &[("action", other.into())],
                ));
            }
        }
        Ok(None)
    }

    fn drop_item(&mut self, p: DropParams) -> Result<Item, String> {
        let list = self.list_name()?;
        let now = now();
        flatten(self.store.update(|items| {
            let position = last_position(items, list) + POSITION_STEP;
            let local_origin = p
                .from
                .as_ref()
                .filter(|s| s.plugin == "local")
                .and_then(|s| items.iter().position(|r| r.id == s.id));
            if let Some(index) = local_origin {
                let record = &mut items[index];
                record.list = list.to_string();
                record.position = position;
                return Ok(to_item(record, now));
            }
            let item = p.item;
            let record = Record {
                id: new_id(items),
                list: list.to_string(),
                title: item.title,
                description: item.description,
                due: item.due,
                labels: item.labels,
                from: item.from,
                links: item.links,
                source: item.source.or(p.from),
                position,
                created: now,
                done_at: None,
                extra: serde_json::Map::new(),
            };
            let made = to_item(&record, now);
            items.push(record);
            Ok(made)
        }))
    }

    fn reminders(&mut self, p: RemindersParams) -> Result<Vec<Reminder>, String> {
        let list = self.list_name()?;
        let items = self.store.read().map_err(|e| e.to_string())?;
        let now = now();
        let mut found: Vec<Reminder> = open_in(&items, list)
            .filter_map(|r| {
                let at = r.due.filter(|due| *due > p.since)?;
                Some(Reminder {
                    at,
                    item: to_item(r, now),
                })
            })
            .collect();
        found.sort_by_key(|r| r.at);
        Ok(found)
    }
}

pub fn serve_stdio() -> io::Result<()> {
    let plugin = Local::new(Store::from_paths(&Paths::from_env()));
    focusdeck_protocol::serve(plugin, io::stdin().lock(), io::stdout().lock())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::store::Record;
    use focusdeck_protocol::{Link, Source, View};
    use serde_json::json;

    fn at(raw: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(raw).unwrap()
    }

    fn record(id: &str, list: &str, position: f64) -> Record {
        Record {
            id: id.into(),
            list: list.into(),
            title: format!("note {id}"),
            description: None,
            due: None,
            labels: vec![],
            from: None,
            links: vec![],
            source: None,
            position,
            created: at("2020-01-01T00:00:00+00:00"),
            done_at: None,
            extra: serde_json::Map::new(),
        }
    }

    fn seeded(records: Vec<Record>) -> (tempfile::TempDir, Local) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path().join("local.json"));
        store
            .update(|items| {
                items.extend(records);
                Ok(())
            })
            .unwrap()
            .unwrap();
        let mut local = Local::new(Store::new(dir.path().join("local.json")));
        local.configure(json!({"list": "ev"})).unwrap();
        (dir, local)
    }

    fn stored(dir: &tempfile::TempDir) -> Vec<Record> {
        Store::new(dir.path().join("local.json")).read().unwrap()
    }

    fn summary() -> ListParams {
        ListParams {
            view: View::Summary,
        }
    }

    fn ids(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.id.as_str()).collect()
    }

    fn act(item: &str, action: &str, args: Value) -> ActionParams {
        ActionParams {
            item: item.into(),
            action: action.into(),
            args,
        }
    }

    fn captured(title: &str) -> Item {
        Item {
            id: String::new(),
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

    #[test]
    fn describe_offers_move_drop_and_reminders() {
        let (_dir, local) = seeded(vec![]);
        let d = local.describe();
        assert_eq!(d.name, "local");
        assert_eq!(d.protocol, PROTOCOL_VERSION);
        assert_eq!(d.actions, ["done", "delete", "move", "reorder"]);
        assert!(d.accepts_drop);
        assert!(d.reminders);
    }

    #[test]
    fn configure_requires_a_list() {
        let dir = tempfile::tempdir().unwrap();
        let mut local = Local::new(Store::new(dir.path().join("local.json")));
        for bad in [
            json!({}),
            json!({"list": ""}),
            json!({"list": 3}),
            json!(null),
        ] {
            let err = local.configure(bad).unwrap_err();
            assert!(err.starts_with("invalid local box settings"), "{err}");
        }
        assert!(local.list(summary()).is_err());
        let tr = i18n::localizer(Some("tr"));
        assert_eq!(
            invalid_settings(&tr),
            "yerel kutu ayarı geçersiz: `list` boş olmayan bir metin olmalı"
        );
        local.configure(json!({"list": "ev", "other": 1})).unwrap();
        assert_eq!(local.list(summary()).unwrap(), vec![]);
    }

    #[test]
    fn lists_only_open_items_of_its_own_list_in_position_order() {
        let mut late = record("late", "ev", 3.0);
        late.due = Some(at("2001-01-01T00:00:00+00:00"));
        let mut future = record("future", "ev", 1.0);
        future.due = Some(at("2999-01-01T00:00:00+00:00"));
        let mut finished = record("finished", "ev", 0.5);
        finished.done_at = Some(at("2020-02-01T00:00:00+00:00"));
        let (_dir, mut local) = seeded(vec![
            late,
            future,
            record("plain", "ev", 2.0),
            finished,
            record("other", "is", 0.1),
        ]);
        let items = local.list(summary()).unwrap();
        assert_eq!(ids(&items), ["future", "plain", "late"]);
        assert!(!items[0].overdue);
        assert!(!items[1].overdue);
        assert!(items[2].overdue);
        assert_eq!(items[1].subtitle, None);
        assert_eq!(items[1].url, None);
    }

    #[test]
    fn done_keeps_the_record_and_hides_it() {
        let (dir, mut local) = seeded(vec![record("a", "ev", 1.0), record("b", "ev", 2.0)]);
        assert_eq!(local.action(act("a", "done", Value::Null)).unwrap(), None);
        assert_eq!(ids(&local.list(summary()).unwrap()), ["b"]);
        let all = stored(&dir);
        assert_eq!(all.len(), 2);
        assert!(all.iter().find(|r| r.id == "a").unwrap().done_at.is_some());
        assert!(all.iter().find(|r| r.id == "b").unwrap().done_at.is_none());
    }

    #[test]
    fn delete_removes_the_record() {
        let (dir, mut local) = seeded(vec![record("a", "ev", 1.0), record("b", "ev", 2.0)]);
        assert_eq!(local.action(act("a", "delete", Value::Null)).unwrap(), None);
        let all = stored(&dir);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "b");
    }

    #[test]
    fn actions_reject_unknown_missing_and_foreign_items() {
        let (dir, mut local) = seeded(vec![record("a", "ev", 1.0), record("x", "is", 1.0)]);
        let err = local.action(act("a", "explode", Value::Null)).unwrap_err();
        assert_eq!(err, "unknown action: explode");
        let err = local.action(act("nope", "done", Value::Null)).unwrap_err();
        assert_eq!(err, "note nope is not in this box");
        let err = local.action(act("x", "delete", Value::Null)).unwrap_err();
        assert_eq!(err, "note x is not in this box");
        let err = local.action(act("a", "reorder", json!({}))).unwrap_err();
        assert_eq!(err, "a position is required");
        assert_eq!(stored(&dir).len(), 2);
        assert!(stored(&dir).iter().all(|r| r.done_at.is_none()));
    }

    #[test]
    fn reorder_moves_only_the_dragged_item() {
        let (dir, mut local) = seeded(vec![
            record("a", "ev", 1024.0),
            record("b", "ev", 2048.0),
            record("c", "ev", 3072.0),
        ]);
        local
            .action(act("c", "reorder", json!({"position": 0})))
            .unwrap();
        assert_eq!(ids(&local.list(summary()).unwrap()), ["c", "a", "b"]);
        let all = stored(&dir);
        let pos = |id: &str| all.iter().find(|r| r.id == id).unwrap().position;
        assert_eq!(pos("a"), 1024.0);
        assert_eq!(pos("b"), 2048.0);
        assert!(pos("c") < 1024.0);

        local
            .action(act("c", "move", json!({"position": 2})))
            .unwrap();
        assert_eq!(ids(&local.list(summary()).unwrap()), ["a", "b", "c"]);

        local
            .action(act("c", "reorder", json!({"position": 1})))
            .unwrap();
        assert_eq!(ids(&local.list(summary()).unwrap()), ["a", "c", "b"]);
        let all = stored(&dir);
        let pos = |id: &str| all.iter().find(|r| r.id == id).unwrap().position;
        assert_eq!(pos("c"), 1536.0);
    }

    #[test]
    fn reorder_ignores_other_lists_and_done_items() {
        let mut finished = record("d", "ev", 1500.0);
        finished.done_at = Some(at("2020-02-01T00:00:00+00:00"));
        let (dir, mut local) = seeded(vec![
            record("a", "ev", 1024.0),
            record("b", "ev", 2048.0),
            record("o", "is", 1100.0),
            finished,
        ]);
        local
            .action(act("b", "reorder", json!({"position": 1})))
            .unwrap();
        let all = stored(&dir);
        assert!(all.iter().find(|r| r.id == "b").unwrap().position > 1024.0);
        assert_eq!(all.iter().find(|r| r.id == "b").unwrap().position, 2048.0);
        local
            .action(act("a", "reorder", json!({"position": 5})))
            .unwrap();
        let all = stored(&dir);
        assert_eq!(all.iter().find(|r| r.id == "a").unwrap().position, 3072.0);
    }

    #[test]
    fn reorder_in_a_single_item_list_keeps_it_listed() {
        let (_dir, mut local) = seeded(vec![record("a", "ev", 7.0)]);
        local
            .action(act("a", "reorder", json!({"position": 0})))
            .unwrap();
        assert_eq!(ids(&local.list(summary()).unwrap()), ["a"]);
    }

    #[test]
    fn drop_from_another_plugin_creates_with_source() {
        let (dir, mut local) = seeded(vec![record("a", "ev", 1024.0), record("o", "is", 9999.0)]);
        let from = Source {
            plugin: "redmine".into(),
            id: "12345".into(),
        };
        let item = local
            .drop_item(DropParams {
                item: captured("call back"),
                from: Some(from.clone()),
            })
            .unwrap();
        assert_eq!(item.title, "call back");
        assert_eq!(item.source, Some(from.clone()));
        let all = stored(&dir);
        assert_eq!(all.len(), 3);
        let created = all.iter().find(|r| r.id == item.id).unwrap();
        assert_eq!(created.source, Some(from));
        assert_eq!(created.list, "ev");
        assert_eq!(created.position, 2048.0);
        assert!(created.done_at.is_none());
        assert_eq!(
            ids(&local.list(summary()).unwrap()),
            ["a", item.id.as_str()]
        );
    }

    #[test]
    fn drop_into_an_empty_list_starts_after_zero() {
        let (dir, mut local) = seeded(vec![]);
        let item = local
            .drop_item(DropParams {
                item: captured("first"),
                from: None,
            })
            .unwrap();
        let all = stored(&dir);
        assert_eq!(all[0].id, item.id);
        assert_eq!(all[0].position, 1024.0);
        assert_eq!(all[0].source, None);
    }

    #[test]
    fn item_source_wins_over_the_drag_origin() {
        let (dir, mut local) = seeded(vec![]);
        let mut captured = captured("x");
        captured.source = Some(Source {
            plugin: "mail".into(),
            id: "m1".into(),
        });
        local
            .drop_item(DropParams {
                item: captured,
                from: Some(Source {
                    plugin: "redmine".into(),
                    id: "12345".into(),
                }),
            })
            .unwrap();
        assert_eq!(stored(&dir)[0].source.as_ref().unwrap().plugin, "mail");
    }

    #[test]
    fn drop_from_local_moves_without_copying() {
        let (dir, mut local) = seeded(vec![record("a", "ev", 1024.0), record("o", "is", 500.0)]);
        let item = local
            .drop_item(DropParams {
                item: captured("note a"),
                from: Some(Source {
                    plugin: "local".into(),
                    id: "a".into(),
                }),
            })
            .unwrap();
        assert_eq!(item.id, "a");
        let mut other = Local::new(Store::new(dir.path().join("local.json")));
        other.configure(json!({"list": "is"})).unwrap();
        let dropped = other
            .drop_item(DropParams {
                item: item.clone(),
                from: Some(Source {
                    plugin: "local".into(),
                    id: "a".into(),
                }),
            })
            .unwrap();
        assert_eq!(dropped.id, "a");
        let all = stored(&dir);
        assert_eq!(all.len(), 2);
        let moved = all.iter().find(|r| r.id == "a").unwrap();
        assert_eq!(moved.list, "is");
        assert_eq!(moved.position, 1524.0);
        assert_eq!(ids(&other.list(summary()).unwrap()), ["o", "a"]);
        assert_eq!(local.list(summary()).unwrap(), vec![]);
    }

    #[test]
    fn drop_from_local_with_an_unknown_id_creates_a_record() {
        let (dir, mut local) = seeded(vec![]);
        let item = local
            .drop_item(DropParams {
                item: captured("copy"),
                from: Some(Source {
                    plugin: "local".into(),
                    id: "gone".into(),
                }),
            })
            .unwrap();
        let all = stored(&dir);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, item.id);
        assert_ne!(all[0].id, "gone");
        assert_eq!(
            all[0].source,
            Some(Source {
                plugin: "local".into(),
                id: "gone".into()
            })
        );
    }

    #[test]
    fn capture_fields_survive() {
        let (dir, mut local) = seeded(vec![]);
        let mut item = captured("pay the bill");
        item.description = Some("account 12345".into());
        item.due = Some(at("2999-05-06T07:08:09+03:00"));
        item.labels = vec!["Home".into()];
        item.from = Some("Home".into());
        item.links = vec![Link {
            label: "bill".into(),
            url: "https://example.com/bill".into(),
        }];
        let made = local.drop_item(DropParams { item, from: None }).unwrap();
        let listed = local.list(summary()).unwrap();
        assert_eq!(listed.len(), 1);
        for shown in [&made, &listed[0]] {
            assert_eq!(shown.title, "pay the bill");
            assert_eq!(shown.description.as_deref(), Some("account 12345"));
            assert_eq!(shown.due, Some(at("2999-05-06T07:08:09+03:00")));
            assert_eq!(shown.labels, ["Home"]);
            assert_eq!(shown.from.as_deref(), Some("Home"));
            assert_eq!(shown.links[0].url, "https://example.com/bill");
            assert!(!shown.overdue);
        }
        let record = &stored(&dir)[0];
        assert_eq!(record.description.as_deref(), Some("account 12345"));
        assert_eq!(record.labels, ["Home"]);
        assert_eq!(record.from.as_deref(), Some("Home"));
        assert_eq!(record.links.len(), 1);
        assert!(record.created.timestamp() > 1_700_000_000);
    }

    #[test]
    fn reminders_fire_at_due_after_since() {
        let mut soon = record("soon", "ev", 1.0);
        soon.due = Some(at("2030-01-02T00:00:00+00:00"));
        let mut sooner = record("sooner", "ev", 2.0);
        sooner.due = Some(at("2030-01-01T00:00:00+00:00"));
        let mut earlier = record("earlier", "ev", 3.0);
        earlier.due = Some(at("2029-01-01T00:00:00+00:00"));
        let mut finished = record("finished", "ev", 4.0);
        finished.due = Some(at("2031-01-01T00:00:00+00:00"));
        finished.done_at = Some(at("2020-02-01T00:00:00+00:00"));
        let mut foreign = record("foreign", "is", 5.0);
        foreign.due = Some(at("2031-01-01T00:00:00+00:00"));
        let (_dir, mut local) = seeded(vec![
            soon,
            sooner,
            earlier,
            finished,
            foreign,
            record("undated", "ev", 6.0),
        ]);
        let found = local
            .reminders(RemindersParams {
                since: at("2029-06-01T00:00:00+00:00"),
            })
            .unwrap();
        let got: Vec<(&str, DateTime<FixedOffset>)> =
            found.iter().map(|r| (r.item.id.as_str(), r.at)).collect();
        assert_eq!(
            got,
            [
                ("sooner", at("2030-01-01T00:00:00+00:00")),
                ("soon", at("2030-01-02T00:00:00+00:00"))
            ]
        );
        let none = local
            .reminders(RemindersParams {
                since: at("2030-01-02T00:00:00+00:00"),
            })
            .unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn corrupt_store_turns_into_an_error_line() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("local.json");
        std::fs::write(&path, "{ not json").unwrap();
        let mut local = Local::new(Store::new(path.clone()));
        local.configure(json!({"list": "ev"})).unwrap();
        let err = local.list(summary()).unwrap_err();
        assert!(err.contains(&path.display().to_string()), "{err}");
        let err = local.action(act("a", "done", Value::Null)).unwrap_err();
        assert!(err.contains(&path.display().to_string()), "{err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
    }

    #[test]
    fn serves_the_protocol_end_to_end() {
        let (_dir, local) = seeded(vec![record("a", "ev", 1.0)]);
        let input = format!(
            "{}\n{}\n",
            json!({"id": 1, "method": "describe"}),
            json!({"id": 2, "method": "list", "params": {"view": "summary"}})
        );
        let mut out = Vec::new();
        focusdeck_protocol::serve(local, input.as_bytes(), &mut out).unwrap();
        let lines: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines[0]["result"]["name"], "local");
        assert_eq!(lines[1]["result"]["items"][0]["id"], "a");
    }
}

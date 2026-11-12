use std::io::Cursor;

use focusdeck_protocol::{
    ActionParams, Describe, Item, ListParams, PROTOCOL_VERSION, Plugin, Response, serve,
};
use serde_json::Value;

struct Deneme {
    list_error: Option<String>,
}

impl Plugin for Deneme {
    fn describe(&self) -> Describe {
        Describe {
            name: "deneme".into(),
            version: "0.1.0".into(),
            protocol: PROTOCOL_VERSION,
            actions: vec![],
            accepts_drop: false,
            reminders: false,
        }
    }

    fn configure(&mut self, _config: Value) -> Result<(), String> {
        Ok(())
    }

    fn list(&mut self, _p: ListParams) -> Result<Vec<Item>, String> {
        match &self.list_error {
            Some(e) => Err(e.clone()),
            None => Ok(vec![]),
        }
    }

    fn action(&mut self, _p: ActionParams) -> Result<Option<Item>, String> {
        Ok(None)
    }
}

fn run(plugin: Deneme, input: &str) -> Vec<String> {
    let mut out = Vec::new();
    serve(plugin, Cursor::new(input.as_bytes()), &mut out).unwrap();
    String::from_utf8(out)
        .unwrap()
        .lines()
        .map(String::from)
        .collect()
}

fn run_one(input: &str) -> Response {
    let lines = run(Deneme { list_error: None }, input);
    assert_eq!(lines.len(), 1);
    serde_json::from_str(&lines[0]).unwrap()
}

#[test]
fn describe_roundtrip() {
    let r = run_one("{\"id\":1,\"method\":\"describe\"}\n");
    assert_eq!(r.id, 1);
    let result = r.result.unwrap();
    assert_eq!(result["protocol"], 1);
    assert_eq!(result["name"], "deneme");
    assert!(r.error.is_none());
}

#[test]
fn unknown_method_is_error() {
    let r = run_one("{\"id\":2,\"method\":\"yok\"}\n");
    assert_eq!(r.id, 2);
    assert_eq!(r.error.unwrap().message, "bilinmeyen yöntem: yok");
    assert!(r.result.is_none());
}

#[test]
fn bad_json_line_is_error_with_id_zero() {
    let lines = run(Deneme { list_error: None }, "{bozuk\n");
    assert_eq!(lines.len(), 1);
    let r: Response = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(r.id, 0);
    assert!(r.error.unwrap().message.starts_with("geçersiz istek: "));
    assert!(lines[0].starts_with("{\"id\":0,\"error\":{\"message\":\"geçersiz istek: "));
}

#[test]
fn plugin_error_is_forwarded() {
    let lines = run(
        Deneme {
            list_error: Some("401".into()),
        },
        "{\"id\":3,\"method\":\"list\",\"params\":{\"view\":\"summary\"}}\n",
    );
    let r: Response = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(r.id, 3);
    assert_eq!(r.error.unwrap().message, "401");
    assert!(r.result.is_none());
}

#[test]
fn item_omits_nothing_required() {
    let item: Item = serde_json::from_str("{\"id\":\"1\",\"title\":\"a\"}").unwrap();
    assert!(item.labels.is_empty());
    assert!(item.links.is_empty());
    assert!(!item.overdue);
    assert!(item.due.is_none());
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains("\"labels\":[]"));
    assert!(json.contains("\"overdue\":false"));
}

#[test]
fn drop_default_message() {
    let item = "{\"id\":\"1\",\"title\":\"a\"}";
    let line = format!("{{\"id\":4,\"method\":\"drop\",\"params\":{{\"item\":{item}}}}}\n");
    let r = run_one(&line);
    assert_eq!(
        r.error.unwrap().message,
        "bu kutu bırakılan öğe kabul etmiyor"
    );
}

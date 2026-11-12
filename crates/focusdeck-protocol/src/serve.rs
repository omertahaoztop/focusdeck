use std::io::{BufRead, Write};

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::types::{
    ActionParams, Describe, DropParams, Item, ListParams, ListResult, Reminder, RemindersParams,
    RemindersResult, Request, Response, RpcError,
};

pub trait Plugin {
    fn describe(&self) -> Describe;
    fn configure(&mut self, config: Value) -> Result<(), String>;
    fn list(&mut self, p: ListParams) -> Result<Vec<Item>, String>;
    fn action(&mut self, p: ActionParams) -> Result<Option<Item>, String>;
    fn drop_item(&mut self, _p: DropParams) -> Result<Item, String> {
        Err("this box does not accept dropped items".into())
    }
    fn reminders(&mut self, _p: RemindersParams) -> Result<Vec<Reminder>, String> {
        Ok(vec![])
    }
}

pub fn serve<P: Plugin>(
    mut plugin: P,
    input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(req) => {
                let id = req.id;
                match dispatch(&mut plugin, req) {
                    Ok(value) => Response {
                        id,
                        result: Some(value),
                        error: None,
                    },
                    Err(message) => failure(id, message),
                }
            }
            Err(e) => failure(0, format!("geçersiz istek: {e}")),
        };
        serde_json::to_writer(&mut output, &response)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}

fn failure(id: u64, message: String) -> Response {
    Response {
        id,
        result: None,
        error: Some(RpcError { message }),
    }
}

fn params<T: DeserializeOwned>(value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|e| format!("geçersiz parametre: {e}"))
}

fn to_value<T: Serialize>(value: T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

fn dispatch<P: Plugin>(plugin: &mut P, req: Request) -> Result<Value, String> {
    match req.method.as_str() {
        "describe" => to_value(plugin.describe()),
        "configure" => {
            plugin.configure(req.params)?;
            Ok(Value::Object(Default::default()))
        }
        "list" => {
            let items = plugin.list(params(req.params)?)?;
            to_value(ListResult { items })
        }
        "action" => to_value(plugin.action(params(req.params)?)?),
        "drop" => to_value(plugin.drop_item(params(req.params)?)?),
        "reminders" => {
            let reminders = plugin.reminders(params(req.params)?)?;
            to_value(RemindersResult { reminders })
        }
        other => Err(format!("bilinmeyen yöntem: {other}")),
    }
}

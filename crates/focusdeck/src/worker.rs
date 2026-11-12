//! Drives one box's plugin host from a command channel and reports results as events.

use focusdeck_protocol::{
    ActionParams, Describe, DropParams, Item, ListParams, ListResult, Source, View,
};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::host::PluginHost;

// the plan fixes Drop to carry Item by value; commands are rare so the size is irrelevant
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum BoxCmd {
    Refresh(View),
    Action {
        item: String,
        action: String,
        args: Value,
    },
    Drop {
        item: Item,
        from: Option<Source>,
    },
}

#[derive(Debug)]
pub enum BoxEvent {
    Described {
        idx: usize,
        describe: Describe,
    },
    Listed {
        idx: usize,
        view: View,
        result: Result<Vec<Item>, String>,
    },
    Finished {
        idx: usize,
        what: String,
        result: Result<(), String>,
    },
}

pub fn spawn(
    idx: usize,
    mut host: PluginHost,
    mut rx: mpsc::Receiver<BoxCmd>,
    tx: mpsc::Sender<BoxEvent>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut described = false;
        if !announce(idx, &mut host, &mut described, &tx).await {
            return;
        }
        while let Some(cmd) = rx.recv().await {
            if !announce(idx, &mut host, &mut described, &tx).await {
                return;
            }
            let event = match cmd {
                BoxCmd::Refresh(view) => {
                    let result = host
                        .call::<_, ListResult>("list", &ListParams { view })
                        .await
                        .map(|r| r.items)
                        .map_err(|e| e.to_string());
                    BoxEvent::Listed { idx, view, result }
                }
                BoxCmd::Action { item, action, args } => {
                    let params = ActionParams {
                        item,
                        action: action.clone(),
                        args,
                    };
                    let result = host
                        .call::<_, Option<Item>>("action", &params)
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string());
                    BoxEvent::Finished {
                        idx,
                        what: action,
                        result,
                    }
                }
                BoxCmd::Drop { item, from } => {
                    let result = host
                        .call::<_, Item>("drop", &DropParams { item, from })
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string());
                    BoxEvent::Finished {
                        idx,
                        what: "drop".into(),
                        result,
                    }
                }
            };
            if tx.send(event).await.is_err() {
                return;
            }
        }
    })
}

// false means the event receiver is gone and the worker should stop
async fn announce(
    idx: usize,
    host: &mut PluginHost,
    described: &mut bool,
    tx: &mpsc::Sender<BoxEvent>,
) -> bool {
    if *described {
        return true;
    }
    // a failed describe is retried on the next command, whose own call reports the error
    let Ok(describe) = host.describe().await else {
        return true;
    };
    *described = true;
    tx.send(BoxEvent::Described { idx, describe }).await.is_ok()
}

//! Drives one box's plugin host from a command channel and reports results as events.

use focusdeck_protocol::{
    ActionParams, Describe, DropParams, Item, ListParams, ListResult, Source, View,
};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::host::BoxHost;

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
        capture: bool,
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
    host: impl Into<BoxHost>,
    mut rx: mpsc::Receiver<BoxCmd>,
    tx: mpsc::Sender<BoxEvent>,
) -> JoinHandle<()> {
    let mut host = host.into();
    tokio::spawn(async move {
        let mut described = false;
        // the first command is answered with this error instead of trying the handshake a second time
        let mut start_error = match announce(idx, &mut host, &mut described, &tx).await {
            Announced::Ready => None,
            Announced::Failed(error) => Some(error),
            Announced::ReceiverGone => return,
        };
        while let Some(cmd) = rx.recv().await {
            let failure = if described {
                None
            } else if let Some(error) = start_error.take() {
                Some(error)
            } else {
                match announce(idx, &mut host, &mut described, &tx).await {
                    Announced::Ready => None,
                    Announced::Failed(error) => Some(error),
                    Announced::ReceiverGone => return,
                }
            };
            let event = match (cmd, failure) {
                (BoxCmd::Refresh(view), Some(error)) => BoxEvent::Listed {
                    idx,
                    view,
                    result: Err(error),
                },
                (BoxCmd::Action { action, .. }, Some(error)) => BoxEvent::Finished {
                    idx,
                    what: action,
                    result: Err(error),
                },
                (BoxCmd::Drop { capture, .. }, Some(error)) => BoxEvent::Finished {
                    idx,
                    what: drop_name(capture).into(),
                    result: Err(error),
                },
                (BoxCmd::Refresh(view), None) => {
                    let result = host
                        .call::<_, ListResult>("list", &ListParams { view })
                        .await
                        .map(|r| r.items)
                        .map_err(|e| e.to_string());
                    BoxEvent::Listed { idx, view, result }
                }
                (BoxCmd::Action { item, action, args }, None) => {
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
                (
                    BoxCmd::Drop {
                        item,
                        from,
                        capture,
                    },
                    None,
                ) => {
                    let result = host
                        .call::<_, Item>("drop", &DropParams { item, from })
                        .await
                        .map(|_| ())
                        .map_err(|e| e.to_string());
                    BoxEvent::Finished {
                        idx,
                        what: drop_name(capture).into(),
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

// A typed note reports under its own name, so its result is never taken for that of a moved item.
fn drop_name(capture: bool) -> &'static str {
    if capture { "capture" } else { "drop" }
}

enum Announced {
    Ready,
    Failed(String),
    ReceiverGone,
}

async fn announce(
    idx: usize,
    host: &mut BoxHost,
    described: &mut bool,
    tx: &mpsc::Sender<BoxEvent>,
) -> Announced {
    match host.describe().await {
        Ok(describe) => {
            *described = true;
            match tx.send(BoxEvent::Described { idx, describe }).await {
                Ok(()) => Announced::Ready,
                Err(_) => Announced::ReceiverGone,
            }
        }
        Err(e) => Announced::Failed(e.to_string()),
    }
}

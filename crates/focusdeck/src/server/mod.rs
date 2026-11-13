pub mod board;
pub mod http;
pub mod idempotency;

use std::sync::Arc;

use anyhow::Context;

use crate::config::Config;
use crate::i18n::{msg, t, t_args};
use crate::paths::Paths;
use crate::secrets;
use board::Board;
use idempotency::IdempotencyStore;

pub async fn run(paths: &Paths, cfg: &Config) -> anyhow::Result<()> {
    let serve = cfg
        .serve
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!(t(msg::SERVER_NO_SECTION)))?;
    let home = dirs::home_dir().unwrap_or_default();
    let token = secrets::read_token(&serve.token_file, &home)?;
    let board = Board::load(paths, cfg, &home)?;
    std::fs::create_dir_all(&paths.state_dir)
        .with_context(|| paths.state_dir.display().to_string())?;
    let (keys, dropped) = IdempotencyStore::open(
        paths.state_dir.join("idempotency.json"),
        chrono::Local::now().fixed_offset(),
    )?;
    let listener = tokio::net::TcpListener::bind(serve.listen)
        .await
        .with_context(|| serve.listen.to_string())?;
    if dropped > 0 {
        eprintln!(
            "{}",
            t_args(
                msg::SERVER_DROPPED_IN_PROGRESS,
                &[("count", dropped.into())]
            )
        );
    }
    let addr = listener.local_addr()?;
    println!(
        "{}",
        t_args(
            msg::SERVER_LISTENING,
            &[("addr", addr.to_string().as_str().into())]
        )
    );
    axum::serve(
        listener,
        http::router(Arc::new(board), Arc::new(keys), token),
    )
    .await?;
    Ok(())
}

//! # tessera-app
//!
//! The thin app around the `tessera` engine: one shared world in memory (the
//! Commons), live passes over SSE, a weather loop that drifts the world, and
//! pages for the atlas and the tileset.
//!
//!   Engine       -> `tessera`: tiles, the solver, worlds that repair.
//!   State        -> [`commons::Commons`], an `AppState` extension. No
//!                   database: see docs/adr/0001-the-commons-lives-in-memory.md.
//!   Live events  -> one topic, `commons`. Every pass is published with its
//!                   trace; browsers replay the search tile by tile.
//!   Interface    -> Maud for the shell, one canvas per map, and three
//!                   scripts: the engine port, the renderer, the page glue.

pub mod commons;
pub mod routes;
pub mod view;

use std::sync::Arc;
use std::time::Duration;

use autumn_web::AppState;

use crate::commons::{Broadcast, Commons, TOPIC};

/// How often the weather drifts the world.
pub const DRIFT_EVERY: Duration = Duration::from_millis(2_600);

/// The word the Commons grows from, unless `TESSERA_WORD` says otherwise.
pub const DEFAULT_WORD: &str = "Tessera";

/// Publish one event. A publish with nobody listening is not an error worth
/// reporting.
pub fn publish(state: &AppState, b: &Broadcast) {
    match serde_json::to_string(b) {
        Ok(json) => {
            if let Err(e) = state.channels().publish(TOPIC, json) {
                tracing::debug!(error = %e, "commons event not delivered");
            }
        }
        Err(e) => tracing::warn!(error = %e, "commons event did not serialise"),
    }
}

/// Install a Commons grown from `word`. Synchronous, so tests can call it
/// from a state initializer.
///
/// # Errors
///
/// If the word is unusable.
pub fn install(state: &AppState, word: &str) -> anyhow::Result<()> {
    let commons = Commons::grow(word)?;
    state.insert_extension(commons);
    Ok(())
}

/// The installed Commons.
pub fn commons(state: &AppState) -> Option<Arc<Commons>> {
    state.extension::<Commons>()
}

/// Boot: grow the Commons and start the weather.
///
/// # Errors
///
/// If the Commons cannot be grown.
#[allow(clippy::unused_async)] // `on_startup` takes an async hook; this one has nothing to await
pub async fn boot(state: AppState) -> autumn_web::AutumnResult<()> {
    use autumn_web::error::AutumnError;
    let word = std::env::var("TESSERA_WORD").unwrap_or_else(|_| DEFAULT_WORD.into());
    let started = std::time::Instant::now();
    install(&state, &word).map_err(|e| AutumnError::service_unavailable_msg(e.to_string()))?;
    let commons = commons(&state)
        .ok_or_else(|| AutumnError::service_unavailable_msg("the Commons did not install"))?;
    if std::env::var("TESSERA_DRIFT").is_ok_and(|v| v == "off") {
        commons.set_drift(false);
    }
    tracing::info!(
        word,
        ms = started.elapsed().as_millis(),
        "the Commons has grown"
    );

    tokio::spawn(async move {
        let mut tick = tokio::time::interval(DRIFT_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            let c = Arc::clone(&commons);
            match tokio::task::spawn_blocking(move || c.drift_once()).await {
                Ok(Ok(Some(b))) => publish(&state, &b),
                Ok(Ok(None)) => {}
                Ok(Err(e)) => tracing::warn!(error = %e, "the weather failed"),
                Err(e) => tracing::warn!(error = %e, "the weather task died"),
            }
        }
    });
    Ok(())
}

//! The HTTP surface.

use std::convert::Infallible;
use std::sync::Arc;

use autumn_web::error::AutumnError;
use autumn_web::prelude::*;
use autumn_web::sse::{Sse, from_subscriber};
use serde::{Deserialize, Serialize};
use tessera::tiles::Brush;
use tokio_stream::Stream;

use crate::commons::{Broadcast, Commons, CommonsError, Painter, Snapshot, TOPIC};
use crate::view;

fn the_commons(state: &AppState) -> AutumnResult<Arc<Commons>> {
    crate::commons(state)
        .ok_or_else(|| AutumnError::service_unavailable_msg("the Commons is still growing"))
}

fn refuse(e: &CommonsError) -> AutumnError {
    AutumnError::unprocessable_msg(e.to_string())
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, CommonsError> + Send + 'static,
) -> AutumnResult<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AutumnError::internal_server_error_msg(e.to_string()))?
        .map_err(|e| refuse(&e))
}

/// The Commons.
#[get("/")]
#[public]
pub async fn index(csrf: Option<CsrfToken>) -> Markup {
    view::commons_page(csrf.as_ref().map(CsrfToken::token))
}

/// The whole world, for a browser starting or catching up.
#[get("/_world.json")]
#[public]
pub async fn world_json(State(state): State<AppState>) -> AutumnResult<Json<Snapshot>> {
    Ok(Json(the_commons(&state)?.snapshot()))
}

/// A stroke, from a browser.
#[derive(Debug, Clone, Deserialize)]
pub struct PaintForm {
    /// Cells as `x,y;x,y;…`.
    pub cells: String,
    /// A brush name, or `erase`.
    pub brush: String,
    /// The painter's name.
    #[serde(default)]
    pub name: String,
    /// The painter's hue.
    #[serde(default)]
    pub hue: u32,
}

/// Parse `x,y;x,y`.
pub fn parse_cells(s: &str) -> Result<Vec<(i64, i64)>, String> {
    s.split(';')
        .filter(|p| !p.trim().is_empty())
        .map(|p| {
            let (x, y) = p
                .split_once(',')
                .ok_or_else(|| format!("a cell is x,y, not {p:?}"))?;
            let x = x.trim().parse().map_err(|_| format!("bad column {x:?}"))?;
            let y = y.trim().parse().map_err(|_| format!("bad row {y:?}"))?;
            Ok((x, y))
        })
        .collect()
}

/// Parse a brush name; `erase` is `None`.
pub fn parse_brush(s: &str) -> Result<Option<Brush>, String> {
    if s == "erase" {
        return Ok(None);
    }
    Brush::ALL
        .into_iter()
        .find(|b| b.name() == s)
        .map(Some)
        .ok_or_else(|| format!("no brush called {s:?}"))
}

/// What a paint did, for the painter.
#[derive(Debug, Clone, Serialize)]
pub struct PaintReply {
    /// The pass's sequence number.
    pub seq: u64,
    /// Cells changed.
    pub changed: u32,
    /// Disc radius.
    pub radius: u32,
    /// Discs that could not hold it.
    pub grown: u32,
    /// Backtracks.
    pub backtracks: u32,
}

/// Paint a stroke. The pass goes to everyone over `/stream`; the reply is
/// only its summary.
#[post("/paint")]
#[public]
pub async fn paint(
    State(state): State<AppState>,
    Form(form): Form<PaintForm>,
) -> AutumnResult<Json<PaintReply>> {
    let stroke = parse_cells(&form.cells).map_err(AutumnError::bad_request_msg)?;
    let brush = parse_brush(&form.brush).map_err(AutumnError::bad_request_msg)?;
    let by = Painter::new(&form.name, form.hue);
    let c = the_commons(&state)?;
    let b = blocking(move || c.paint(&stroke, brush, by)).await?;
    crate::publish(&state, &b);
    match b {
        Broadcast::Pass {
            seq,
            radius,
            grown,
            pass,
            ..
        } => Ok(Json(PaintReply {
            seq,
            changed: pass.changed,
            radius,
            grown,
            backtracks: pass.backtracks,
        })),
        Broadcast::World { .. } => Err(AutumnError::internal_server_error_msg(
            "a paint grew a world",
        )),
    }
}

/// A word, from a browser.
#[derive(Debug, Clone, Deserialize)]
pub struct GrowForm {
    /// The word.
    pub word: String,
    /// The painter's name.
    #[serde(default)]
    pub name: String,
    /// The painter's hue.
    #[serde(default)]
    pub hue: u32,
}

/// What growing did.
#[derive(Debug, Clone, Serialize)]
pub struct GrowReply {
    /// The new world's sequence number.
    pub seq: u64,
    /// The word.
    pub word: String,
}

/// Replace the Commons with a world grown from a word.
#[post("/grow")]
#[public]
pub async fn grow(
    State(state): State<AppState>,
    Form(form): Form<GrowForm>,
) -> AutumnResult<Json<GrowReply>> {
    let by = Painter::new(&form.name, form.hue);
    let c = the_commons(&state)?;
    let word = form.word;
    let b = blocking(move || c.regrow(&word, by)).await?;
    crate::publish(&state, &b);
    match b {
        Broadcast::World { seq, word, .. } => Ok(Json(GrowReply { seq, word })),
        Broadcast::Pass { .. } => Err(AutumnError::internal_server_error_msg(
            "growing painted instead",
        )),
    }
}

/// The weather's state.
#[derive(Debug, Clone, Serialize)]
pub struct DriftReply {
    /// Whether the weather drifts the world.
    pub drift: bool,
}

/// Start or stop the weather.
#[post("/drift/{on}")]
#[public]
pub async fn drift(
    State(state): State<AppState>,
    Path(on): Path<String>,
) -> AutumnResult<Json<DriftReply>> {
    let on = match on.as_str() {
        "on" => true,
        "off" => false,
        other => {
            return Err(AutumnError::bad_request_msg(format!(
                "the weather is on or off, not {other}"
            )));
        }
    };
    let c = the_commons(&state)?;
    c.set_drift(on);
    Ok(Json(DriftReply {
        drift: c.drifting(),
    }))
}

/// The live feed: one message per pass.
#[get("/stream")]
#[public]
pub async fn stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<autumn_web::sse::Event, Infallible>>> {
    from_subscriber(state.channels().subscribe(TOPIC))
}

/// The atlas: any word's island, grown in the browser.
#[get("/atlas")]
#[public]
pub async fn atlas(csrf: Option<CsrfToken>) -> Markup {
    view::atlas_page(csrf.as_ref().map(CsrfToken::token))
}

/// The tileset, every tile on one plate.
#[get("/tiles")]
#[public]
pub async fn tiles(State(state): State<AppState>) -> AutumnResult<Markup> {
    Ok(view::tiles_page(the_commons(&state)?.tileset()))
}

/// How it works.
#[get("/about")]
#[public]
pub async fn about() -> Markup {
    view::about_page()
}

fn script(body: &'static str) -> impl IntoResponse {
    (
        [
            ("content-type", "text/javascript; charset=utf-8"),
            ("cache-control", "no-cache"),
        ],
        body,
    )
}

/// The engine, ported.
#[get("/engine.js")]
#[public]
pub async fn engine_js() -> impl IntoResponse {
    script(view::ENGINE)
}

/// The renderer.
#[get("/render.js")]
#[public]
pub async fn render_js() -> impl IntoResponse {
    script(view::RENDER)
}

/// The page glue.
#[get("/tessera.js")]
#[public]
pub async fn tessera_js() -> impl IntoResponse {
    script(view::SCRIPT)
}

/// Every route, for `main` and the tests.
#[macro_export]
macro_rules! all_routes {
    () => {
        autumn_web::routes![
            $crate::routes::index,
            $crate::routes::world_json,
            $crate::routes::paint,
            $crate::routes::grow,
            $crate::routes::drift,
            $crate::routes::stream,
            $crate::routes::atlas,
            $crate::routes::tiles,
            $crate::routes::about,
            $crate::routes::engine_js,
            $crate::routes::render_js,
            $crate::routes::tessera_js,
        ]
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_parse() {
        assert_eq!(parse_cells("1,2;3,4").unwrap(), vec![(1, 2), (3, 4)]);
        assert_eq!(parse_cells(" 5 , 6 ;").unwrap(), vec![(5, 6)]);
        assert!(parse_cells("1;2").is_err());
        assert!(parse_cells("a,2").is_err());
        assert!(parse_cells("").unwrap().is_empty());
    }

    #[test]
    fn brushes_parse() {
        assert_eq!(parse_brush("town").unwrap(), Some(Brush::Town));
        assert_eq!(parse_brush("erase").unwrap(), None);
        assert!(parse_brush("lava").is_err());
    }
}

//! The Commons: one shared world, in memory, that everyone paints on.
//!
//! Every change to the world — a paint, a drift, a new world grown from a
//! word — is serialised under one lock, given the next sequence number, and
//! published as one [`Broadcast`]. A browser that sees a gap in the sequence
//! refetches the whole world.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use tessera::rng::Seed;
use tessera::tiles::{Brush, Tileset};
use tessera::wave::{Border, Dims, SolveError, Tables};
use tessera::world::{PaintError, Pass, World};
use thiserror::Error;

/// The live-events topic.
pub const TOPIC: &str = "commons";
/// Columns of the Commons.
pub const WIDTH: u32 = 64;
/// Rows of the Commons.
pub const HEIGHT: u32 = 40;
/// The longest word a world may grow from.
pub const MAX_WORD: usize = 40;
/// The longest painter name kept.
pub const MAX_NAME: usize = 32;

/// Why a request to the Commons failed.
#[derive(Debug, Error)]
pub enum CommonsError {
    /// The paint did not land.
    #[error(transparent)]
    Paint(#[from] PaintError),
    /// A world could not be grown.
    #[error(transparent)]
    Solve(#[from] SolveError),
    /// A malformed request.
    #[error("{0}")]
    Bad(String),
}

/// Who did something.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Painter {
    /// A display name.
    pub name: String,
    /// A hue, 0..360.
    pub hue: u16,
}

impl Painter {
    /// A painter from untrusted input: trimmed, clipped, never empty.
    pub fn new(name: &str, hue: u32) -> Self {
        let name: String = name
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_NAME)
            .collect();
        let name = name.trim();
        Self {
            name: if name.is_empty() {
                "a cartographer".into()
            } else {
                name.into()
            },
            hue: u16::try_from(hue % 360).unwrap_or(0),
        }
    }

    /// The world itself, drifting.
    pub fn weather() -> Self {
        Self {
            name: "the weather".into(),
            hue: 200,
        }
    }
}

/// A pass on the wire: cells as indices, the trace in compact form.
#[derive(Debug, Clone, Serialize)]
pub struct WirePass {
    /// Region cells.
    pub region: Vec<u32>,
    /// Encoded trace events (see `tessera::wave::Event::encode`).
    pub trace: Vec<i64>,
    /// Final tile ids of the region.
    pub tiles: Vec<u16>,
    /// Cells changed.
    pub changed: u32,
    /// Backtracks.
    pub backtracks: u32,
}

impl From<&Pass> for WirePass {
    fn from(p: &Pass) -> Self {
        Self {
            region: p.region.iter().map(|c| c.0).collect(),
            trace: p.trace.iter().map(|e| e.encode()).collect(),
            tiles: p.tiles.iter().map(|t| t.0).collect(),
            changed: p.changed,
            backtracks: p.backtracks,
        }
    }
}

/// One event on the topic.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Broadcast {
    /// Someone painted, or the weather drifted.
    Pass {
        /// Sequence number.
        seq: u64,
        /// Who.
        by: Painter,
        /// `sea`, `town`, …, `erase` or `drift`.
        what: String,
        /// The first stroke cell, for the feed.
        at: [u32; 2],
        /// Disc radius that held it (0 for drift).
        radius: u32,
        /// Discs that could not.
        grown: u32,
        /// The pass.
        pass: WirePass,
    },
    /// A new world grew from a word.
    World {
        /// Sequence number.
        seq: u64,
        /// Who.
        by: Painter,
        /// The word.
        word: String,
        /// The bare geology, before any block.
        bare: Vec<u16>,
        /// Every generation block.
        passes: Vec<WirePass>,
    },
}

/// What a browser starting up needs.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    /// Columns.
    pub w: u32,
    /// Rows.
    pub h: u32,
    /// The latest sequence number.
    pub seq: u64,
    /// The word the world grew from.
    pub word: String,
    /// Every tile id, row-major.
    pub tiles: Vec<u16>,
    /// Pinned cells: `(cell, brush)`.
    pub pins: Vec<(u32, Brush)>,
    /// Whether the weather is drifting the world.
    pub drift: bool,
    /// Strokes and drifts committed since the world grew.
    pub passes: u32,
}

struct Inner {
    world: World,
    word: String,
    seq: u64,
    salt: u32,
}

/// The Commons.
#[derive(Debug)]
pub struct Commons {
    ts: Tileset,
    tables: Tables,
    inner: Mutex<Inner>,
    drift: AtomicBool,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner")
            .field("word", &self.word)
            .field("seq", &self.seq)
            .finish_non_exhaustive()
    }
}

const fn dims() -> Result<Dims, SolveError> {
    Dims::new(WIDTH, HEIGHT)
}

fn clean_word(word: &str) -> Result<String, CommonsError> {
    let w: String = word.trim().chars().filter(|c| !c.is_control()).collect();
    if w.is_empty() || w.chars().count() > MAX_WORD {
        return Err(CommonsError::Bad(format!(
            "a world grows from a word of 1 to {MAX_WORD} characters"
        )));
    }
    Ok(w)
}

impl Commons {
    /// Grow the Commons from `word`.
    pub fn grow(word: &str) -> Result<Self, CommonsError> {
        let ts = Tileset::island();
        let tables = Tables::new(&ts);
        let word = clean_word(word)?;
        let (grown, _) =
            World::generate(&ts, &tables, dims()?, Border::Sea, Seed::from_word(&word))?;
        Ok(Self {
            ts,
            tables,
            inner: Mutex::new(Inner {
                world: grown,
                word,
                seq: 0,
                salt: 0,
            }),
            drift: AtomicBool::new(true),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while holding the lock cannot leave the world half-written:
        // every mutation is computed first and assigned whole.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The tileset.
    pub const fn tileset(&self) -> &Tileset {
        &self.ts
    }

    /// The whole world.
    pub fn snapshot(&self) -> Snapshot {
        let g = self.lock();
        let w = &g.world;
        Snapshot {
            w: w.dims().w,
            h: w.dims().h,
            seq: g.seq,
            word: g.word.clone(),
            tiles: w.tiles().iter().map(|t| t.0).collect(),
            pins: w
                .pins()
                .iter()
                .enumerate()
                .filter_map(|(i, p)| p.map(|b| (u32::try_from(i).unwrap_or(u32::MAX), b)))
                .collect(),
            drift: self.drifting(),
            passes: w.passes(),
        }
    }

    /// Whether the weather drifts the world.
    pub fn drifting(&self) -> bool {
        self.drift.load(Ordering::Relaxed)
    }

    /// Turn the weather on or off.
    pub fn set_drift(&self, on: bool) {
        self.drift.store(on, Ordering::Relaxed);
    }

    /// Paint. `brush == None` erases pins.
    pub fn paint(
        &self,
        stroke: &[(i64, i64)],
        brush: Option<Brush>,
        by: Painter,
    ) -> Result<Broadcast, CommonsError> {
        let mut g = self.lock();
        g.salt = g.salt.wrapping_add(1);
        let salt = g.salt;
        let repair = g.world.paint(&self.ts, &self.tables, stroke, brush, salt)?;
        g.seq += 1;
        let at = stroke.first().map_or([0, 0], |&(x, y)| {
            [u32::try_from(x).unwrap_or(0), u32::try_from(y).unwrap_or(0)]
        });
        Ok(Broadcast::Pass {
            seq: g.seq,
            by,
            what: brush.map_or("erase", Brush::name).into(),
            at,
            radius: repair.radius,
            grown: repair.grown,
            pass: WirePass::from(&repair.pass),
        })
    }

    /// One drift. `None` if the weather is off or nothing changed.
    pub fn drift_once(&self) -> Result<Option<Broadcast>, CommonsError> {
        if !self.drifting() {
            return Ok(None);
        }
        let mut g = self.lock();
        g.salt = g.salt.wrapping_add(1);
        let salt = g.salt;
        let pass = g.world.drift(&self.ts, &self.tables, salt)?;
        if !pass.ok || pass.changed == 0 {
            return Ok(None);
        }
        g.seq += 1;
        let first = pass.region.first().map_or(0, |c| c.0);
        Ok(Some(Broadcast::Pass {
            seq: g.seq,
            by: Painter::weather(),
            what: "drift".into(),
            at: [first % WIDTH, first / WIDTH],
            radius: 0,
            grown: 0,
            pass: WirePass::from(&pass),
        }))
    }

    /// Replace the world with one grown from `word`.
    pub fn regrow(&self, word: &str, by: Painter) -> Result<Broadcast, CommonsError> {
        let word = clean_word(word)?;
        let seed = Seed::from_word(&word);
        let bare = World::bare(&self.ts, dims()?, Border::Sea, seed);
        let (grown, passes) = World::generate(&self.ts, &self.tables, dims()?, Border::Sea, seed)?;
        let mut g = self.lock();
        g.world = grown;
        g.word.clone_from(&word);
        g.seq += 1;
        Ok(Broadcast::World {
            seq: g.seq,
            by,
            word,
            bare: bare.tiles().iter().map(|t| t.0).collect(),
            passes: passes.iter().map(WirePass::from).collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn painters_are_cleaned() {
        assert_eq!(Painter::new("  ", 30).name, "a cartographer");
        assert_eq!(Painter::new("ochre\u{7}", 400).name, "ochre");
        assert_eq!(Painter::new("x", 400).hue, 40);
        assert_eq!(Painter::new(&"a".repeat(100), 1).name.len(), MAX_NAME);
    }

    #[test]
    fn words_are_checked() {
        assert!(clean_word("").is_err());
        assert!(clean_word(&"w".repeat(41)).is_err());
        assert_eq!(clean_word("  Avalon ").unwrap(), "Avalon");
    }

    #[test]
    fn the_commons_paints_drifts_and_regrows_in_sequence() {
        let c = Commons::grow("tessera").unwrap();
        assert_eq!(c.snapshot().seq, 0);
        let b = c
            .paint(&[(32, 20)], Some(Brush::Town), Painter::new("me", 10))
            .unwrap();
        assert!(matches!(b, Broadcast::Pass { seq: 1, .. }));
        let s = c.snapshot();
        assert_eq!(s.pins.len(), 1);
        assert_eq!(s.tiles.len(), (WIDTH * HEIGHT) as usize);
        let mut drifted = 0;
        for _ in 0..30 {
            if c.drift_once().unwrap().is_some() {
                drifted += 1;
            }
        }
        assert!(drifted > 0);
        c.set_drift(false);
        assert!(c.drift_once().unwrap().is_none());
        let before = c.snapshot().seq;
        let w = c.regrow("lyonesse", Painter::weather()).unwrap();
        assert!(matches!(w, Broadcast::World { .. }));
        let s = c.snapshot();
        assert_eq!(s.seq, before + 1);
        assert_eq!(s.word, "lyonesse");
        assert!(s.pins.is_empty());
    }

    #[test]
    fn an_impossible_paint_is_an_error_and_changes_nothing() {
        let c = Commons::grow("tessera").unwrap();
        let before = c.snapshot();
        assert!(
            c.paint(&[(0, 0)], Some(Brush::Town), Painter::weather())
                .is_err()
        );
        let after = c.snapshot();
        assert_eq!(before.tiles, after.tiles);
        assert_eq!(after.seq, 0);
    }
}

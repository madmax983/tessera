//! A world you can paint on: a map that is valid at every instant, the brushes
//! people have pinned to it, and *modification in blocks*.
//!
//! A world starts as open sea — a map that trivially keeps every promise —
//! and only ever changes by **reshaping a region**: the region's cells are
//! freed, everything outside is held fixed, and the solver fills the region
//! in. The result is committed only if the solve succeeds, so the map is
//! consistent before and after every pass (`docs/SPEC.md` §4, the splice
//! lemma in `verus/src/tessera.rs`).
//!
//! * **Generation** sweeps overlapping square blocks over the sea in a
//!   shuffled order. Land rises block by block.
//! * **Painting** pins a brush to a stroke and reshapes a disc around it,
//!   growing the disc until it can hold the stroke.
//! * **Drift** reshapes a small random disc, so a world left alone keeps
//!   redrawing itself — everywhere except where someone has pinned a brush.

use serde::Serialize;
use thiserror::Error;

use crate::rng::{Rng, Seed};
use crate::tiles::{Brush, Level, TileId, Tileset};
use crate::wave::{self, Border, Cell, Dims, Event, Problem, SolveError, Tables};

/// Backtracks allowed per solve attempt.
pub const BUDGET: u32 = 600;
/// Fresh starts allowed per solve.
pub const RESTARTS: u32 = 4;
/// Disc radii tried for a paint stroke, in cells.
pub const RADII: [u32; 5] = [3, 4, 6, 8, 11];
/// The most cells one stroke may touch.
pub const MAX_STROKE: usize = 64;
/// Side of a generation block, in cells.
pub const BLOCK: u32 = 12;
/// Distance between generation blocks, in cells.
pub const STRIDE: u32 = 6;

/// Why a paint did not land.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PaintError {
    /// A stroke cell is off the map.
    #[error("({x}, {y}) is off the map")]
    OffMap {
        /// Column.
        x: i64,
        /// Row.
        y: i64,
    },
    /// No cells, or too many.
    #[error("a stroke touches 1 to {MAX_STROKE} cells, not {0}")]
    Stroke(usize),
    /// The pins, together, admit no map at all, or none near enough.
    #[error("the world cannot hold that here: {0}")]
    Impossible(SolveError),
}

/// One committed (or abandoned) reshaping.
#[derive(Debug, Clone, Serialize)]
pub struct Pass {
    /// Cells freed, in row-major order.
    pub region: Vec<Cell>,
    /// The search inside the region, for replay. Placements of cells outside
    /// the region (which start fixed) are left out, and rewinds count region
    /// placements only.
    pub trace: Vec<Event>,
    /// The final tile of every region cell, in `region` order.
    pub tiles: Vec<TileId>,
    /// Region cells whose tile changed.
    pub changed: u32,
    /// Backtracks taken.
    pub backtracks: u32,
    /// Whether the pass was committed. An abandoned pass changed nothing.
    pub ok: bool,
}

/// What one paint did.
#[derive(Debug, Clone, Serialize)]
pub struct Repair {
    /// The pass that committed it.
    pub pass: Pass,
    /// The disc radius that held the stroke.
    pub radius: u32,
    /// Smaller discs that could not.
    pub grown: u32,
}

/// A world.
#[derive(Debug, Clone)]
pub struct World {
    dims: Dims,
    border: Border,
    seed: Seed,
    tiles: Vec<TileId>,
    pins: Vec<Option<Brush>>,
    passes: u32,
}

/// How a region is re-solved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Every region cell keeps its corner levels; only what runs over the
    /// land — rivers, roads, towns — is re-solved. Used to grow and to drift.
    Locked,
    /// Anything may change, pulled toward the current terrain by the guide.
    /// Used to paint, so a brush can raise a mountain out of open sea.
    Free,
}

/// Lattice spacing of the coarse and fine geology noise, in cells.
pub const NOISE: [u32; 2] = [11, 4];

/// The world's geology: a corner level (0 sea … 3 mountain) at every vertex.
///
/// There are `(w + 1) × (h + 1)` vertices, row-major, from two octaves of
/// integer value noise with an island falloff, then relaxed until every pair
/// of vertices that share a tile differs by at most one level.
///
/// Integer arithmetic throughout, so the browser computes the same geology.
pub fn geology(dims: Dims, border: Border, seed: Seed) -> Vec<u8> {
    let (width, height) = (i64::from(dims.w), i64::from(dims.h));
    let mut rng = Rng::new(seed.salted(0x0006_e010));
    let octaves: Vec<(i64, i64, Vec<i64>)> = NOISE
        .iter()
        .map(|&s| {
            let s = i64::from(s);
            let lw = width / s + 2;
            let lh = height / s + 2;
            let v = (0..lw * lh).map(|_| i64::from(rng.below(1000))).collect();
            (s, lw, v)
        })
        .collect();
    let mut out = Vec::with_capacity((dims.cells()) + dims.w as usize + dims.h as usize + 1);
    for y in 0..=height {
        for x in 0..=width {
            let mut v = 0;
            for (k, (s, lw, lat)) in octaves.iter().enumerate() {
                let (i, j) = (x / s, y / s);
                let (fx, fy) = (x % s, y % s);
                let at = |a: i64, b: i64| lat[usize::try_from(b * lw + a).unwrap_or(0)];
                let top = at(i, j) * (s - fx) + at(i + 1, j) * fx;
                let bot = at(i, j + 1) * (s - fx) + at(i + 1, j + 1) * fx;
                let n = (top * (s - fy) + bot * fy) / (s * s);
                v += if k == 0 { n * 2 } else { n };
            }
            // Distance from the middle, 0 at the centre and 1000 at an edge.
            let dx = (2 * x - width).abs() * 1000 / width;
            let dy = (2 * y - height).abs() * 1000 / height;
            let d = (dx * dx + dy * dy) / 1000;
            let score = v - d * 3 / 2 + 550;
            let edge = x == 0 || y == 0 || x == width || y == height;
            out.push(if border == Border::Sea && edge {
                0
            } else {
                match score {
                    ..1500 => 0,
                    1500..2050 => 1,
                    2050..2400 => 2,
                    _ => 3,
                }
            });
        }
    }
    relax(&mut out, dims.w as usize + 1, dims.h as usize + 1);
    out
}

/// Lower vertices until every vertex is within one level of all eight of its
/// neighbours.
///
/// Only ever lowers, so it terminates; at the fixed point every
/// tile's four corners form a smooth pattern (`docs/SPEC.md` §4, the relax
/// lemma in `verus/src/tessera.rs`).
pub fn relax(h: &mut [u8], w: usize, rows: usize) {
    loop {
        let mut changed = false;
        for y in 0..rows {
            for x in 0..w {
                let mut lo = h[y * w + x];
                for ny in y.saturating_sub(1)..(y + 2).min(rows) {
                    for nx in x.saturating_sub(1)..(x + 2).min(w) {
                        lo = lo.min(h[ny * w + nx].saturating_add(1));
                    }
                }
                if lo < h[y * w + x] {
                    h[y * w + x] = lo;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
}

impl World {
    /// Bare geology: every cell the plain tile whose corners are its four
    /// vertices' levels. Consistent by construction — neighbouring cells
    /// share the vertices of their common edge, and nothing else is on it.
    pub fn bare(ts: &Tileset, dims: Dims, border: Border, seed: Seed) -> Self {
        let vertices = geology(dims, border, seed);
        let vw = dims.w as usize + 1;
        let mut tiles = Vec::with_capacity(dims.cells());
        for y in 0..dims.h as usize {
            for x in 0..dims.w as usize {
                let v = |dx: usize, dy: usize| vertices[(y + dy) * vw + x + dx];
                let c = [v(0, 0), v(1, 0), v(1, 1), v(0, 1)];
                tiles.push(ts.plain(c.map(Level::from_u8)).unwrap_or(TileId(0)));
            }
        }
        Self {
            dims,
            border,
            seed,
            tiles,
            pins: vec![None; dims.cells()],
            passes: 0,
        }
    }

    /// Grow a world from a seed: bare geology, then one pass per generation
    /// block, in a shuffled order, each filling its block with rivers, roads
    /// and towns that fit the land around it.
    pub fn generate(
        ts: &Tileset,
        tables: &Tables,
        dims: Dims,
        border: Border,
        seed: Seed,
    ) -> Result<(Self, Vec<Pass>), SolveError> {
        let mut world = Self::bare(ts, dims, border, seed);
        let blocks = world.blocks(seed);
        let mut passes = Vec::with_capacity(blocks.len());
        for (k, region) in blocks.into_iter().enumerate() {
            let salt = u32::try_from(k).unwrap_or(u32::MAX);
            passes.push(world.reshape(
                ts,
                tables,
                &region,
                seed.salted(0xb10c_0000 ^ salt),
                Mode::Locked,
            )?);
        }
        Ok((world, passes))
    }

    /// The generation blocks for this map, shuffled by `seed`.
    pub fn blocks(&self, seed: Seed) -> Vec<Vec<Cell>> {
        let mut origins = Vec::new();
        let mut y = 0;
        while y < self.dims.h {
            let mut x = 0;
            while x < self.dims.w {
                origins.push((x, y));
                if x + BLOCK >= self.dims.w {
                    break;
                }
                x += STRIDE;
            }
            if y + BLOCK >= self.dims.h {
                break;
            }
            y += STRIDE;
        }
        // Fisher–Yates, high index first; the browser port does the same.
        let mut rng = Rng::new(seed.salted(0x5bu32 << 24));
        for i in (1..origins.len()).rev() {
            let j = rng.below(u32::try_from(i + 1).unwrap_or(u32::MAX)) as usize;
            origins.swap(i, j);
        }
        origins
            .into_iter()
            .map(|(x0, y0)| {
                let mut cells = Vec::new();
                for y in y0..(y0 + BLOCK).min(self.dims.h) {
                    for x in x0..(x0 + BLOCK).min(self.dims.w) {
                        cells.push(Cell(y * self.dims.w + x));
                    }
                }
                cells
            })
            .collect()
    }

    /// The map.
    pub const fn dims(&self) -> Dims {
        self.dims
    }

    /// The border.
    pub const fn border(&self) -> Border {
        self.border
    }

    /// The seed it grew from.
    pub const fn seed(&self) -> Seed {
        self.seed
    }

    /// Every tile.
    pub fn tiles(&self) -> &[TileId] {
        &self.tiles
    }

    /// Every pin.
    pub fn pins(&self) -> &[Option<Brush>] {
        &self.pins
    }

    /// Every cell's corner levels: the world's current geology.
    pub fn corners(&self, ts: &Tileset) -> Vec<[u8; 4]> {
        self.tiles
            .iter()
            .map(|&t| ts.tile(t).corners.map(|c| c as u8))
            .collect()
    }

    /// Passes committed so far.
    pub const fn passes(&self) -> u32 {
        self.passes
    }

    fn problem(
        &self,
        ts: &Tileset,
        pins: &[Option<Brush>],
        inside: &[bool],
        mode: Mode,
    ) -> Result<Problem, SolveError> {
        let mut p = Problem::new(ts, self.dims, self.border)?;
        let corners = self.corners(ts);
        p.guide(corners.clone());
        let brushes: Vec<(Brush, Vec<TileId>)> =
            Brush::ALL.iter().map(|&b| (b, ts.brush(b))).collect();
        for (i, &t) in self.tiles.iter().enumerate() {
            #[allow(clippy::cast_possible_truncation)] // cells ≤ 65536
            let c = Cell(i as u32);
            if !inside[i] {
                p.fix(c, t);
                continue;
            }
            if mode == Mode::Locked {
                let same: Vec<TileId> = ts
                    .ids()
                    .filter(|&u| ts.tile(u).corners.map(|l| l as u8) == corners[i])
                    .collect();
                p.restrict(c, &same);
            }
            if let Some(b) = pins[i]
                && let Some((_, ids)) = brushes.iter().find(|(x, _)| *x == b)
            {
                p.restrict(c, ids);
            }
        }
        Ok(p)
    }

    fn reshape_with(
        &mut self,
        ts: &Tileset,
        tables: &Tables,
        region: &[Cell],
        pins: &[Option<Brush>],
        seed: Seed,
        mode: Mode,
    ) -> Result<Pass, SolveError> {
        let mut inside = vec![false; self.dims.cells()];
        for c in region {
            inside[c.index()] = true;
        }
        let p = self.problem(ts, pins, &inside, mode)?;
        let sol = wave::solve(tables, &p, seed, BUDGET, RESTARTS)?;
        let tiles: Vec<TileId> = region.iter().map(|c| sol.tiles[c.index()]).collect();
        let changed = region
            .iter()
            .filter(|c| self.tiles[c.index()] != sol.tiles[c.index()])
            .count();
        self.tiles = sol.tiles;
        self.passes += 1;
        Ok(Pass {
            region: region.to_vec(),
            trace: keep_region(&sol.trace, &inside),
            tiles,
            changed: u32::try_from(changed).unwrap_or(u32::MAX),
            backtracks: sol.backtracks,
            ok: true,
        })
    }

    /// Free `region`, hold everything else, and solve. If the region cannot
    /// be filled, the world is unchanged and the pass comes back with
    /// `ok == false`. `Err` only for a malformed problem.
    pub fn reshape(
        &mut self,
        ts: &Tileset,
        tables: &Tables,
        region: &[Cell],
        seed: Seed,
        mode: Mode,
    ) -> Result<Pass, SolveError> {
        let pins = self.pins.clone();
        match self.reshape_with(ts, tables, region, &pins, seed, mode) {
            Ok(p) => Ok(p),
            Err(SolveError::Contradiction | SolveError::Exhausted | SolveError::OverBudget(_)) => {
                Ok(Pass {
                    region: region.to_vec(),
                    trace: Vec::new(),
                    tiles: region.iter().map(|c| self.tiles[c.index()]).collect(),
                    changed: 0,
                    backtracks: 0,
                    ok: false,
                })
            }
            Err(e) => Err(e),
        }
    }

    /// Paint `brush` (or erase, with `None`) over `stroke`, then repair a
    /// disc around it. On error the world, pins included, is unchanged.
    pub fn paint(
        &mut self,
        ts: &Tileset,
        tables: &Tables,
        stroke: &[(i64, i64)],
        brush: Option<Brush>,
        salt: u32,
    ) -> Result<Repair, PaintError> {
        if stroke.is_empty() || stroke.len() > MAX_STROKE {
            return Err(PaintError::Stroke(stroke.len()));
        }
        let mut cells = Vec::with_capacity(stroke.len());
        for &(x, y) in stroke {
            cells.push(self.dims.at(x, y).ok_or(PaintError::OffMap { x, y })?);
        }
        let mut pins = self.pins.clone();
        for c in &cells {
            pins[c.index()] = brush;
        }
        // A cheap proof of impossibility before any search: the pins alone,
        // on an otherwise free map.
        let free = vec![true; self.dims.cells()];
        let open = self
            .problem(ts, &pins, &free, Mode::Free)
            .map_err(PaintError::Impossible)?;
        wave::arc_consistent(tables, &open).map_err(PaintError::Impossible)?;

        let seed = self.seed.salted(0x9a17_0000 ^ salt);
        let mut last = SolveError::Exhausted;
        for (grown, radius) in RADII.into_iter().enumerate() {
            let region = self.disc(&cells, radius);
            #[allow(clippy::cast_possible_truncation)] // grown < RADII.len()
            let grown = grown as u32;
            match self.reshape_with(ts, tables, &region, &pins, seed.salted(grown), Mode::Free) {
                Ok(pass) => {
                    self.pins = pins;
                    return Ok(Repair {
                        pass,
                        radius,
                        grown,
                    });
                }
                Err(
                    e @ (SolveError::Contradiction
                    | SolveError::Exhausted
                    | SolveError::OverBudget(_)),
                ) => {
                    last = e;
                }
                Err(e) => return Err(PaintError::Impossible(e)),
            }
        }
        Err(PaintError::Impossible(last))
    }

    /// Reshape a small disc somewhere, chosen by `salt`. Pins still hold.
    pub fn drift(&mut self, ts: &Tileset, tables: &Tables, salt: u32) -> Result<Pass, SolveError> {
        let mut rng = Rng::new(self.seed.salted(0xd21f_0000 ^ salt));
        let c = Cell(rng.below(u32::try_from(self.dims.cells()).unwrap_or(1)));
        let radius = 2 + rng.below(3);
        let region = self.disc(&[c], radius);
        self.reshape(ts, tables, &region, self.seed.salted(salt), Mode::Locked)
    }

    /// Cells within `radius` (Euclidean) of any of `cells`.
    pub fn disc(&self, cells: &[Cell], radius: u32) -> Vec<Cell> {
        let r2 = i64::from(radius) * i64::from(radius);
        let centers: Vec<(i64, i64)> = cells.iter().map(|&c| self.dims.xy(c)).collect();
        (0..self.dims.cells())
            .map(|i| {
                #[allow(clippy::cast_possible_truncation)] // cells ≤ 65536
                Cell(i as u32)
            })
            .filter(|&c| {
                let (x, y) = self.dims.xy(c);
                centers
                    .iter()
                    .any(|&(cx, cy)| (x - cx).pow(2) + (y - cy).pow(2) <= r2)
            })
            .collect()
    }
}

/// Keep only the trace events that concern the region, renumbering rewinds
/// so they count region placements only.
fn keep_region(trace: &[Event], inside: &[bool]) -> Vec<Event> {
    // `kept_before[k]` = how many of the first k placements were inside.
    let mut kept_before: Vec<u32> = vec![0];
    let mut out = Vec::new();
    for &e in trace {
        match e {
            Event::Place { cell, .. } => {
                let keep = inside[cell.index()];
                let last = kept_before.last().copied().unwrap_or(0);
                kept_before.push(last + u32::from(keep));
                if keep {
                    out.push(e);
                }
            }
            Event::Rewind { to } => {
                kept_before.truncate(to as usize + 1);
                out.push(Event::Rewind {
                    to: kept_before.last().copied().unwrap_or(0),
                });
            }
        }
    }
    out
}

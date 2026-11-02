//! The solver: wave function collapse with arc-consistency propagation and
//! chronological backtracking.
//!
//! Every cell holds a *domain* — the set of tiles it could still be — as a
//! bitset. Solving alternates two moves:
//!
//! * **propagate**: for a cell `i` whose domain changed and each neighbour `j`
//!   in direction `d`, keep in `D[j]` only the tiles whose `opposite(d)`
//!   socket is one `D[i]` still shows on side `d`. This is the AC-3 *revise*
//!   step; it never removes a tile that appears in any solution (`docs/SPEC.md`
//!   §3, proved in `verus/src/tessera.rs`).
//! * **decide**: pick the undecided cell with the fewest tiles left (ties
//!   broken by a per-cell random priority), pick one of its tiles by weight,
//!   and propagate. If propagation empties a domain, undo back to the last
//!   decision and ban the tile it chose — a deduction, since that tile led to
//!   no solution — and propagate again.
//!
//! The trace records every cell the moment it is down to one tile, and every
//! rewind, so a browser can replay the search exactly as it ran.

use serde::Serialize;
use thiserror::Error;

use crate::rng::{Rng, Seed};
use crate::tiles::{Dir, Socket, TileId, Tileset};

/// The most tiles a domain bitset holds.
pub const MAX_TILES: usize = 1024;

/// A cell's index, row-major. Newtype so it is never a tile id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct Cell(pub u32);

impl Cell {
    /// As a usize, for indexing.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// The size of a map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct Dims {
    /// Columns.
    pub w: u32,
    /// Rows.
    pub h: u32,
}

impl Dims {
    /// The largest side accepted.
    pub const MAX_SIDE: u32 = 256;

    /// Checked constructor.
    pub const fn new(w: u32, h: u32) -> Result<Self, SolveError> {
        if w == 0 || h == 0 || w > Self::MAX_SIDE || h > Self::MAX_SIDE {
            return Err(SolveError::BadDims { w, h });
        }
        Ok(Self { w, h })
    }

    /// Cells.
    pub const fn cells(self) -> usize {
        (self.w * self.h) as usize
    }

    /// The cell at `(x, y)`, if on the map.
    pub const fn at(self, x: i64, y: i64) -> Option<Cell> {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            None
        } else {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            // bounded above by w*h ≤ 65536
            Some(Cell((y * self.w as i64 + x) as u32))
        }
    }

    /// `(x, y)` of a cell.
    pub const fn xy(self, c: Cell) -> (i64, i64) {
        ((c.0 % self.w) as i64, (c.0 / self.w) as i64)
    }

    /// The neighbour of `c` toward `d`, if on the map.
    pub const fn step(self, c: Cell, d: Dir) -> Option<Cell> {
        let (x, y) = self.xy(c);
        match d {
            Dir::N => self.at(x, y - 1),
            Dir::E => self.at(x + 1, y),
            Dir::S => self.at(x, y + 1),
            Dir::W => self.at(x - 1, y),
        }
    }
}

/// What lies beyond the map's edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Border {
    /// The sea: every map is an island (or several).
    Sea,
    /// Nothing is asked of the edge.
    Open,
}

/// Why a solve did not produce a map.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SolveError {
    /// The dimensions are out of range.
    #[error("a map is 1 to 256 cells on a side, not {w} × {h}")]
    BadDims {
        /// Asked width.
        w: u32,
        /// Asked height.
        h: u32,
    },
    /// The tileset is larger than a domain can hold.
    #[error("{0} tiles is more than a domain holds")]
    TooManyTiles(usize),
    /// Propagation alone emptied a domain: no map satisfies the constraints.
    #[error("those constraints contradict each other")]
    Contradiction,
    /// The search tried everything: no map satisfies the constraints.
    #[error("no map satisfies those constraints")]
    Exhausted,
    /// The search ran out of backtracks before finishing.
    #[error("the search gave up after {0} backtracks")]
    OverBudget(u32),
}

/// One step of a search, for replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Event {
    /// `cell` is down to one tile.
    Place {
        /// Where.
        cell: Cell,
        /// Which tile.
        tile: TileId,
    },
    /// Undo every `Place` after the first `to`.
    Rewind {
        /// How many placements survive.
        to: u32,
    },
}

impl Event {
    /// Compact wire form: a placement is `cell * 1024 + tile`, a rewind is
    /// `-(to + 1)`.
    pub fn encode(self) -> i64 {
        match self {
            Self::Place { cell, tile } => i64::from(cell.0) * 1024 + i64::from(tile.0),
            Self::Rewind { to } => -(i64::from(to) + 1),
        }
    }
}

/// A problem: a map and the starting domain of every cell.
#[derive(Debug, Clone)]
pub struct Problem {
    dims: Dims,
    words: usize,
    tiles: usize,
    domains: Vec<u64>,
    guide: Option<Vec<[u8; 4]>>,
}

/// How strongly the guide pulls: the weight multiplier for a tile, indexed
/// by how many of its four corners match the guide's corners for the cell.
pub const AFFINITY: [u32; 5] = [1, 2, 6, 24, 96];

impl Problem {
    /// Every cell may be any tile, except that with `Border::Sea` an edge
    /// cell must show the sea socket outward.
    pub fn new(ts: &Tileset, dims: Dims, border: Border) -> Result<Self, SolveError> {
        if ts.len() > MAX_TILES {
            return Err(SolveError::TooManyTiles(ts.len()));
        }
        let words = ts.len().div_ceil(64);
        let mut full = vec![0_u64; words];
        for id in ts.ids() {
            full[id.index() / 64] |= 1 << (id.index() % 64);
        }
        let mut domains = Vec::with_capacity(dims.cells() * words);
        for _ in 0..dims.cells() {
            domains.extend_from_slice(&full);
        }
        let mut p = Self {
            dims,
            words,
            tiles: ts.len(),
            domains,
            guide: None,
        };
        if border == Border::Sea {
            for i in 0..dims.cells() {
                #[allow(clippy::cast_possible_truncation)] // cells ≤ 65536
                let c = Cell(i as u32);
                for d in Dir::ALL {
                    if dims.step(c, d).is_none() {
                        let ok: Vec<TileId> = ts
                            .ids()
                            .filter(|&t| ts.socket(t, d) == Socket::SEA)
                            .collect();
                        p.restrict(c, &ok);
                    }
                }
            }
        }
        Ok(p)
    }

    /// The map.
    pub const fn dims(&self) -> Dims {
        self.dims
    }

    /// Bias tile choice toward target corner levels per cell (nw, ne, se,
    /// sw; 0..=3). The guide changes only *which* consistent map is found,
    /// never whether a map is consistent: it scales weights, it does not
    /// remove tiles.
    pub fn guide(&mut self, guide: Vec<[u8; 4]>) {
        if guide.len() == self.dims.cells() {
            self.guide = Some(guide);
        }
    }

    /// Intersect `c`'s domain with `allowed`.
    pub fn restrict(&mut self, c: Cell, allowed: &[TileId]) {
        let mut mask = vec![0_u64; self.words];
        for t in allowed {
            if t.index() < self.tiles {
                mask[t.index() / 64] |= 1 << (t.index() % 64);
            }
        }
        let base = c.index() * self.words;
        for (w, m) in mask.iter().enumerate() {
            self.domains[base + w] &= m;
        }
    }

    /// Fix `c` to exactly `t` (intersected with what it already allows).
    pub fn fix(&mut self, c: Cell, t: TileId) {
        self.restrict(c, &[t]);
    }

    /// Whether `t` is still in `c`'s domain.
    pub fn allows(&self, c: Cell, t: TileId) -> bool {
        t.index() < self.tiles
            && self.domains[c.index() * self.words + t.index() / 64] & (1 << (t.index() % 64)) != 0
    }

    /// The number of tiles `c` still allows.
    pub fn count(&self, c: Cell) -> u32 {
        let base = c.index() * self.words;
        self.domains[base..base + self.words]
            .iter()
            .map(|w| w.count_ones())
            .sum()
    }
}

/// A solved map and how it was found.
#[derive(Debug, Clone, Serialize)]
pub struct Solution {
    /// One tile per cell, row-major.
    pub tiles: Vec<TileId>,
    /// The search, for replay.
    pub trace: Vec<Event>,
    /// Decisions taken (including ones later undone).
    pub decisions: u32,
    /// Backtracks taken.
    pub backtracks: u32,
    /// Fresh starts taken before this one succeeded (0 = first try).
    pub restarts: u32,
}

/// Precomputed socket tables for one tileset.
#[derive(Debug, Clone)]
pub struct Tables {
    words: usize,
    weights: Vec<u32>,
    /// Each tile's corner levels.
    corners: Vec<[u8; 4]>,
    /// `sock[t][d]`.
    sock: Vec<[u8; 4]>,
    /// `with[d][s * words ..]`: the tiles showing socket `s` on side `d`.
    with: [Vec<u64>; 4],
}

impl Tables {
    /// Build the tables.
    pub fn new(ts: &Tileset) -> Self {
        let words = ts.len().div_ceil(64);
        let sock: Vec<[u8; 4]> = ts
            .ids()
            .map(|t| Dir::ALL.map(|d| ts.socket(t, d).0))
            .collect();
        let mut with: [Vec<u64>; 4] = std::array::from_fn(|_| vec![0; Socket::COUNT * words]);
        for (t, s) in sock.iter().enumerate() {
            for d in 0..4 {
                with[d][usize::from(s[d]) * words + t / 64] |= 1 << (t % 64);
            }
        }
        Self {
            words,
            weights: ts.tiles().iter().map(|t| t.weight).collect(),
            corners: ts
                .tiles()
                .iter()
                .map(|t| t.corners.map(|c| c as u8))
                .collect(),
            sock,
            with,
        }
    }
}

struct Decision {
    cell: Cell,
    tile: usize,
    trail: usize,
    placed: u32,
}

struct Search<'a> {
    tables: &'a Tables,
    dims: Dims,
    words: usize,
    dom: Vec<u64>,
    count: Vec<u32>,
    trail_cells: Vec<u32>,
    trail_saved: Vec<u64>,
    queue: Vec<u32>,
    queued: Vec<bool>,
    trace: Vec<Event>,
    placed: u32,
}

impl<'a> Search<'a> {
    fn new(tables: &'a Tables, problem: &Problem) -> Self {
        let n = problem.dims.cells();
        let words = tables.words;
        let count = (0..n)
            .map(|c| {
                problem.domains[c * words..(c + 1) * words]
                    .iter()
                    .map(|w| w.count_ones())
                    .sum()
            })
            .collect();
        Self {
            tables,
            dims: problem.dims,
            words,
            dom: problem.domains.clone(),
            count,
            trail_cells: Vec::new(),
            trail_saved: Vec::new(),
            queue: Vec::new(),
            queued: vec![false; n],
            trace: Vec::new(),
            placed: 0,
        }
    }

    fn domain(&self, c: usize) -> &[u64] {
        &self.dom[c * self.words..(c + 1) * self.words]
    }

    /// Replace `c`'s domain, saving the old one on the trail. Returns `false`
    /// if the new domain is empty.
    fn set(&mut self, c: usize, new: &[u64]) -> bool {
        let base = c * self.words;
        self.trail_cells.push(u32::try_from(c).unwrap_or(u32::MAX));
        self.trail_saved
            .extend_from_slice(&self.dom[base..base + self.words]);
        let before = self.count[c];
        self.dom[base..base + self.words].copy_from_slice(new);
        let after: u32 = new.iter().map(|w| w.count_ones()).sum();
        self.count[c] = after;
        if after == 1
            && before > 1
            && let Some(t) = first_bit(new)
        {
            self.placed += 1;
            #[allow(clippy::cast_possible_truncation)] // t < MAX_TILES
            self.trace.push(Event::Place {
                cell: Cell(u32::try_from(c).unwrap_or(u32::MAX)),
                tile: TileId(t as u16),
            });
        }
        after > 0
    }

    fn undo_to(&mut self, len: usize) {
        while self.trail_cells.len() > len {
            let Some(c) = self.trail_cells.pop() else {
                break;
            };
            let c = c as usize;
            let at = self.trail_saved.len() - self.words;
            let base = c * self.words;
            self.dom[base..base + self.words].copy_from_slice(&self.trail_saved[at..]);
            self.trail_saved.truncate(at);
            self.count[c] = self.domain(c).iter().map(|w| w.count_ones()).sum();
        }
    }

    fn enqueue(&mut self, c: usize) {
        if !self.queued[c] {
            self.queued[c] = true;
            self.queue.push(u32::try_from(c).unwrap_or(u32::MAX));
        }
    }

    /// Run propagation to a fixed point. `false` on contradiction.
    fn propagate(&mut self) -> bool {
        let mut allowed = vec![0_u64; self.words];
        let mut next = vec![0_u64; self.words];
        let mut head = 0;
        let mut ok = true;
        while head < self.queue.len() {
            let i = self.queue[head] as usize;
            head += 1;
            self.queued[i] = false;
            if !ok {
                continue;
            }
            #[allow(clippy::cast_possible_truncation)]
            let ci = Cell(i as u32);
            for d in Dir::ALL {
                let Some(cj) = self.dims.step(ci, d) else {
                    continue;
                };
                let j = cj.index();
                // The sockets D[i] still shows toward d.
                let mut present: u64 = 0;
                for (w, &bits) in self.domain(i).iter().enumerate() {
                    let mut b = bits;
                    while b != 0 {
                        let t = w * 64 + b.trailing_zeros() as usize;
                        present |= 1 << self.tables.sock[t][d.index()];
                        b &= b - 1;
                    }
                }
                // The tiles that show one of them back.
                allowed.fill(0);
                let back = &self.tables.with[d.opposite().index()];
                let mut p = present;
                while p != 0 {
                    let s = p.trailing_zeros() as usize;
                    for (w, a) in allowed.iter_mut().enumerate() {
                        *a |= back[s * self.words + w];
                    }
                    p &= p - 1;
                }
                let mut changed = false;
                for (w, n) in next.iter_mut().enumerate() {
                    *n = self.dom[j * self.words + w] & allowed[w];
                    changed |= *n != self.dom[j * self.words + w];
                }
                if changed {
                    if !self.set(j, &next) {
                        ok = false;
                        break;
                    }
                    self.enqueue(j);
                }
            }
        }
        self.queue.clear();
        ok
    }

    fn weight(&self, t: usize, target: Option<[u8; 4]>) -> u32 {
        let base = self.tables.weights[t];
        target.map_or(base, |g| {
            let k = (0..4)
                .filter(|&i| self.tables.corners[t][i] == g[i])
                .count();
            base * AFFINITY[k]
        })
    }

    fn pick_tile(&self, c: usize, target: Option<[u8; 4]>, rng: &mut Rng) -> usize {
        let dom = self.domain(c);
        let mut total: u32 = 0;
        for_each_bit(dom, |t| total += self.weight(t, target));
        let mut r = rng.below(total);
        let mut chosen = first_bit(dom).unwrap_or(0);
        let mut done = false;
        for_each_bit(dom, |t| {
            if done {
                return;
            }
            let w = self.weight(t, target);
            if r < w {
                chosen = t;
                done = true;
            } else {
                r -= w;
            }
        });
        chosen
    }
}

fn first_bit(bits: &[u64]) -> Option<usize> {
    bits.iter()
        .enumerate()
        .find(|(_, w)| **w != 0)
        .map(|(i, w)| i * 64 + w.trailing_zeros() as usize)
}

fn for_each_bit(bits: &[u64], mut f: impl FnMut(usize)) {
    for (w, &word) in bits.iter().enumerate() {
        let mut b = word;
        while b != 0 {
            f(w * 64 + b.trailing_zeros() as usize);
            b &= b - 1;
        }
    }
}

/// Choose the next cell to decide: fewest tiles left, then lowest priority,
/// then lowest index. `None` when every cell is down to one tile.
fn choose(count: &[u32], priority: &[u32]) -> Option<usize> {
    let mut best: Option<(u32, u32, usize)> = None;
    for (c, (&k, &p)) in count.iter().zip(priority).enumerate() {
        if k > 1 {
            let key = (k, p, c);
            if best.is_none_or(|b| key < b) {
                best = Some(key);
            }
        }
    }
    best.map(|(_, _, c)| c)
}

/// Solve once from `seed`, giving up after `budget` backtracks.
pub fn solve_once(
    tables: &Tables,
    problem: &Problem,
    seed: Seed,
    budget: u32,
) -> Result<Solution, SolveError> {
    let n = problem.dims.cells();
    let words = tables.words;
    let mut rng = Rng::new(seed);
    let priority: Vec<u32> = (0..n).map(|_| rng.next_u32()).collect();
    let mut s = Search::new(tables, problem);
    if s.count.contains(&0) {
        return Err(SolveError::Contradiction);
    }
    // Cells that start as single tiles are placements too.
    for c in 0..n {
        if s.count[c] == 1
            && let Some(t) = first_bit(s.domain(c))
        {
            s.placed += 1;
            #[allow(clippy::cast_possible_truncation)]
            s.trace.push(Event::Place {
                cell: Cell(c as u32),
                tile: TileId(t as u16),
            });
        }
        s.enqueue(c);
    }
    if !s.propagate() {
        return Err(SolveError::Contradiction);
    }
    let mut stack: Vec<Decision> = Vec::new();
    let mut decisions = 0;
    let mut backtracks = 0;
    while let Some(c) = choose(&s.count, &priority) {
        decisions += 1;
        let target = problem.guide.as_ref().map(|g| g[c]);
        let tile = s.pick_tile(c, target, &mut rng);
        stack.push(Decision {
            #[allow(clippy::cast_possible_truncation)]
            cell: Cell(c as u32),
            tile,
            trail: s.trail_cells.len(),
            placed: s.placed,
        });
        let mut only = vec![0_u64; words];
        only[tile / 64] |= 1 << (tile % 64);
        let mut ok = s.set(c, &only);
        if ok {
            s.enqueue(c);
            ok = s.propagate();
        }
        while !ok {
            let Some(d) = stack.pop() else {
                return Err(SolveError::Exhausted);
            };
            backtracks += 1;
            if backtracks > budget {
                return Err(SolveError::OverBudget(budget));
            }
            s.undo_to(d.trail);
            s.placed = d.placed;
            s.trace.push(Event::Rewind { to: d.placed });
            let ci = d.cell.index();
            let mut banned = s.domain(ci).to_vec();
            banned[d.tile / 64] &= !(1 << (d.tile % 64));
            ok = s.set(ci, &banned);
            if ok {
                s.enqueue(ci);
                ok = s.propagate();
            }
        }
    }
    let tiles = (0..n)
        .map(|c| {
            #[allow(clippy::cast_possible_truncation)]
            TileId(first_bit(s.domain(c)).unwrap_or(0) as u16)
        })
        .collect();
    Ok(Solution {
        tiles,
        trace: s.trace,
        decisions,
        backtracks,
        restarts: 0,
    })
}

/// Solve, starting afresh with a salted seed whenever a run goes over
/// `budget` backtracks, up to `restarts` times. A contradiction or an
/// exhausted search is final: no restart can change it.
pub fn solve(
    tables: &Tables,
    problem: &Problem,
    seed: Seed,
    budget: u32,
    restarts: u32,
) -> Result<Solution, SolveError> {
    let mut last = SolveError::OverBudget(budget);
    for k in 0..=restarts {
        match solve_once(tables, problem, seed.salted(k), budget) {
            Ok(mut sol) => {
                sol.restarts = k;
                return Ok(sol);
            }
            Err(e @ SolveError::OverBudget(_)) => last = e,
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

/// Propagate a problem to its arc-consistent fixed point without searching.
/// `Err(Contradiction)` if that empties a domain — a cheap proof that no map
/// exists.
pub fn arc_consistent(tables: &Tables, problem: &Problem) -> Result<Problem, SolveError> {
    let n = problem.dims.cells();
    let mut s = Search::new(tables, problem);
    if s.count.contains(&0) {
        return Err(SolveError::Contradiction);
    }
    for c in 0..n {
        s.enqueue(c);
    }
    if !s.propagate() {
        return Err(SolveError::Contradiction);
    }
    Ok(Problem {
        dims: problem.dims,
        words: problem.words,
        tiles: problem.tiles,
        domains: s.dom,
        guide: problem.guide.clone(),
    })
}

/// Replay a trace: the placements still standing at the end, in order.
/// A browser does exactly this to animate a search.
pub fn replay(trace: &[Event]) -> Vec<(Cell, TileId)> {
    let mut live = Vec::new();
    for e in trace {
        match *e {
            Event::Place { cell, tile } => live.push((cell, tile)),
            Event::Rewind { to } => live.truncate(to as usize),
        }
    }
    live
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::check;

    fn small(w: u32, h: u32) -> (Tileset, Tables, Dims) {
        let ts = Tileset::island();
        let tables = Tables::new(&ts);
        (ts, tables, Dims::new(w, h).unwrap())
    }

    #[test]
    fn a_small_open_map_solves_and_checks() {
        let (ts, tables, dims) = small(10, 8);
        let p = Problem::new(&ts, dims, Border::Open).unwrap();
        let sol = solve(&tables, &p, Seed(3), 500, 3).unwrap();
        check(&ts, dims, Border::Open, &sol.tiles).unwrap();
    }

    #[test]
    fn a_small_island_solves_and_meets_the_sea() {
        let (ts, tables, dims) = small(10, 8);
        let p = Problem::new(&ts, dims, Border::Sea).unwrap();
        let sol = solve(&tables, &p, Seed(9), 500, 3).unwrap();
        check(&ts, dims, Border::Sea, &sol.tiles).unwrap();
    }

    #[test]
    fn replaying_a_full_solve_places_every_cell_once() {
        let (ts, tables, dims) = small(9, 7);
        let p = Problem::new(&ts, dims, Border::Open).unwrap();
        let sol = solve_once(&tables, &p, Seed(11), 500).unwrap();
        let live = replay(&sol.trace);
        assert_eq!(live.len(), dims.cells());
        for (c, t) in live {
            assert_eq!(sol.tiles[c.index()], t);
        }
    }

    #[test]
    fn a_town_on_the_shore_is_a_contradiction() {
        let (ts, tables, dims) = small(6, 6);
        let mut p = Problem::new(&ts, dims, Border::Sea).unwrap();
        p.restrict(Cell(0), &ts.brush(crate::tiles::Brush::Town));
        assert_eq!(
            arc_consistent(&tables, &p).unwrap_err(),
            SolveError::Contradiction
        );
        assert_eq!(
            solve(&tables, &p, Seed(1), 50, 0).unwrap_err(),
            SolveError::Contradiction
        );
    }

    #[test]
    fn propagation_keeps_every_tile_of_a_known_solution() {
        let (ts, tables, dims) = small(8, 8);
        let p = Problem::new(&ts, dims, Border::Sea).unwrap();
        let sol = solve(&tables, &p, Seed(5), 500, 3).unwrap();
        let mut q = p;
        for i in [0_u32, 9, 27, 45, 63] {
            q.fix(Cell(i), sol.tiles[i as usize]);
        }
        let ac = arc_consistent(&tables, &q).unwrap();
        for (i, &t) in sol.tiles.iter().enumerate() {
            assert!(ac.allows(Cell(u32::try_from(i).unwrap()), t), "cell {i}");
        }
    }

    #[test]
    fn events_encode_compactly() {
        let place = Event::Place {
            cell: Cell(3),
            tile: TileId(7),
        };
        assert_eq!(place.encode(), 3 * 1024 + 7);
        assert_eq!(Event::Rewind { to: 0 }.encode(), -1);
        assert_eq!(Event::Rewind { to: 41 }.encode(), -42);
    }

    #[test]
    fn dims_are_checked() {
        assert!(Dims::new(0, 4).is_err());
        assert!(Dims::new(257, 4).is_err());
        let d = Dims::new(5, 4).unwrap();
        assert_eq!(d.at(-1, 0), None);
        assert_eq!(d.at(4, 3), Some(Cell(19)));
        assert_eq!(d.step(Cell(0), Dir::N), None);
        assert_eq!(d.step(Cell(0), Dir::E), Some(Cell(1)));
        assert_eq!(d.xy(Cell(7)), (2, 1));
    }
}

//! The tileset: every tile, its sockets, its weight, and which brushes it
//! answers to.
//!
//! A tile is four corner heights and four edge features. Heights are
//! [`Level`]s — sea, grass, forest, mountain — and an edge carries nothing, a
//! road, or a river. The *socket* on an edge is the pair of corner heights at
//! its two ends plus its feature, read in absolute orientation (west→east for
//! a horizontal edge, north→south for a vertical one). Two tiles may share an
//! edge iff the sockets they show on it are equal. That is the only rule.
//!
//! The tileset is enumerated, not hand-listed, in a fixed order the browser
//! port repeats exactly (see `docs/SPEC.md` §2).

use serde::Serialize;

/// A corner height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[repr(u8)]
pub enum Level {
    /// Open water.
    Sea = 0,
    /// Meadow.
    Grass = 1,
    /// Woodland.
    Forest = 2,
    /// Rock and snow.
    Mountain = 3,
}

impl Level {
    /// All levels, in order.
    pub const ALL: [Self; 4] = [Self::Sea, Self::Grass, Self::Forest, Self::Mountain];

    /// From 0..=3; anything higher is a mountain.
    pub const fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Sea,
            1 => Self::Grass,
            2 => Self::Forest,
            _ => Self::Mountain,
        }
    }
}

/// What runs along an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[repr(u8)]
pub enum Feature {
    /// Nothing.
    None = 0,
    /// A road crosses this edge.
    Road = 1,
    /// A river crosses this edge.
    River = 2,
}

impl Feature {
    /// All features, in order.
    pub const ALL: [Self; 3] = [Self::None, Self::Road, Self::River];
}

/// What sits in the middle of a tile, for drawing and for brushes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Center {
    /// Terrain only, or a road or river passing through.
    Plain,
    /// A walled town. Every road dead end is one of these.
    Town,
    /// Where a river rises, in the mountains.
    Spring,
    /// Where a river meets the sea.
    Mouth,
    /// A road crossing a river.
    Bridge,
}

/// A direction from a cell to its neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dir {
    /// Up.
    N = 0,
    /// Right.
    E = 1,
    /// Down.
    S = 2,
    /// Left.
    W = 3,
}

impl Dir {
    /// All four, in index order.
    pub const ALL: [Self; 4] = [Self::N, Self::E, Self::S, Self::W];

    /// The direction back.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::N => Self::S,
            Self::E => Self::W,
            Self::S => Self::N,
            Self::W => Self::E,
        }
    }

    /// Index 0..4.
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// A socket: what an edge looks like from either side.
///
/// Encoded as `a * 12 + b * 3 + feature` where `a`, `b` are the corner levels
/// at the edge's two ends in absolute order. There are 48 of them, so a set
/// of sockets fits in one `u64`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct Socket(pub u8);

impl Socket {
    /// How many distinct sockets exist.
    pub const COUNT: usize = 48;
    /// The all-sea socket: the outside of an island.
    pub const SEA: Self = Self::new(Level::Sea, Level::Sea, Feature::None);

    /// A socket from its parts.
    pub const fn new(a: Level, b: Level, f: Feature) -> Self {
        Self(a as u8 * 12 + b as u8 * 3 + f as u8)
    }
}

/// A tile's index in the tileset. Newtype so it is never a cell index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct TileId(pub u16);

impl TileId {
    /// As a usize, for indexing.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// One tile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tile {
    /// Corner heights: north-west, north-east, south-east, south-west.
    pub corners: [Level; 4],
    /// Edge features: north, east, south, west.
    pub edges: [Feature; 4],
    /// What is in the middle.
    pub center: Center,
    /// Relative frequency. Positive.
    pub weight: u32,
}

impl Tile {
    /// The socket this tile shows on the edge in `dir`.
    pub const fn socket(&self, dir: Dir) -> Socket {
        let [nw, ne, se, sw] = self.corners;
        let (a, b) = match dir {
            Dir::N => (nw, ne),
            Dir::E => (ne, se),
            Dir::S => (sw, se),
            Dir::W => (nw, sw),
        };
        Socket::new(a, b, self.edges[dir.index()])
    }

    fn count(&self, f: Feature) -> usize {
        self.edges.iter().filter(|&&e| e == f).count()
    }

    /// Roads crossing the edge.
    pub fn roads(&self) -> usize {
        self.count(Feature::Road)
    }

    /// Rivers crossing the edge.
    pub fn rivers(&self) -> usize {
        self.count(Feature::River)
    }

    fn uniform(&self, l: Level) -> bool {
        self.corners.iter().all(|&c| c == l)
    }
}

/// What a brush asks of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Brush {
    /// Open sea, no features.
    Sea,
    /// Meadow, no features.
    Grass,
    /// Woodland, no features.
    Forest,
    /// High rock, no features.
    Mountain,
    /// A town.
    Town,
    /// Any tile a road crosses.
    Road,
    /// Any tile a river crosses.
    River,
}

impl Brush {
    /// Every brush.
    pub const ALL: [Self; 7] = [
        Self::Sea,
        Self::Grass,
        Self::Forest,
        Self::Mountain,
        Self::Town,
        Self::Road,
        Self::River,
    ];

    /// Whether `tile` satisfies this brush.
    pub fn accepts(self, tile: &Tile) -> bool {
        let plain = tile.roads() == 0 && tile.rivers() == 0;
        match self {
            Self::Sea => plain && tile.uniform(Level::Sea),
            Self::Grass => plain && tile.uniform(Level::Grass),
            Self::Forest => plain && tile.uniform(Level::Forest),
            Self::Mountain => plain && tile.uniform(Level::Mountain),
            Self::Town => tile.center == Center::Town,
            Self::Road => tile.roads() > 0 && tile.center != Center::Town,
            Self::River => tile.rivers() > 0,
        }
    }

    /// Its name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Sea => "sea",
            Self::Grass => "grass",
            Self::Forest => "forest",
            Self::Mountain => "mountain",
            Self::Town => "town",
            Self::Road => "road",
            Self::River => "river",
        }
    }
}

const fn close(a: Level, b: Level) -> bool {
    (a as i8 - b as i8).abs() <= 1
}

/// Whether four corners form a legal height pattern.
///
/// Every pair of corners — edge-adjacent or diagonal — differs by at most one
/// level. The terrain
/// therefore never jumps from sea straight to forest inside a tile, and since
/// corners are shared, never across tiles either.
pub const fn smooth(c: [Level; 4]) -> bool {
    close(c[0], c[1])
        && close(c[1], c[2])
        && close(c[2], c[3])
        && close(c[3], c[0])
        && close(c[0], c[2])
        && close(c[1], c[3])
}

fn plain_weight(c: [Level; 4]) -> u32 {
    let lo = c.iter().min().copied().unwrap_or(Level::Sea);
    let hi = c.iter().max().copied().unwrap_or(Level::Sea);
    if lo == hi {
        match lo {
            Level::Sea => 240,
            Level::Grass => 640,
            Level::Forest => 560,
            Level::Mountain => 440,
        }
    } else if lo == Level::Sea {
        // Coastline pieces: common enough that coasts are long and ragged.
        120
    } else {
        160
    }
}

/// The corners an edge in `dir` touches, as indices into `corners`.
const fn edge_corners(dir: usize) -> (usize, usize) {
    match dir {
        0 => (0, 1),
        1 => (1, 2),
        2 => (3, 2),
        _ => (0, 3),
    }
}

/// Classify a (corners, edges) pair: the tile it is, if it is one.
fn classify(corners: [Level; 4], edges: [Feature; 4]) -> Option<(Center, u32)> {
    let roads = edges.iter().filter(|&&f| f == Feature::Road).count();
    let rivers = edges.iter().filter(|&&f| f == Feature::River).count();
    let all = |p: fn(Level) -> bool| corners.iter().all(|&l| p(l));
    let lowland = |l: Level| l == Level::Grass || l == Level::Forest;
    let land = |l: Level| l != Level::Sea;
    // A two-ended feature that runs straight through, not round a corner.
    let straight = (edges[0] == edges[2] && edges[0] != Feature::None)
        || (edges[1] == edges[3] && edges[1] != Feature::None);

    if roads == 0 && rivers == 0 {
        return Some((Center::Plain, plain_weight(corners)));
    }
    if rivers == 0 {
        // Roads: on grass or through woods, never on rock or water.
        if all(|l| l == Level::Grass) {
            // A dead end is only ever a town, and towns are added separately
            // by `Tileset::island`, so a single road edge is no plain tile.
            return match roads {
                1 => None,
                2 if straight => Some((Center::Plain, 10)),
                2 => Some((Center::Plain, 5)),
                _ => Some((Center::Plain, 2)),
            };
        }
        if all(lowland) && roads == 2 {
            return Some((Center::Plain, if straight { 4 } else { 2 }));
        }
        return None;
    }
    if roads == 0 {
        if !all(land) {
            // A mouth: one river edge, and the opposite edge entirely sea.
            if rivers != 1 {
                return None;
            }
            let d = edges.iter().position(|&f| f == Feature::River)?;
            let (a, b) = edge_corners(d);
            let (oa, ob) = edge_corners((d + 2) % 4);
            if land(corners[a])
                && land(corners[b])
                && corners[oa] == Level::Sea
                && corners[ob] == Level::Sea
            {
                return Some((Center::Mouth, 4));
            }
            return None;
        }
        return match rivers {
            1 if corners.contains(&Level::Mountain) => Some((Center::Spring, 4)),
            2 if straight => Some((Center::Plain, 2)),
            2 => Some((Center::Plain, 1)),
            _ => None,
        };
    }
    // A road crossing a river: the two must be straight and perpendicular.
    if roads == 2 && rivers == 2 && edges[0] == edges[2] && edges[1] == edges[3] && all(lowland) {
        return Some((Center::Bridge, 2));
    }
    None
}

/// The whole tileset, in canonical order.
#[derive(Debug, Clone)]
pub struct Tileset {
    tiles: Vec<Tile>,
    /// `sockets[t][d]`: socket of tile `t` on the edge in direction `d`.
    sockets: Vec<[Socket; 4]>,
}

impl Tileset {
    /// Enumerate the tileset.
    ///
    /// Order: corners `nw, ne, se, sw` each over `Level::ALL` (outermost
    /// first), then edges `n, e, s, w` each over `Feature::ALL`; a town tile,
    /// when one exists for the pattern, immediately follows its road piece's
    /// slot. The browser port enumerates in exactly this order.
    pub fn island() -> Self {
        let mut tiles = Vec::new();
        for nw in Level::ALL {
            for ne in Level::ALL {
                for se in Level::ALL {
                    for sw in Level::ALL {
                        let c = [nw, ne, se, sw];
                        if !smooth(c) {
                            continue;
                        }
                        for fe in 0..81_u32 {
                            let pick = |k: u32| Feature::ALL[((fe / 3_u32.pow(k)) % 3) as usize];
                            let e = [pick(3), pick(2), pick(1), pick(0)];
                            if let Some((center, weight)) = classify(c, e) {
                                tiles.push(Tile {
                                    corners: c,
                                    edges: e,
                                    center,
                                    weight,
                                });
                            }
                            let roads = e.iter().filter(|&&f| f == Feature::Road).count();
                            let rivers = e.iter().filter(|&&f| f == Feature::River).count();
                            if rivers == 0 && roads >= 1 && c.iter().all(|&l| l == Level::Grass) {
                                tiles.push(Tile {
                                    corners: c,
                                    edges: e,
                                    center: Center::Town,
                                    weight: if roads == 1 { 5 } else { 3 },
                                });
                            }
                        }
                    }
                }
            }
        }
        let sockets = tiles
            .iter()
            .map(|t| Dir::ALL.map(|d| t.socket(d)))
            .collect();
        Self { tiles, sockets }
    }

    /// How many tiles.
    pub const fn len(&self) -> usize {
        self.tiles.len()
    }

    /// Never true for the island set; here for completeness.
    pub const fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// A tile by id.
    pub fn tile(&self, id: TileId) -> &Tile {
        &self.tiles[id.index()]
    }

    /// Every tile.
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    /// The socket tile `id` shows toward `dir`.
    pub fn socket(&self, id: TileId, dir: Dir) -> Socket {
        self.sockets[id.index()][dir.index()]
    }

    /// Whether `a` may sit with `b` on its `dir` side.
    pub fn fits(&self, a: TileId, dir: Dir, b: TileId) -> bool {
        self.socket(a, dir) == self.socket(b, dir.opposite())
    }

    /// The plain (featureless) tile with these corners, if they are smooth.
    pub fn plain(&self, corners: [Level; 4]) -> Option<TileId> {
        self.ids().find(|&id| {
            let t = self.tile(id);
            t.corners == corners && t.roads() == 0 && t.rivers() == 0
        })
    }

    /// The tile ids a brush accepts.
    pub fn brush(&self, brush: Brush) -> Vec<TileId> {
        self.ids()
            .filter(|&id| brush.accepts(self.tile(id)))
            .collect()
    }

    /// Every id.
    pub fn ids(&self) -> impl Iterator<Item = TileId> + '_ {
        (0..self.tiles.len()).map(|i| TileId(u16::try_from(i).unwrap_or(u16::MAX)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_island_set_is_a_few_hundred_tiles_and_fits_the_bitset() {
        let ts = Tileset::island();
        assert!(ts.len() > 100, "{}", ts.len());
        assert!(ts.len() <= crate::wave::MAX_TILES, "{}", ts.len());
    }

    #[test]
    fn every_brush_has_tiles() {
        let ts = Tileset::island();
        for b in Brush::ALL {
            assert!(!ts.brush(b).is_empty(), "{b:?}");
        }
    }

    #[test]
    fn every_tile_is_smooth_and_weighted() {
        let ts = Tileset::island();
        for t in ts.tiles() {
            assert!(smooth(t.corners));
            assert!(t.weight > 0);
        }
    }

    #[test]
    fn dead_ends_are_only_towns_springs_and_mouths() {
        let ts = Tileset::island();
        for t in ts.tiles() {
            if t.roads() == 1 && t.rivers() == 0 {
                assert_eq!(t.center, Center::Town, "{t:?}");
            }
            if t.rivers() == 1 {
                assert!(matches!(t.center, Center::Spring | Center::Mouth), "{t:?}");
            }
        }
    }

    #[test]
    fn features_only_cross_land() {
        let ts = Tileset::island();
        for t in ts.tiles() {
            for d in Dir::ALL {
                if t.edges[d.index()] != Feature::None {
                    let (a, b) = edge_corners(d.index());
                    assert_ne!(t.corners[a], Level::Sea, "{t:?}");
                    assert_ne!(t.corners[b], Level::Sea, "{t:?}");
                }
                if t.edges[d.index()] == Feature::Road {
                    let (a, b) = edge_corners(d.index());
                    assert_ne!(t.corners[a], Level::Mountain, "{t:?}");
                    assert_ne!(t.corners[b], Level::Mountain, "{t:?}");
                }
            }
        }
    }

    #[test]
    fn sockets_agree_with_shared_corners() {
        // East of a, west of b: a's NE/SE corners are b's NW/SW corners.
        let ts = Tileset::island();
        for a in ts.ids() {
            for b in ts.ids() {
                if ts.fits(a, Dir::E, b) {
                    let (ta, tb) = (ts.tile(a), ts.tile(b));
                    assert_eq!(ta.corners[1], tb.corners[0]);
                    assert_eq!(ta.corners[2], tb.corners[3]);
                    assert_eq!(ta.edges[1], tb.edges[3]);
                }
            }
        }
    }

    #[test]
    fn fits_is_symmetric() {
        let ts = Tileset::island();
        for a in ts.ids().step_by(3) {
            for b in ts.ids().step_by(5) {
                for d in Dir::ALL {
                    assert_eq!(ts.fits(a, d, b), ts.fits(b, d.opposite(), a));
                }
            }
        }
    }
}

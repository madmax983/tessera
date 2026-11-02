//! An independent checker: does a finished map keep every promise?
//!
//! Written without reference to the solver's bitsets, so a solver bug cannot
//! hide behind a shared helper. The same predicate is the `consistent` spec in
//! `verus/src/tessera.rs`.

use thiserror::Error;

use crate::tiles::{Dir, Socket, TileId, Tileset};
use crate::wave::{Border, Cell, Dims};

/// A broken promise.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Violation {
    /// The map has the wrong number of cells.
    #[error("{got} tiles for a map of {want} cells")]
    Size {
        /// Cells on the map.
        want: usize,
        /// Tiles given.
        got: usize,
    },
    /// A tile id is not in the tileset.
    #[error("cell {0:?} holds tile {1:?}, which does not exist")]
    Unknown(Cell, TileId),
    /// Two neighbours disagree about their shared edge.
    #[error("cells {0:?} and {1:?} disagree about the edge between them")]
    Seam(Cell, Cell),
    /// An edge cell shows land, a road or a river to the open sea.
    #[error("cell {0:?} spills off the edge of the map")]
    Shore(Cell),
}

/// Check every seam of a finished map.
pub fn check(ts: &Tileset, dims: Dims, border: Border, tiles: &[TileId]) -> Result<(), Violation> {
    if tiles.len() != dims.cells() {
        return Err(Violation::Size {
            want: dims.cells(),
            got: tiles.len(),
        });
    }
    for (i, &t) in tiles.iter().enumerate() {
        #[allow(clippy::cast_possible_truncation)] // cells ≤ 65536
        let c = Cell(i as u32);
        if t.index() >= ts.len() {
            return Err(Violation::Unknown(c, t));
        }
        for d in [Dir::E, Dir::S] {
            if let Some(n) = dims.step(c, d) {
                let u = tiles[n.index()];
                if u.index() >= ts.len() {
                    return Err(Violation::Unknown(n, u));
                }
                if !ts.fits(t, d, u) {
                    return Err(Violation::Seam(c, n));
                }
            }
        }
        if border == Border::Sea {
            for d in Dir::ALL {
                if dims.step(c, d).is_none() && ts.socket(t, d) != Socket::SEA {
                    return Err(Violation::Shore(c));
                }
            }
        }
    }
    Ok(())
}

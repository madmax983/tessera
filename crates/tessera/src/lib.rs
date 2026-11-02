//! # tessera
//!
//! Wave function collapse over an island tileset. The engine of Tessera, with
//! no IO, no async and no clock: everything is a pure function of a seed.
//!
//! * [`tiles`] — the tileset: corner heights, edge features, sockets, brushes.
//! * [`wave`]  — the solver: propagation, decisions, backtracking, the trace.
//! * [`check`] — an independent checker for finished maps.
//! * [`world`] — a paintable world and repair in blocks.
//! * [`rng`]   — mulberry32 and word seeds, portable to JavaScript bit for bit.
//!
//! ```
//! use tessera::{rng::Seed, tiles::Tileset, wave::{Border, Dims, Tables}, world::World};
//! let ts = Tileset::island();
//! let tables = Tables::new(&ts);
//! let dims = Dims::new(24, 16).unwrap();
//! let (world, _trace) = World::generate(&ts, &tables, dims, Border::Sea, Seed::from_word("avalon")).unwrap();
//! assert!(tessera::check::check(&ts, dims, Border::Sea, world.tiles()).is_ok());
//! ```

pub mod check;
pub mod rng;
pub mod tiles;
pub mod wave;
pub mod world;

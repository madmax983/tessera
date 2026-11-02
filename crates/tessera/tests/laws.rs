//! The promises, as properties. Each one is a line of docs/SPEC.md.

// Test arithmetic is on maps of at most 50 × 40 cells; the numeric casts
// below cannot truncate or wrap at those sizes.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap
)]

use proptest::prelude::*;
use tessera::check::check;
use tessera::rng::Seed;
use tessera::tiles::{Brush, Center, Dir, Feature, Tileset};
use tessera::wave::{self, Border, Cell, Dims, Event, Tables};
use tessera::world::{self, World};

fn kit() -> (Tileset, Tables) {
    let ts = Tileset::island();
    let tables = Tables::new(&ts);
    (ts, tables)
}

fn brush() -> impl Strategy<Value = Option<Brush>> {
    prop_oneof![
        Just(None),
        Just(Some(Brush::Sea)),
        Just(Some(Brush::Grass)),
        Just(Some(Brush::Forest)),
        Just(Some(Brush::Mountain)),
        Just(Some(Brush::Town)),
        Just(Some(Brush::Road)),
        Just(Some(Brush::River)),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// §4.1 Every generated world keeps every promise, and is a function of
    /// its seed alone.
    #[test]
    fn generated_worlds_are_consistent_and_deterministic(seed in any::<u32>(), w in 8_u32..40, h in 8_u32..30) {
        let (ts, tables) = kit();
        let dims = Dims::new(w, h).unwrap();
        let (a, passes) = World::generate(&ts, &tables, dims, Border::Sea, Seed(seed)).unwrap();
        prop_assert!(check(&ts, dims, Border::Sea, a.tiles()).is_ok());
        let (b, _) = World::generate(&ts, &tables, dims, Border::Sea, Seed(seed)).unwrap();
        prop_assert_eq!(a.tiles(), b.tiles());
        prop_assert!(!passes.is_empty());
    }

    /// §4.2 Bare geology is consistent by construction, and relaxed: every
    /// tile's corners are smooth.
    #[test]
    fn bare_geology_is_consistent(seed in any::<u32>(), w in 1_u32..50, h in 1_u32..40, open in any::<bool>()) {
        let (ts, _) = kit();
        let dims = Dims::new(w, h).unwrap();
        let border = if open { Border::Open } else { Border::Sea };
        let bare = World::bare(&ts, dims, border, Seed(seed));
        prop_assert!(check(&ts, dims, border, bare.tiles()).is_ok());
        let v = world::geology(dims, border, Seed(seed));
        prop_assert_eq!(v.len(), ((w + 1) * (h + 1)) as usize);
    }

    /// §4.3 Relaxation only lowers, and leaves every vertex within one level
    /// of all eight neighbours.
    #[test]
    fn relax_is_lipschitz_and_only_lowers(raw in prop::collection::vec(0_u8..4, 1..200), w in 1_usize..20) {
        let rows = raw.len().div_ceil(w);
        let mut h = raw;
        h.resize(rows * w, 0);
        let before = h.clone();
        world::relax(&mut h, w, rows);
        for i in 0..h.len() {
            prop_assert!(h[i] <= before[i]);
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            for dy in -1..=1_i64 {
                for dx in -1..=1_i64 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx >= 0 && ny >= 0 && (nx as usize) < w && (ny as usize) < rows {
                        let n = h[ny as usize * w + nx as usize];
                        prop_assert!(h[i].abs_diff(n) <= 1);
                    }
                }
            }
        }
    }

    /// §4.4 A paint either lands — the world stays consistent, every stroke
    /// cell answers to the brush, and nothing outside the region changed —
    /// or fails and changes nothing at all.
    #[test]
    fn paint_lands_cleanly_or_not_at_all(
        seed in any::<u32>(),
        strokes in prop::collection::vec((0_i64..32, 0_i64..24, brush(), 1_usize..4), 1..5),
    ) {
        let (ts, tables) = kit();
        let dims = Dims::new(32, 24).unwrap();
        let (mut w, _) = World::generate(&ts, &tables, dims, Border::Sea, Seed(seed)).unwrap();
        for (k, (x, y, b, len)) in strokes.into_iter().enumerate() {
            let stroke: Vec<(i64, i64)> = (0..len as i64).map(|d| ((x + d).min(31), y)).collect();
            let before = w.clone();
            if let Ok(r) = w.paint(&ts, &tables, &stroke, b, k as u32) {
                {
                    prop_assert!(check(&ts, dims, Border::Sea, w.tiles()).is_ok());
                    let mut inside = vec![false; dims.cells()];
                    for c in &r.pass.region { inside[c.index()] = true; }
                    for ((now, was), &free) in w.tiles().iter().zip(before.tiles()).zip(&inside) {
                        if !free {
                            prop_assert_eq!(now, was);
                        }
                    }
                    for &(sx, sy) in &stroke {
                        let c = dims.at(sx, sy).unwrap();
                        prop_assert_eq!(w.pins()[c.index()], b);
                        if let Some(b) = b {
                            prop_assert!(b.accepts(ts.tile(w.tiles()[c.index()])));
                        }
                    }
                    // The pass's tiles are the region's tiles, and its
                    // trace replays to them.
                    for (c, t) in r.pass.region.iter().zip(&r.pass.tiles) {
                        prop_assert_eq!(w.tiles()[c.index()], *t);
                    }
                    for (c, t) in wave::replay(&r.pass.trace) {
                        prop_assert!(inside[c.index()]);
                        prop_assert_eq!(w.tiles()[c.index()], t);
                    }
                }
            } else {
                prop_assert_eq!(w.tiles(), before.tiles());
                prop_assert_eq!(w.pins(), before.pins());
            }
        }
    }

    /// §4.5 Drift never breaks the map, never touches terrain, and never
    /// overrides a pin.
    #[test]
    fn drift_keeps_terrain_and_pins(seed in any::<u32>(), salts in prop::collection::vec(any::<u32>(), 1..12)) {
        let (ts, tables) = kit();
        let dims = Dims::new(28, 20).unwrap();
        let (mut w, _) = World::generate(&ts, &tables, dims, Border::Sea, Seed(seed)).unwrap();
        let _ = w.paint(&ts, &tables, &[(14, 10)], Some(Brush::Town), 1);
        let corners = w.corners(&ts);
        for s in salts {
            let p = w.drift(&ts, &tables, s).unwrap();
            prop_assert!(check(&ts, dims, Border::Sea, w.tiles()).is_ok());
            prop_assert_eq!(&w.corners(&ts), &corners);
            if !p.ok { prop_assert_eq!(p.changed, 0); }
            for (i, pin) in w.pins().iter().enumerate() {
                if let Some(b) = pin {
                    prop_assert!(b.accepts(ts.tile(w.tiles()[i])));
                }
            }
        }
    }

    /// §3.3 Every pass's trace replays to exactly the pass's tiles.
    #[test]
    fn generation_traces_replay(seed in any::<u32>()) {
        let (ts, tables) = kit();
        let dims = Dims::new(24, 18).unwrap();
        let mut w = World::bare(&ts, dims, Border::Sea, Seed(seed));
        for (k, region) in w.blocks(Seed(seed)).into_iter().enumerate() {
            let p = w.reshape(&ts, &tables, &region, Seed(seed).salted(k as u32), world::Mode::Locked).unwrap();
            if !p.ok { continue; }
            let live = wave::replay(&p.trace);
            // Every region cell not already a single tile at the start is placed.
            for (c, t) in &live {
                prop_assert_eq!(w.tiles()[c.index()], *t);
            }
            let in_region = p.trace.iter().all(|e| match e {
                Event::Place { cell, .. } => region.contains(cell),
                Event::Rewind { .. } => true,
            });
            prop_assert!(in_region);
        }
    }
}

/// §1 The local promises that make the map readable: in any consistent map,
/// a road or river only stops at a town, a spring or a mouth.
#[test]
fn features_never_stop_in_a_field() {
    let (ts, tables) = kit();
    let dims = Dims::new(48, 30).unwrap();
    for word in ["avalon", "lyonesse", "hy-brasil", "thule", "ys"] {
        let (w, _) =
            World::generate(&ts, &tables, dims, Border::Sea, Seed::from_word(word)).unwrap();
        let mut features = 0;
        for (i, &t) in w.tiles().iter().enumerate() {
            let tile = ts.tile(t);
            let c = Cell(u32::try_from(i).unwrap());
            for d in Dir::ALL {
                let f = tile.edges[d.index()];
                if f == Feature::None {
                    continue;
                }
                features += 1;
                // The edge continues into a neighbour that carries it on.
                let n = dims.step(c, d).expect("features never touch the map edge");
                assert_eq!(ts.tile(w.tiles()[n.index()]).edges[d.opposite().index()], f);
            }
            if tile.roads() == 1 {
                assert_eq!(tile.center, Center::Town);
            }
            if tile.rivers() == 1 {
                assert!(matches!(tile.center, Center::Spring | Center::Mouth));
            }
        }
        assert!(features > 0, "{word} has no rivers or roads at all");
    }
}

/// A town pinned on the shore of a sea-bordered map cannot exist anywhere.
#[test]
fn an_impossible_pin_is_refused_and_changes_nothing() {
    let (ts, tables) = kit();
    let dims = Dims::new(20, 14).unwrap();
    let (mut w, _) = World::generate(&ts, &tables, dims, Border::Sea, Seed(77)).unwrap();
    let before = w.clone();
    assert!(
        w.paint(&ts, &tables, &[(0, 0)], Some(Brush::Town), 0)
            .is_err()
    );
    assert_eq!(w.tiles(), before.tiles());
    assert_eq!(w.pins(), before.pins());
    assert!(
        w.paint(&ts, &tables, &[(99, 0)], Some(Brush::Sea), 0)
            .is_err()
    );
    assert!(w.paint(&ts, &tables, &[], Some(Brush::Sea), 0).is_err());
}

/// Words are seeds: an island is a function of its name.
#[test]
fn words_name_islands() {
    let (ts, tables) = kit();
    let dims = Dims::new(30, 20).unwrap();
    let a = World::generate(&ts, &tables, dims, Border::Sea, Seed::from_word("Avalon"))
        .unwrap()
        .0;
    let b = World::generate(&ts, &tables, dims, Border::Sea, Seed::from_word(" avalon"))
        .unwrap()
        .0;
    let c = World::generate(&ts, &tables, dims, Border::Sea, Seed::from_word("lyonesse"))
        .unwrap()
        .0;
    assert_eq!(a.tiles(), b.tiles());
    assert_ne!(a.tiles(), c.tiles());
}

/// The land rises to meet a mountain dropped in open sea.
#[test]
fn a_mountain_in_the_sea_raises_land() {
    let (ts, tables) = kit();
    let dims = Dims::new(40, 30).unwrap();
    let mut w = World::bare(&ts, dims, Border::Sea, Seed(1));
    let here = dims.at(5, 5).unwrap().index();
    assert!(
        Brush::Sea.accepts(ts.tile(w.tiles()[here])),
        "the test wants open sea at (5, 5)"
    );
    let r = w
        .paint(&ts, &tables, &[(5, 5)], Some(Brush::Mountain), 3)
        .unwrap();
    assert!(r.pass.changed > 0);
    let t = ts.tile(w.tiles()[here]);
    assert!(Brush::Mountain.accepts(t));
    check(&ts, dims, Border::Sea, w.tiles()).unwrap();
}

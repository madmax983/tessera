//! Dump generated, painted and drifted islands as JSON, so the browser port
//! (crates/tessera-app/src/engine.js) can be checked against this build.
//!
//!   cargo run -p tessera --example dump -- avalon lyonesse

use tessera::rng::Seed;
use tessera::tiles::{Brush, Tileset};
use tessera::wave::{Border, Dims, Tables};
use tessera::world::World;

/// One stroke: the cells, and the brush (`None` erases).
type Stroke = (&'static [(i64, i64)], Option<Brush>);

/// The strokes every word is painted with.
const STROKES: [Stroke; 5] = [
    (&[(6, 6)], Some(Brush::Mountain)),
    (&[(24, 15), (25, 15), (26, 15)], Some(Brush::River)),
    (&[(30, 20)], Some(Brush::Town)),
    (&[(20, 10)], Some(Brush::Sea)),
    (&[(12, 22)], None),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ts = Tileset::island();
    let tables = Tables::new(&ts);
    let dims = Dims::new(48, 30)?;
    let ids = |w: &World| w.tiles().iter().map(|t| t.0).collect::<Vec<_>>();
    let mut out = Vec::new();
    for word in std::env::args().skip(1) {
        let (mut world, passes) =
            World::generate(&ts, &tables, dims, Border::Sea, Seed::from_word(&word))?;
        tessera::check::check(&ts, dims, Border::Sea, world.tiles())?;
        let generated = ids(&world);
        let trace: Vec<i64> = passes
            .iter()
            .flat_map(|p| p.trace.iter().map(|e| e.encode()))
            .collect();
        let mut paints = Vec::new();
        for (k, (stroke, brush)) in STROKES.iter().enumerate() {
            let salt = u32::try_from(k)?;
            let r = world.paint(&ts, &tables, stroke, *brush, salt);
            paints.push(serde_json::json!({
                "ok": r.is_ok(),
                "radius": r.as_ref().map_or(0, |r| r.radius),
                "changed": r.as_ref().map_or(0, |r| r.pass.changed),
                "trace": r.as_ref().map_or(0, |r| r.pass.trace.len()),
            }));
        }
        let painted = ids(&world);
        for salt in 0..30 {
            world.drift(&ts, &tables, salt)?;
        }
        out.push(serde_json::json!({
            "word": word,
            "seed": Seed::from_word(&word).0,
            "generated": generated,
            "trace": trace,
            "paints": paints,
            "painted": painted,
            "drifted": ids(&world),
        }));
    }
    println!(
        "{}",
        serde_json::json!({ "tiles": ts.len(), "islands": out })
    );
    Ok(())
}

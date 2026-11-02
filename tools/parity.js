// Check the browser engine against the Rust build, tile for tile.
//   cargo run -q --release -p tessera --example dump -- <words> > /tmp/rust.json
//   node tools/parity.js /tmp/rust.json
const fs = require("fs");
const E = require("../crates/tessera-app/src/engine.js");
const rust = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
const STROKES = [
  [[[6, 6]], "mountain"],
  [[[24, 15], [25, 15], [26, 15]], "river"],
  [[[30, 20]], "town"],
  [[[20, 10]], "sea"],
  [[[12, 22]], null],
];
const kit = E.kit();
let fail = 0, cells = 0, events = 0;
const same = (a, b) => a.length === b.length && a.every((v, i) => v === b[i]);
if (kit.tiles.length !== rust.tiles) { console.log(`tileset: js ${kit.tiles.length}, rust ${rust.tiles}`); fail++; }
for (const isl of rust.islands) {
  const seed = E.fromWord(isl.word);
  if (seed !== isl.seed) { console.log(`${isl.word}: seed js ${seed} rust ${isl.seed}`); fail++; continue; }
  const dims = new E.Dims(48, 30);
  const { world, passes } = E.World.generate(kit, dims, "sea", seed);
  const trace = passes.flatMap((p) => p.trace.map(E.encode));
  if (!same(world.tiles, isl.generated)) { console.log(`${isl.word}: generated differs`); fail++; }
  if (!same(trace, isl.trace)) { console.log(`${isl.word}: trace differs (${trace.length} vs ${isl.trace.length})`); fail++; }
  STROKES.forEach(([s, b], k) => {
    let r = null;
    try { r = world.paint(s, b, k); } catch (e) { r = null; }
    const want = isl.paints[k];
    const got = { ok: !!r, radius: r ? r.radius : 0, changed: r ? r.pass.changed : 0, trace: r ? r.pass.trace.length : 0 };
    if (["ok", "radius", "changed", "trace"].some((f) => got[f] !== want[f])) { console.log(`${isl.word}: paint ${k} js ${JSON.stringify(got)} rust ${JSON.stringify(want)}`); fail++; }
  });
  if (!same(world.tiles, isl.painted)) { console.log(`${isl.word}: painted differs`); fail++; }
  for (let s = 0; s < 30; s++) world.drift(s);
  if (!same(world.tiles, isl.drifted)) { console.log(`${isl.word}: drifted differs`); fail++; }
  cells += 3 * dims.cells(); events += trace.length;
}
console.log(`${rust.islands.length} islands, ${cells} tiles and ${events} trace events compared: ${fail === 0 ? "identical" : fail + " differences"}`);
process.exit(fail === 0 ? 0 : 1);

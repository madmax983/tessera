# CLAUDE.md — Tessera

A shared island drawn by a wave-function-collapse solver. Read `PITCH.md`,
then `docs/SPEC.md` (the rules and the proof scoreboard), then
`docs/adr/0001-the-commons-lives-in-memory.md`.

* Engine: `crates/tessera` (pure). App: `crates/tessera-app` (autumn-web 0.9).
* **The JS port must stay bit-exact.** Any change to tile enumeration,
  weights, RNG, the solver's order of choice, geology or world passes must be
  made in `engine.js` too; check with
  `cargo run -q --release -p tessera --example dump -- avalon ys > /tmp/r.json && node tools/parity.js /tmp/r.json`.
* A world changes only by whole passes (`World::reshape*`). Never add a
  set-one-tile API.
* Gates: `./verify.sh` (authoritative, needs Verus). Cloud pre-check:
  fmt, `clippy --all-targets -D warnings` (pedantic + nursery), tests.
* No `unwrap`/`expect` outside tests.

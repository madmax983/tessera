# Tessera — the rules

Everything here is a function of a seed. No clock, no IO in the engine
(`crates/tessera`). The browser port (`crates/tessera-app/src/engine.js`)
follows the same rules bit for bit; `tools/parity.js` checks it.

## 1. Tiles

A tile is four corner **levels** — 0 sea, 1 grass, 2 forest, 3 mountain,
in the order NW, NE, SE, SW — and four edge **features** — none, road,
river, in the order N, E, S, W.

* **Smooth:** every pair of corners (edge-adjacent *and* diagonal) differs by
  at most one level.
* **Features only cross land:** a road or river edge has both of its corners
  ≥ grass; a road edge has neither corner on a mountain.
* **Kinds** (`tiles::classify`):
  plain (no features, any smooth pattern) · road piece (2+ roads, all grass,
  or exactly 2 roads through grass/forest) · **town** (1+ roads, all grass —
  the only tile with a single road) · river piece (exactly 2 river edges,
  all land) · **spring** (1 river edge, a mountain corner) · **mouth**
  (1 river edge whose corners are land, the opposite edge all sea) ·
  bridge (a straight road crossing a straight river, lowland).
* **Socket** of an edge = (level at its first end, level at its second end,
  feature), ends read west→east or north→south. Encoded `a·12 + b·3 + f`,
  48 sockets.
* **Fit:** `a` may sit on side `d` of `b`'s neighbour iff
  `socket(a, d) == socket(b, opp(d))`. This is the only rule.

Enumeration order is fixed (corners NW..SW outermost-first over 0..3, then
edge combinations `fe = 0..81` with N the most significant base-3 digit; a
town follows its road slot). 444 tiles.

**Observable consequence (tested, `tests/laws.rs::features_never_stop_in_a_field`):**
in any consistent map every road dead end is a town and every river end is a
spring or a mouth. Loops of river are *not* excluded — no local rule can.

## 2. Problems and solutions

A problem gives every cell a domain (a set of tiles). With `Border::Sea`
every edge cell must show the all-sea socket outward. A **solution** picks one
tile per cell from its domain so that every pair of neighbours fits.

## 3. The solver (`wave.rs`)

* **Revise** `i → j` across `d`: `D[j] := { b ∈ D[j] | ∃ a ∈ D[i]. fits(a, d, b) }`,
  computed as "tiles of `D[j]` whose `opp(d)` socket is one `D[i]` shows on `d`".
* **Propagate:** a FIFO of changed cells, revising every neighbour, to the
  fixed point. Contradiction if a domain empties.
* **Choose:** the cell with the fewest tiles left (> 1), ties by a per-cell
  random priority drawn once per attempt, then by index.
* **Pick:** weighted by `weight(t) × AFFINITY[k]`, `k` = how many of the tile's
  corners equal the guide's corners for that cell (the current terrain).
  `AFFINITY = [1, 2, 6, 24, 96]`. The guide never removes a tile.
* **Backtrack:** on contradiction, undo to the last decision and *ban* the
  chosen tile from that cell, then propagate. Budget: 600 backtracks per
  attempt, then restart with a salted seed, up to 4 restarts.
* **Trace:** `Place(cell, tile)` whenever a domain becomes a singleton (incl.
  cells that start single), `Rewind(to)` = keep the first `to` placements.
  Replaying a trace yields exactly the solution's tiles on every placed cell.

## 4. Worlds (`world.rs`)

1. **Geology:** two octaves of integer value noise at the `(w+1)·(h+1)`
   vertices, minus an island falloff, thresholded to 0..3, edge vertices 0
   for a sea border; then **relaxed**: lower any vertex above a neighbour + 1
   (8-neighbourhood) until none is.
2. **Bare world:** each cell is the plain tile whose corners are its four
   vertices. Consistent by construction.
3. **Reshape(region, mode):** fix every cell outside the region to its tile;
   inside, `Locked` keeps each cell's corners (only features change) and
   `Free` allows anything; pins restrict to their brush. Solve. Commit only on
   success; otherwise the world is untouched.
4. **Generate:** bare, then `Locked` reshapes of 12×12 blocks at stride 6 in a
   Fisher–Yates order.
5. **Paint:** set pins on the stroke; refuse at once if arc consistency on the
   pins alone fails; then `Free` reshapes of discs of radius 3, 4, 6, 8, 11
   around the stroke until one solves. Error ⇒ world and pins unchanged.
6. **Drift:** a `Locked` reshape of a disc of radius 2–4 at a seeded cell.

## 5. What must be true

### Formally (Verus, `verus/src/tessera.rs`)

| # | Obligation | Lemma | Confidence |
|---|---|---|---|
| 1 | revise never removes a tile that is in some solution | `lemma_revise_sound` | high |
| 2 | the socket shortcut computes exactly revise | `lemma_socket_shortcut` | medium — `choose` over a `Set::filter` / `Set::map` with an existential; may need trigger help |
| 3 | a changing revise strictly shrinks a finite domain, and the total size strictly falls | `lemma_strict_subset_shrinks`, `lemma_total_update`, `lemma_revise_terminates` | medium — the `drop_last`/`update` extensionality steps are the usual place Verus wants one more assert |
| 4 | banning a tile no solution uses loses no solution | `lemma_ban_sound` | high |
| 5 | a solution of a reshape is consistent and equals the map outside the region | `lemma_splice` | high |
| 6 | a settled field is 1-Lipschitz (symmetric neighbours); four pairwise-close corners are smooth; lowering keeps heights ≥ 0 and strictly drops the sum | `lemma_settled_is_lipschitz`, `lemma_corners_smooth`, `lemma_lower_decreases`, `lemma_sum_nonneg`, `lemma_lower_keeps_nonneg` | high / high / medium / medium / high |
| 7 | the executable smoothness test equals the spec; the lowering rule never raises and never overflows | `smooth_exec`, `lower_exec` | low (array indexing in `ensures` and a closure with `ensures`) / high |

These were written without a Verus binary to hand, so the confidence column
is the prediction made then. All of them now verify under the Verus
pinned in `verus/verus-version` (`./verify.sh` refuses any other), where `Set` holds only finite sets: domains are
built with `filter` and `map`, and finiteness comes from the type rather than
from a `requires`. The table is the scoreboard.

### Observably (tests)

* Every generated world passes the independent checker; same seed ⇒ same
  world; words fold case and whitespace. (`laws.rs`, proptest)
* Bare geology is consistent for any size and border; relaxation only lowers
  and leaves every vertex within one of its 8 neighbours.
* A paint either lands — consistent, stroke cells satisfy the brush, nothing
  outside the region changed, the trace replays to the region's tiles — or
  fails and changes nothing, pins included.
* Drift never changes a corner and never overrides a pin.
* Propagation keeps every tile of a known solution.
* A town pinned on the shore is a contradiction, refused before any search.
* The browser engine equals the Rust engine on generated, painted and drifted
  worlds and on every trace event (`tools/parity.js`).
* HTTP: pages serve, paints advance the sequence and pin, malformed paints are
  400, impossible ones 422, growing replaces the world, the weather toggles.

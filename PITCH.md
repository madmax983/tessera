# Tessera — a map that redraws itself to keep its promises

**Engine:** search / solve. A wave-function-collapse constraint solver with
arc-consistency propagation and chronological backtracking.
**Subject:** an invented archipelago of seas, fields, forests, mountains,
rivers, roads and walled towns — drawn like an old atlas plate.

## What it is

An island is a grid of square tiles. Every tile carries a terrain height at
each of its four corners (sea, grass, forest, mountain) and, on each edge,
either nothing, a road, or a river. Two tiles may sit side by side only when
the edge they share agrees completely: the same two corner heights, the same
feature. That one rule, applied everywhere at once, is enough to make the
world *keep its promises*: a coast never tears, a river never stops in the
middle of a field (it rises at a spring in the mountains and leaves through a
mouth in the sea), and a road never peters out — every road ends at a town.

Nothing draws a river. Nothing routes a road. The solver only ever refuses
neighbours that disagree, and the rivers fall out.

## The moment

The front page is **the Commons**: one shared island, live in every open
browser. Pick a brush — *Mountain*, *Town*, *River*, *Sea* — and drop it
anywhere. The solver tears up a small disc of the map around your stroke and
re-solves it with everything outside the disc held fixed, growing the disc
until the world can hold what you asked for.

Drop a mountain in the open sea and watch the land rise out of the water to
meet it — shallows, then a beach, then foothills. Paint a river across a
meadow and the map *has* to find it a spring and a sea, so a mountain range
shoulders up out of the grass and a bay cuts in to catch it. Drop a town in a
forest and a road appears out of nowhere to reach it. Everyone else sees the
same tear-and-mend play out tile by tile, in your colour, with the solver's
backtracks flashing red where it changed its mind.

## Why it is delightful

- It surprises: you ask for one tile and get a landscape argument.
- It is alive: every generation and every repair is animated tile by tile —
  the "wave" collapsing, the dead ends, the rewinds.
- The core is an engine: a real constraint solver, not a rule check.
- It photographs well: hillshaded terrain, hand-drawn trees, peaks, rivers,
  roads, little red-roofed towns.
- You can get lost in it: words are seeds; every word is an island.

## Where it could go

- An infinite atlas: chunks solved on demand against their neighbours'
  borders, so you can walk forever (the repair-in-blocks machinery here is
  already the hard half of that).
- Tilesets as data: hex grids, a city block grammar, a dungeon set.
- A tile-weight sequencer: sliders that bend the world (more forest, wetter).
- Export a plate as a printable SVG atlas page.

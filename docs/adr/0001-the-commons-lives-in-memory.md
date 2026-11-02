# ADR 0001 — The Commons lives in memory; a world only changes by whole passes

## Status

Accepted, 2026-11-02.

## Context

Tessera has one shared world that every browser paints on and watches. It
could be persisted (SQLite rows of tiles and pins) or kept in the process.

The world is small (64 × 40 tiles, a few kilobytes) and *derivable*: a word
regrows it, and the interesting state is the last few minutes of strokes. The
interesting engineering is elsewhere — in making sure the map is never torn.

## Decision

1. The Commons is an `AppState` extension holding one `World` behind a mutex.
   No database, no migrations, no Docker. A restart regrows it from
   `TESSERA_WORD`.
2. **A world only ever changes by a whole pass.** A pass re-solves a region
   with everything outside fixed and is assigned to the world only after the
   solve succeeds. There is no API that sets a single tile. This is what makes
   "consistent between any two strokes" true rather than hoped for, and it is
   the property `lemma_splice` states.
3. Every pass gets the next sequence number and is published whole — region,
   trace and final tiles — on one live-events topic. Clients replay the trace
   and refetch the world on any gap. No partial updates on the wire either.
4. The browser carries a port of the engine, used for the atlas (any word,
   grown locally) and for the published artifact. The server never trusts it:
   paints are re-solved on the server.

## Consequences

* A restart loses the strokes. Acceptable for a toy; persisting would be one
  table of `(seq, stroke, brush)` replayed at boot — the world is a pure
  function of its seed and its stroke log, so replay is exact.
* All writes serialise on one lock. A paint is ~5–10 ms; fine for a room of
  people, not for a stadium.
* The large `world` message (a regrow: ~60 traces) can lag a slow client; it
  then sees a sequence gap and refetches, which is correct if less pretty.

// Tessera engine, in the browser.
//
// A line-for-line port of crates/tessera (rng.rs, tiles.rs, wave.rs,
// world.rs). Given the same seed it grows the same island, tile for tile —
// verify.sh checks that against the Rust build. Domains are bitsets of
// 32-bit words here and 64-bit words there; every loop walks tiles in
// ascending id order on both sides, so the word size never shows.
(function (root) {
  "use strict";

  // ── rng.rs ────────────────────────────────────────────────────────────────
  function salted(seed, salt) {
    return (seed ^ Math.imul(salt >>> 0, 0x9e3779b9)) >>> 0;
  }
  function fromWord(word) {
    let h = 0x811c9dc5;
    const bytes = new TextEncoder().encode(String(word).trim().toLowerCase());
    for (const b of bytes) {
      h = (h ^ b) >>> 0;
      h = Math.imul(h, 0x01000193) >>> 0;
    }
    return h >>> 0;
  }
  class Rng {
    constructor(seed) { this.s = seed >>> 0; }
    next() {
      this.s = (this.s + 0x6d2b79f5) >>> 0;
      let t = this.s;
      t = Math.imul(t ^ (t >>> 15), t | 1) >>> 0;
      t = (t ^ ((t + Math.imul(t ^ (t >>> 7), t | 61)) >>> 0)) >>> 0;
      return (t ^ (t >>> 14)) >>> 0;
    }
    below(n) { return n === 0 ? 0 : this.next() % n; }
  }

  // ── tiles.rs ──────────────────────────────────────────────────────────────
  const SEA = 0, GRASS = 1, FOREST = 2, MOUNTAIN = 3;
  const NONE = 0, ROAD = 1, RIVER = 2;
  const PLAIN = "plain", TOWN = "town", SPRING = "spring", MOUTH = "mouth", BRIDGE = "bridge";
  const SOCKET_SEA = 0;
  const close = (a, b) => Math.abs(a - b) <= 1;
  const smooth = (c) => close(c[0], c[1]) && close(c[1], c[2]) && close(c[2], c[3]) &&
    close(c[3], c[0]) && close(c[0], c[2]) && close(c[1], c[3]);
  const EDGE_CORNERS = [[0, 1], [1, 2], [3, 2], [0, 3]];

  function plainWeight(c) {
    const lo = Math.min(...c), hi = Math.max(...c);
    if (lo === hi) return [240, 640, 560, 440][lo];
    return lo === SEA ? 120 : 160;
  }

  function classify(c, e) {
    const roads = e.filter((f) => f === ROAD).length;
    const rivers = e.filter((f) => f === RIVER).length;
    const all = (p) => c.every(p);
    const lowland = (l) => l === GRASS || l === FOREST;
    const land = (l) => l !== SEA;
    const straight = (e[0] === e[2] && e[0] !== NONE) || (e[1] === e[3] && e[1] !== NONE);
    if (roads === 0 && rivers === 0) return [PLAIN, plainWeight(c)];
    if (rivers === 0) {
      if (all((l) => l === GRASS)) {
        if (roads === 1) return null;
        return roads === 2 ? [PLAIN, straight ? 10 : 5] : [PLAIN, 2];
      }
      if (all(lowland) && roads === 2) return [PLAIN, straight ? 4 : 2];
      return null;
    }
    if (roads === 0) {
      if (!all(land)) {
        if (rivers !== 1) return null;
        const d = e.indexOf(RIVER);
        const [a, b] = EDGE_CORNERS[d];
        const [oa, ob] = EDGE_CORNERS[(d + 2) % 4];
        if (land(c[a]) && land(c[b]) && c[oa] === SEA && c[ob] === SEA) return [MOUTH, 4];
        return null;
      }
      if (rivers === 1 && c.includes(MOUNTAIN)) return [SPRING, 4];
      if (rivers === 2) return [PLAIN, straight ? 2 : 1];
      return null;
    }
    if (roads === 2 && rivers === 2 && e[0] === e[2] && e[1] === e[3] && all(lowland)) return [BRIDGE, 2];
    return null;
  }

  function socketOf(t, d) {
    const [nw, ne, se, sw] = t.corners;
    const ab = [[nw, ne], [ne, se], [sw, se], [nw, sw]][d];
    return ab[0] * 12 + ab[1] * 3 + t.edges[d];
  }

  function enumerate() {
    const tiles = [];
    for (let nw = 0; nw < 4; nw++) for (let ne = 0; ne < 4; ne++)
      for (let se = 0; se < 4; se++) for (let sw = 0; sw < 4; sw++) {
        const c = [nw, ne, se, sw];
        if (!smooth(c)) continue;
        for (let fe = 0; fe < 81; fe++) {
          const pick = (k) => Math.floor(fe / Math.pow(3, k)) % 3;
          const e = [pick(3), pick(2), pick(1), pick(0)];
          const k = classify(c, e);
          if (k) tiles.push({ corners: c, edges: e, center: k[0], weight: k[1] });
          const roads = e.filter((f) => f === ROAD).length;
          const rivers = e.filter((f) => f === RIVER).length;
          if (rivers === 0 && roads >= 1 && c.every((l) => l === GRASS)) {
            tiles.push({ corners: c, edges: e, center: TOWN, weight: roads === 1 ? 5 : 3 });
          }
        }
      }
    for (const t of tiles) {
      t.roads = t.edges.filter((f) => f === ROAD).length;
      t.rivers = t.edges.filter((f) => f === RIVER).length;
      t.sockets = [0, 1, 2, 3].map((d) => socketOf(t, d));
    }
    return tiles;
  }

  const BRUSHES = ["sea", "grass", "forest", "mountain", "town", "road", "river"];
  function accepts(brush, t) {
    const plain = t.roads === 0 && t.rivers === 0;
    const uni = (l) => t.corners.every((c) => c === l);
    switch (brush) {
      case "sea": return plain && uni(SEA);
      case "grass": return plain && uni(GRASS);
      case "forest": return plain && uni(FOREST);
      case "mountain": return plain && uni(MOUNTAIN);
      case "town": return t.center === TOWN;
      case "road": return t.roads > 0 && t.center !== TOWN;
      case "river": return t.rivers > 0;
      default: return false;
    }
  }

  // ── wave.rs ───────────────────────────────────────────────────────────────
  const AFFINITY = [1, 2, 6, 24, 96];
  const OPP = [2, 3, 0, 1];
  const popcount = (x) => { x -= (x >>> 1) & 0x55555555; x = (x & 0x33333333) + ((x >>> 2) & 0x33333333); return (((x + (x >>> 4)) & 0x0f0f0f0f) * 0x01010101) >>> 24; };
  const ctz = (x) => 31 - Math.clz32(x & -x);

  class Tables {
    constructor(tiles) {
      this.tiles = tiles;
      this.words = Math.ceil(tiles.length / 32);
      this.with = [0, 1, 2, 3].map(() => new Uint32Array(48 * this.words));
      tiles.forEach((t, i) => {
        for (let d = 0; d < 4; d++) this.with[d][t.sockets[d] * this.words + (i >>> 5)] |= 1 << (i & 31);
      });
    }
  }

  class Dims {
    constructor(w, h) { this.w = w; this.h = h; }
    cells() { return this.w * this.h; }
    at(x, y) { return x < 0 || y < 0 || x >= this.w || y >= this.h ? -1 : y * this.w + x; }
    xy(c) { return [c % this.w, Math.floor(c / this.w)]; }
    step(c, d) {
      const [x, y] = this.xy(c);
      return d === 0 ? this.at(x, y - 1) : d === 1 ? this.at(x + 1, y) : d === 2 ? this.at(x, y + 1) : this.at(x - 1, y);
    }
  }

  class Problem {
    constructor(tables, dims, border) {
      this.dims = dims; this.words = tables.words; this.ntiles = tables.tiles.length;
      const n = dims.cells(), W = this.words;
      this.dom = new Uint32Array(n * W);
      for (let c = 0; c < n; c++) for (let t = 0; t < this.ntiles; t++) this.dom[c * W + (t >>> 5)] |= 1 << (t & 31);
      this.guide = null;
      if (border === "sea") {
        for (let c = 0; c < n; c++) for (let d = 0; d < 4; d++) {
          if (dims.step(c, d) < 0) {
            const ok = [];
            tables.tiles.forEach((t, i) => { if (t.sockets[d] === SOCKET_SEA) ok.push(i); });
            this.restrict(c, ok);
          }
        }
      }
    }
    restrict(c, ids) {
      const W = this.words, mask = new Uint32Array(W);
      for (const t of ids) if (t < this.ntiles) mask[t >>> 5] |= 1 << (t & 31);
      for (let w = 0; w < W; w++) this.dom[c * W + w] &= mask[w];
    }
    fix(c, t) { this.restrict(c, [t]); }
    allows(c, t) { return (this.dom[c * this.words + (t >>> 5)] & (1 << (t & 31))) !== 0; }
  }

  class Contradiction extends Error {}

  function firstBit(dom, base, W) {
    for (let w = 0; w < W; w++) { const b = dom[base + w]; if (b !== 0) return w * 32 + ctz(b); }
    return -1;
  }

  class Search {
    constructor(tables, p) {
      this.tables = tables; this.dims = p.dims; this.W = tables.words;
      const n = p.dims.cells();
      this.dom = p.dom.slice();
      this.count = new Int32Array(n);
      for (let c = 0; c < n; c++) { let k = 0; for (let w = 0; w < this.W; w++) k += popcount(this.dom[c * this.W + w]); this.count[c] = k; }
      this.trailCells = []; this.trailSaved = [];
      this.queue = []; this.queued = new Uint8Array(n);
      this.trace = []; this.placed = 0;
      this.next = new Uint32Array(this.W); this.allowed = new Uint32Array(this.W);
    }
    set(c, nw) {
      const W = this.W, base = c * W;
      this.trailCells.push(c);
      for (let w = 0; w < W; w++) this.trailSaved.push(this.dom[base + w]);
      const before = this.count[c];
      let after = 0;
      for (let w = 0; w < W; w++) { this.dom[base + w] = nw[w]; after += popcount(nw[w]); }
      this.count[c] = after;
      if (after === 1 && before > 1) {
        const t = firstBit(this.dom, base, W);
        this.placed++;
        this.trace.push([c, t]);
      }
      return after > 0;
    }
    undoTo(len) {
      const W = this.W;
      while (this.trailCells.length > len) {
        const c = this.trailCells.pop();
        const at = this.trailSaved.length - W;
        let k = 0;
        for (let w = 0; w < W; w++) { this.dom[c * W + w] = this.trailSaved[at + w]; k += popcount(this.trailSaved[at + w]); }
        this.trailSaved.length = at;
        this.count[c] = k;
      }
    }
    enqueue(c) { if (!this.queued[c]) { this.queued[c] = 1; this.queue.push(c); } }
    propagate() {
      const W = this.W, tiles = this.tables.tiles, present = new Uint8Array(48);
      let head = 0, ok = true;
      while (head < this.queue.length) {
        const i = this.queue[head++];
        this.queued[i] = 0;
        if (!ok) continue;
        for (let d = 0; d < 4; d++) {
          const j = this.dims.step(i, d);
          if (j < 0) continue;
          present.fill(0);
          for (let w = 0; w < W; w++) {
            let b = this.dom[i * W + w];
            while (b !== 0) { const t = w * 32 + ctz(b); present[tiles[t].sockets[d]] = 1; b &= b - 1; }
          }
          this.allowed.fill(0);
          const back = this.tables.with[OPP[d]];
          for (let s = 0; s < 48; s++) if (present[s]) for (let w = 0; w < W; w++) this.allowed[w] |= back[s * W + w];
          let changed = false;
          for (let w = 0; w < W; w++) {
            this.next[w] = this.dom[j * W + w] & this.allowed[w];
            if (this.next[w] !== this.dom[j * W + w]) changed = true;
          }
          if (changed) {
            if (!this.set(j, this.next)) { ok = false; break; }
            this.enqueue(j);
          }
        }
      }
      this.queue.length = 0;
      return ok;
    }
    weight(t, g) {
      const base = this.tables.tiles[t].weight;
      if (!g) return base;
      const c = this.tables.tiles[t].corners;
      let k = 0; for (let i = 0; i < 4; i++) if (c[i] === g[i]) k++;
      return base * AFFINITY[k];
    }
    pick(c, g, rng) {
      const W = this.W, base = c * W;
      let total = 0;
      for (let w = 0; w < W; w++) { let b = this.dom[base + w]; while (b !== 0) { total += this.weight(w * 32 + ctz(b), g); b &= b - 1; } }
      let r = rng.below(total);
      for (let w = 0; w < W; w++) {
        let b = this.dom[base + w];
        while (b !== 0) {
          const t = w * 32 + ctz(b), wt = this.weight(t, g);
          if (r < wt) return t;
          r -= wt; b &= b - 1;
        }
      }
      return firstBit(this.dom, base, W);
    }
  }

  function choose(count, priority) {
    let best = -1, bk = 0, bp = 0;
    for (let c = 0; c < count.length; c++) {
      const k = count[c];
      if (k > 1 && (best < 0 || k < bk || (k === bk && priority[c] < bp))) { best = c; bk = k; bp = priority[c]; }
    }
    return best;
  }

  // Returns {tiles, trace, backtracks} or throws {kind}.
  function solveOnce(tables, p, seed, budget) {
    const n = p.dims.cells(), W = tables.words;
    const rng = new Rng(seed);
    const priority = new Uint32Array(n);
    for (let c = 0; c < n; c++) priority[c] = rng.next();
    const s = new Search(tables, p);
    for (let c = 0; c < n; c++) if (s.count[c] === 0) throw { kind: "contradiction" };
    for (let c = 0; c < n; c++) {
      if (s.count[c] === 1) { s.placed++; s.trace.push([c, firstBit(s.dom, c * W, W)]); }
      s.enqueue(c);
    }
    if (!s.propagate()) throw { kind: "contradiction" };
    const stack = [];
    let backtracks = 0, decisions = 0;
    for (;;) {
      const c = choose(s.count, priority);
      if (c < 0) break;
      decisions++;
      const g = p.guide ? p.guide[c] : null;
      const tile = s.pick(c, g, rng);
      stack.push({ cell: c, tile, trail: s.trailCells.length, placed: s.placed });
      const only = new Uint32Array(W); only[tile >>> 5] |= 1 << (tile & 31);
      let ok = s.set(c, only);
      if (ok) { s.enqueue(c); ok = s.propagate(); }
      while (!ok) {
        const d = stack.pop();
        if (!d) throw { kind: "exhausted" };
        backtracks++;
        if (backtracks > budget) throw { kind: "overbudget" };
        s.undoTo(d.trail);
        s.placed = d.placed;
        s.trace.push([-1, d.placed]);
        const banned = s.dom.slice(d.cell * W, (d.cell + 1) * W);
        banned[d.tile >>> 5] &= ~(1 << (d.tile & 31));
        ok = s.set(d.cell, banned);
        if (ok) { s.enqueue(d.cell); ok = s.propagate(); }
      }
    }
    const tiles = new Array(n);
    for (let c = 0; c < n; c++) tiles[c] = firstBit(s.dom, c * W, W);
    return { tiles, trace: s.trace, backtracks, decisions };
  }

  function solve(tables, p, seed, budget, restarts) {
    let last = { kind: "overbudget" };
    for (let k = 0; k <= restarts; k++) {
      try { return solveOnce(tables, p, salted(seed, k), budget); }
      catch (e) { if (e && e.kind === "overbudget") { last = e; continue; } throw e; }
    }
    throw last;
  }

  function arcConsistent(tables, p) {
    const s = new Search(tables, p);
    const n = p.dims.cells();
    for (let c = 0; c < n; c++) if (s.count[c] === 0) return false;
    for (let c = 0; c < n; c++) s.enqueue(c);
    return s.propagate();
  }

  // Trace events, the same wire form as wave::Event::encode.
  const encode = (e) => (e[0] < 0 ? -(e[1] + 1) : e[0] * 1024 + e[1]);
  const decode = (v) => (v < 0 ? [-1, -v - 1] : [Math.floor(v / 1024), v % 1024]);

  // ── world.rs ──────────────────────────────────────────────────────────────
  const BUDGET = 600, RESTARTS = 4, RADII = [3, 4, 6, 8, 11], BLOCK = 12, STRIDE = 6, NOISE = [11, 4];

  function geology(dims, border, seed) {
    const W = dims.w, H = dims.h;
    const rng = new Rng(salted(seed, 0x0006e010));
    const octaves = NOISE.map((s) => {
      const lw = Math.floor(W / s) + 2, lh = Math.floor(H / s) + 2;
      const v = new Array(lw * lh);
      for (let i = 0; i < lw * lh; i++) v[i] = rng.below(1000);
      return [s, lw, v];
    });
    const out = [];
    for (let y = 0; y <= H; y++) for (let x = 0; x <= W; x++) {
      let v = 0;
      octaves.forEach(([s, lw, lat], k) => {
        const i = Math.floor(x / s), j = Math.floor(y / s), fx = x % s, fy = y % s;
        const at = (a, b) => lat[b * lw + a];
        const top = at(i, j) * (s - fx) + at(i + 1, j) * fx;
        const bot = at(i, j + 1) * (s - fx) + at(i + 1, j + 1) * fx;
        const n = Math.floor((top * (s - fy) + bot * fy) / (s * s));
        v += k === 0 ? n * 2 : n;
      });
      const dx = Math.floor(Math.abs(2 * x - W) * 1000 / W);
      const dy = Math.floor(Math.abs(2 * y - H) * 1000 / H);
      const d = Math.floor((dx * dx + dy * dy) / 1000);
      const score = v - Math.floor(d * 3 / 2) + 550;
      const edge = x === 0 || y === 0 || x === W || y === H;
      out.push(border === "sea" && edge ? 0 : score < 1500 ? 0 : score < 2050 ? 1 : score < 2400 ? 2 : 3);
    }
    relax(out, W + 1, H + 1);
    return out;
  }

  function relax(h, w, rows) {
    for (;;) {
      let changed = false;
      for (let y = 0; y < rows; y++) for (let x = 0; x < w; x++) {
        let lo = h[y * w + x];
        for (let ny = Math.max(0, y - 1); ny < Math.min(rows, y + 2); ny++)
          for (let nx = Math.max(0, x - 1); nx < Math.min(w, x + 2); nx++) lo = Math.min(lo, h[ny * w + nx] + 1);
        if (lo < h[y * w + x]) { h[y * w + x] = lo; changed = true; }
      }
      if (!changed) break;
    }
  }

  function keepRegion(trace, inside) {
    const keptBefore = [0], out = [];
    for (const e of trace) {
      if (e[0] >= 0) {
        const keep = inside[e[0]] ? 1 : 0;
        keptBefore.push(keptBefore[keptBefore.length - 1] + keep);
        if (keep) out.push(e);
      } else {
        keptBefore.length = e[1] + 1;
        out.push([-1, keptBefore[keptBefore.length - 1]]);
      }
    }
    return out;
  }

  class World {
    constructor(kit, dims, border, seed, tiles) {
      this.kit = kit; this.dims = dims; this.border = border; this.seed = seed >>> 0;
      this.tiles = tiles; this.pins = new Array(dims.cells()).fill(null); this.passes = 0;
    }
    static bare(kit, dims, border, seed) {
      const v = geology(dims, border, seed), vw = dims.w + 1, tiles = [];
      for (let y = 0; y < dims.h; y++) for (let x = 0; x < dims.w; x++) {
        const c = [v[y * vw + x], v[y * vw + x + 1], v[(y + 1) * vw + x + 1], v[(y + 1) * vw + x]];
        tiles.push(kit.plain(c));
      }
      return new World(kit, dims, border, seed, tiles);
    }
    static generate(kit, dims, border, seed) {
      const w = World.bare(kit, dims, border, seed);
      const bare = w.tiles.slice();
      const passes = w.blocks(seed).map((region, k) => w.reshape(region, salted(seed, (0xb10c0000 ^ k) >>> 0), "locked"));
      return { world: w, bare, passes };
    }
    blocks(seed) {
      const origins = [];
      for (let y = 0; y < this.dims.h;) {
        for (let x = 0; x < this.dims.w;) {
          origins.push([x, y]);
          if (x + BLOCK >= this.dims.w) break;
          x += STRIDE;
        }
        if (y + BLOCK >= this.dims.h) break;
        y += STRIDE;
      }
      const rng = new Rng(salted(seed, 0x5b000000));
      for (let i = origins.length - 1; i >= 1; i--) {
        const j = rng.below(i + 1);
        const t = origins[i]; origins[i] = origins[j]; origins[j] = t;
      }
      return origins.map(([x0, y0]) => {
        const cells = [];
        for (let y = y0; y < Math.min(y0 + BLOCK, this.dims.h); y++)
          for (let x = x0; x < Math.min(x0 + BLOCK, this.dims.w); x++) cells.push(y * this.dims.w + x);
        return cells;
      });
    }
    corners() { return this.tiles.map((t) => this.kit.tiles[t].corners); }
    problem(pins, inside, mode) {
      const kit = this.kit, p = new Problem(kit.tables, this.dims, this.border);
      const corners = this.corners();
      p.guide = corners;
      for (let i = 0; i < this.tiles.length; i++) {
        if (!inside[i]) { p.fix(i, this.tiles[i]); continue; }
        if (mode === "locked") p.restrict(i, kit.byCorners(corners[i]));
        if (pins[i]) p.restrict(i, kit.brush(pins[i]));
      }
      return p;
    }
    reshapeWith(region, pins, seed, mode) {
      const inside = new Uint8Array(this.dims.cells());
      for (const c of region) inside[c] = 1;
      const p = this.problem(pins, inside, mode);
      const sol = solve(this.kit.tables, p, seed, BUDGET, RESTARTS);
      const tiles = region.map((c) => sol.tiles[c]);
      let changed = 0;
      for (const c of region) if (this.tiles[c] !== sol.tiles[c]) changed++;
      this.tiles = sol.tiles; this.passes++;
      return { region: region.slice(), trace: keepRegion(sol.trace, inside), tiles, changed, backtracks: sol.backtracks, ok: true };
    }
    reshape(region, seed, mode) {
      try { return this.reshapeWith(region, this.pins.slice(), seed, mode); }
      catch (e) {
        if (e && e.kind) return { region: region.slice(), trace: [], tiles: region.map((c) => this.tiles[c]), changed: 0, backtracks: 0, ok: false };
        throw e;
      }
    }
    paint(stroke, brush, salt) {
      if (stroke.length === 0 || stroke.length > 64) throw { kind: "stroke" };
      const cells = stroke.map(([x, y]) => { const c = this.dims.at(x, y); if (c < 0) throw { kind: "offmap" }; return c; });
      const pins = this.pins.slice();
      for (const c of cells) pins[c] = brush;
      const free = new Uint8Array(this.dims.cells()).fill(1);
      if (!arcConsistent(this.kit.tables, this.problem(pins, free, "free"))) throw { kind: "contradiction" };
      const seed = salted(this.seed, (0x9a170000 ^ salt) >>> 0);
      let last = { kind: "exhausted" };
      for (let grown = 0; grown < RADII.length; grown++) {
        const region = this.disc(cells, RADII[grown]);
        try {
          const pass = this.reshapeWith(region, pins, salted(seed, grown), "free");
          this.pins = pins;
          return { pass, radius: RADII[grown], grown };
        } catch (e) { if (e && e.kind) { last = e; continue; } throw e; }
      }
      throw last;
    }
    drift(salt) {
      const rng = new Rng(salted(this.seed, (0xd21f0000 ^ salt) >>> 0));
      const c = rng.below(this.dims.cells());
      const radius = 2 + rng.below(3);
      return this.reshape(this.disc([c], radius), salted(this.seed, salt), "locked");
    }
    disc(cells, r) {
      const centers = cells.map((c) => this.dims.xy(c)), out = [];
      for (let c = 0; c < this.dims.cells(); c++) {
        const [x, y] = this.dims.xy(c);
        if (centers.some(([cx, cy]) => (x - cx) * (x - cx) + (y - cy) * (y - cy) <= r * r)) out.push(c);
      }
      return out;
    }
  }

  function kit() {
    const tiles = enumerate();
    const tables = new Tables(tiles);
    const brushCache = {}, cornerCache = new Map();
    return {
      tiles, tables,
      brush(b) { return brushCache[b] || (brushCache[b] = tiles.map((t, i) => (accepts(b, t) ? i : -1)).filter((i) => i >= 0)); },
      byCorners(c) {
        const key = c.join("");
        if (!cornerCache.has(key)) cornerCache.set(key, tiles.map((t, i) => (t.corners.join("") === key ? i : -1)).filter((i) => i >= 0));
        return cornerCache.get(key);
      },
      plain(c) { return tiles.findIndex((t) => t.roads === 0 && t.rivers === 0 && t.corners.every((v, i) => v === c[i])); },
    };
  }

  const api = { Rng, salted, fromWord, kit, Dims, World, Problem, solve, arcConsistent, encode, decode, accepts, BRUSHES, SEA, GRASS, FOREST, MOUNTAIN, NONE, ROAD, RIVER };
  if (typeof module !== "undefined" && module.exports) module.exports = api;
  else root.TesseraEngine = api;
})(typeof window !== "undefined" ? window : globalThis);

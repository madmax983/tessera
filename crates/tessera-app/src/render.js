// Tessera renderer: an old atlas plate, drawn one tile at a time.
//
// Terrain is shaded per pixel from the four corner heights (bilinear, plus a
// little world-space noise so coasts are ragged but seamless across tiles);
// rivers, roads, towns, trees and peaks are drawn over it, clipped to the
// tile, so any tile can be redrawn alone. A player replays solver traces:
// placements pop in with a glow in the painter's colour, rewinds flash red.
(function (root) {
  "use strict";

  const hash = (x, y, s) => {
    let h = (Math.imul(x | 0, 374761393) + Math.imul(y | 0, 668265263) + Math.imul(s | 0, 2147483647)) | 0;
    h = Math.imul(h ^ (h >>> 13), 1274126177);
    return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
  };
  const lerp = (a, b, t) => a + (b - a) * t;
  function vnoise(x, y, s, seed) {
    const gx = Math.floor(x / s), gy = Math.floor(y / s);
    const fx = x / s - gx, fy = y / s - gy;
    const sx = fx * fx * (3 - 2 * fx), sy = fy * fy * (3 - 2 * fy);
    return lerp(lerp(hash(gx, gy, seed), hash(gx + 1, gy, seed), sx), lerp(hash(gx, gy + 1, seed), hash(gx + 1, gy + 1, seed), sx), sy);
  }
  const hex = (h) => [parseInt(h.slice(1, 3), 16), parseInt(h.slice(3, 5), 16), parseInt(h.slice(5, 7), 16)];
  const mix = (a, b, t) => [lerp(a[0], b[0], t), lerp(a[1], b[1], t), lerp(a[2], b[2], t)];
  const P = {
    deep: hex("#14324f"), sea: hex("#1f4f72"), shallow: hex("#3b8199"), surf: hex("#7fb9bf"),
    sand: hex("#e3cf97"), grass: hex("#a3c06c"), meadow: hex("#86ad5a"), wood: hex("#5f8a46"),
    forest: hex("#3e6a3b"), rock: hex("#8d8372"), crag: hex("#6f6759"), snow: hex("#f4f1e8"),
  };

  function terrain(h, n2) {
    if (h < 0.5) {
      const d = h / 0.5;
      if (d < 0.55) return mix(P.deep, P.sea, d / 0.55);
      if (d < 0.9) return mix(P.sea, P.shallow, (d - 0.55) / 0.35);
      return mix(P.shallow, P.surf, (d - 0.9) / 0.1);
    }
    if (h < 0.6) return mix(P.sand, P.grass, Math.max(0, (h - 0.55) / 0.05));
    if (h < 1.5) return mix(P.grass, P.meadow, (h - 0.6) / 0.9 * 0.8 + n2 * 0.2);
    if (h < 2.5) return mix(P.wood, P.forest, Math.min(1, (h - 1.5) / 0.6) * 0.7 + n2 * 0.3);
    if (h < 2.82) return mix(P.rock, P.crag, n2 * 0.6 + (h - 2.5) * 0.6);
    return P.snow;
  }

  function create(canvas, kit, dims, opts) {
    const o = Object.assign({ cell: 15 }, opts || {});
    const dpr = Math.max(1, Math.min(2, root.devicePixelRatio || 1));
    const C = Math.round(o.cell * dpr);
    canvas.width = dims.w * C;
    canvas.height = dims.h * C;
    canvas.style.width = "100%";
    canvas.style.aspectRatio = `${dims.w} / ${dims.h}`;
    const ctx = canvas.getContext("2d");
    const tiles = kit.tiles;
    let shown = new Array(dims.cells()).fill(-1);
    const img = ctx.createImageData(C, C);

    function bilinear(t, u, v) {
      const c = t.corners;
      return lerp(lerp(c[0], c[1], u), lerp(c[3], c[2], u), v);
    }
    function height(t, wx, wy, u, v) {
      const h = bilinear(t, u, v);
      const n = vnoise(wx, wy, C * 0.55, 7) - 0.5 + (vnoise(wx, wy, C * 0.18, 9) - 0.5) * 0.45;
      return h + n * 0.42;
    }

    function base(cell, t) {
      const [cx, cy] = dims.xy(cell), x0 = cx * C, y0 = cy * C, d = img.data;
      for (let py = 0; py < C; py++) for (let px = 0; px < C; px++) {
        const u = (px + 0.5) / C, v = (py + 0.5) / C, wx = x0 + px, wy = y0 + py;
        const h = height(t, wx, wy, u, v);
        const hn = height(t, wx - 1.5, wy - 1.5, u - 1.5 / C, v - 1.5 / C);
        const n2 = vnoise(wx, wy, C * 0.3, 3);
        let col, shade = 1;
        if (h >= 0.5) {
          col = terrain(h, n2);
          shade = 1 + Math.max(-0.35, Math.min(0.35, (hn - h) * -2.4));
        } else {
          // The sea: depth from the smooth corner field, not the coast noise.
          const b = bilinear(t, u, v), big = vnoise(wx, wy, C * 3, 5) - 0.5;
          col = terrain(Math.max(0, Math.min(0.499, b * 0.62 + big * 0.12 + 0.04)), n2);
          if (h > 0.42) col = mix(col, P.surf, (h - 0.42) / 0.08 * 0.55);
        }
        const i = (py * C + px) * 4;
        d[i] = Math.min(255, col[0] * shade); d[i + 1] = Math.min(255, col[1] * shade); d[i + 2] = Math.min(255, col[2] * shade); d[i + 3] = 255;
      }
      ctx.putImageData(img, x0, y0);
    }

    function mids(cx, cy) {
      const x0 = cx * C, y0 = cy * C, m = C / 2;
      return [[x0 + m, y0], [x0 + C, y0 + m], [x0 + m, y0 + C], [x0, y0 + m]];
    }

    function strokeFeature(cx, cy, edges, f, styles) {
      const M = mids(cx, cy), c = [cx * C + C / 2, cy * C + C / 2];
      const ds = [0, 1, 2, 3].filter((d) => edges[d] === f);
      for (const [w, col] of styles) {
        ctx.lineWidth = w; ctx.strokeStyle = col; ctx.lineCap = "round";
        ctx.beginPath();
        if (ds.length === 2) {
          ctx.moveTo(M[ds[0]][0], M[ds[0]][1]);
          ctx.quadraticCurveTo(c[0], c[1], M[ds[1]][0], M[ds[1]][1]);
        } else {
          for (const d of ds) { ctx.moveTo(M[d][0], M[d][1]); ctx.lineTo(c[0], c[1]); }
        }
        ctx.stroke();
      }
      return ds;
    }

    function trees(cell, t) {
      const [cx, cy] = dims.xy(cell);
      if (!t.corners.some((v) => v >= 2)) return;
      for (let k = 0; k < 7; k++) {
        const u = 0.12 + hash(cell, k, 31) * 0.76, v = 0.12 + hash(cell, k, 37) * 0.76;
        const wx = cx * C + u * C, wy = cy * C + v * C;
        const h = height(t, wx, wy, u, v);
        if (h < 1.45 || h > 2.55) continue;
        if (t.roads + t.rivers > 0 && Math.abs(u - 0.5) < 0.2 && Math.abs(v - 0.5) < 0.2) continue;
        const r = C * (0.1 + hash(cell, k, 41) * 0.05);
        ctx.fillStyle = "rgba(20,35,20,0.35)";
        ctx.beginPath(); ctx.arc(wx + r * 0.4, wy + r * 0.5, r, 0, 6.283); ctx.fill();
        ctx.fillStyle = "#2c5530";
        ctx.beginPath(); ctx.arc(wx, wy, r, 0, 6.283); ctx.fill();
        ctx.fillStyle = "#4f8146";
        ctx.beginPath(); ctx.arc(wx - r * 0.3, wy - r * 0.35, r * 0.45, 0, 6.283); ctx.fill();
      }
    }

    function peaks(cell, t) {
      const [cx, cy] = dims.xy(cell);
      if (!t.corners.some((v) => v === 3)) return;
      for (let k = 0; k < 2; k++) {
        const u = 0.25 + hash(cell, k, 53) * 0.5, v = 0.35 + hash(cell, k, 59) * 0.45;
        const wx = cx * C + u * C, wy = cy * C + v * C;
        const h = height(t, wx, wy, u, v);
        if (h < 2.45) continue;
        const b = C * (0.36 + hash(cell, k, 61) * 0.2), ht = b * 0.95;
        ctx.fillStyle = "#a69c8b";
        ctx.beginPath(); ctx.moveTo(wx - b / 2, wy + ht / 3); ctx.lineTo(wx, wy - ht * 0.66); ctx.lineTo(wx, wy + ht / 3); ctx.fill();
        ctx.fillStyle = "#6d6557";
        ctx.beginPath(); ctx.moveTo(wx, wy - ht * 0.66); ctx.lineTo(wx + b / 2, wy + ht / 3); ctx.lineTo(wx, wy + ht / 3); ctx.fill();
        if (h > 2.7) {
          ctx.fillStyle = "#f6f3ea";
          ctx.beginPath(); ctx.moveTo(wx - b * 0.17, wy - ht * 0.32); ctx.lineTo(wx, wy - ht * 0.66); ctx.lineTo(wx + b * 0.17, wy - ht * 0.32); ctx.lineTo(wx, wy - ht * 0.4); ctx.fill();
        }
      }
    }

    function town(cell) {
      const [cx, cy] = dims.xy(cell), x = cx * C + C / 2, y = cy * C + C / 2;
      ctx.fillStyle = "rgba(226,210,170,0.9)"; ctx.strokeStyle = "#6e5a3a"; ctx.lineWidth = Math.max(1, C * 0.07);
      ctx.beginPath(); ctx.arc(x, y, C * 0.43, 0, 6.283); ctx.fill(); ctx.stroke();
      const spots = [[-0.2, -0.16], [0.17, -0.18], [-0.17, 0.18], [0.2, 0.15], [0, 0]];
      const n = 3 + Math.floor(hash(cell, 1, 71) * 3);
      for (let k = 0; k < n; k++) {
        const [dx, dy] = spots[k];
        const hx = x + dx * C + (hash(cell, k, 73) - 0.5) * C * 0.06, hy = y + dy * C;
        const w = C * 0.24, h = C * 0.17;
        ctx.fillStyle = "rgba(30,20,10,0.35)"; ctx.fillRect(hx - w / 2 + 1.5, hy - h / 2 + 1.5, w, h);
        ctx.fillStyle = "#f2e7cf"; ctx.fillRect(hx - w / 2, hy - h / 2, w, h);
        ctx.fillStyle = k === 4 ? "#7a3b2a" : "#b4472d";
        ctx.beginPath(); ctx.moveTo(hx - w * 0.62, hy - h / 2); ctx.lineTo(hx, hy - h * 1.35); ctx.lineTo(hx + w * 0.62, hy - h / 2); ctx.fill();
      }
    }

    function draw(cell, id) {
      shown[cell] = id;
      if (id < 0) return fog(cell, 0.88);
      const t = tiles[id], [cx, cy] = dims.xy(cell);
      base(cell, t);
      ctx.save();
      ctx.beginPath(); ctx.rect(cx * C, cy * C, C, C); ctx.clip();
      trees(cell, t);
      peaks(cell, t);
      if (t.rivers) {
        const ds = strokeFeature(cx, cy, t.edges, 2, [[C * 0.24, "#2d6f9e"], [C * 0.12, "#6aa8d0"]]);
        const c = [cx * C + C / 2, cy * C + C / 2];
        if (t.center === "spring") {
          ctx.fillStyle = "#2d6f9e"; ctx.beginPath(); ctx.arc(c[0], c[1], C * 0.2, 0, 6.283); ctx.fill();
          ctx.fillStyle = "#8cc3e0"; ctx.beginPath(); ctx.arc(c[0], c[1], C * 0.1, 0, 6.283); ctx.fill();
        } else if (t.center === "mouth") {
          const M = mids(cx, cy), o = M[(ds[0] + 2) % 4];
          const px = (o[1] - c[1]) * 0.55, py = (o[0] - c[0]) * 0.55;
          ctx.fillStyle = "#4a8cb8";
          ctx.beginPath(); ctx.moveTo(c[0] - px * 0.25, c[1] - py * 0.25); ctx.lineTo(o[0] - px, o[1] - py);
          ctx.lineTo(o[0] + px, o[1] + py); ctx.lineTo(c[0] + px * 0.25, c[1] + py * 0.25); ctx.fill();
        }
      }
      if (t.roads) {
        strokeFeature(cx, cy, t.edges, 1, [[C * 0.2, "#6b5537"], [C * 0.1, "#ead8a6"]]);
        if (t.center === "bridge") {
          const vertical = t.edges[0] === 1;
          ctx.strokeStyle = "#3d2f1f"; ctx.lineWidth = Math.max(1, C * 0.05);
          ctx.beginPath();
          const x = cx * C + C / 2, y = cy * C + C / 2, a = C * 0.12, b = C * 0.3;
          if (vertical) { ctx.moveTo(x - a, y - b); ctx.lineTo(x - a, y + b); ctx.moveTo(x + a, y - b); ctx.lineTo(x + a, y + b); }
          else { ctx.moveTo(x - b, y - a); ctx.lineTo(x + b, y - a); ctx.moveTo(x - b, y + a); ctx.lineTo(x + b, y + a); }
          ctx.stroke();
        }
      }
      if (t.center === "town") town(cell);
      ctx.restore();
    }

    function fog(cell, a) {
      const [cx, cy] = dims.xy(cell);
      ctx.fillStyle = `rgba(9,15,22,${a})`;
      ctx.fillRect(cx * C, cy * C, C, C);
      if (a > 0.8) {
        ctx.strokeStyle = "rgba(160,190,210,0.07)"; ctx.lineWidth = 1;
        ctx.strokeRect(cx * C + 0.5, cy * C + 0.5, C - 1, C - 1);
      }
    }

    function drawAll(ids) { ids.forEach((id, c) => draw(c, id)); }

    // ── The player ──────────────────────────────────────────────────────────
    const jobs = [];
    let job = null, glows = [], speed = o.speed || 10, raf = 0;
    const now = () => (root.performance ? performance.now() : Date.now());

    function glow(cell, color, life) { glows.push({ cell, color, t0: now(), life: life || 520 }); }

    function start(j) {
      job = j; job.i = 0; job.live = [];
      if (j.bare) drawAll(j.bare);
      for (const c of j.region) fog(c, j.veil == null ? 0.62 : j.veil);
      if (j.onStart) j.onStart(j);
    }

    function stepJob() {
      let budget = Math.max(1, Math.round(job.speed || speed));
      while (budget-- > 0 && job.i < job.trace.length) {
        const [c, v] = job.trace[job.i++];
        if (c >= 0) { draw(c, v); job.live.push(c); glow(c, job.hue); }
        else {
          const undone = job.live.splice(v);
          for (const u of undone) { fog(u, 0.82); shown[u] = -1; glow(u, "#ff5a4f", 700); }
          if (job.onRewind) job.onRewind(undone.length);
        }
      }
      if (job.i >= job.trace.length) {
        job.region.forEach((c, k) => { if (shown[c] !== job.tiles[k]) draw(c, job.tiles[k]); });
        const done = job; job = null;
        if (done.onDone) done.onDone(done);
      }
    }

    function frame() {
      raf = 0;
      if (!job && jobs.length) start(jobs.shift());
      if (job) stepJob();
      const t = now();
      glows = glows.filter((g) => t - g.t0 < g.life);
      for (const g of glows) {
        if (shown[g.cell] >= 0) draw(g.cell, shown[g.cell]); else fog(g.cell, 0.82);
        const [cx, cy] = dims.xy(g.cell), a = 1 - (t - g.t0) / g.life;
        ctx.globalAlpha = a * 0.75; ctx.fillStyle = g.color; ctx.fillRect(cx * C, cy * C, C, C);
        ctx.globalAlpha = 1;
      }
      if (job || jobs.length || glows.length) raf = requestAnimationFrame(frame);
      if (o.onFrame) o.onFrame();
    }
    function kick() { if (!raf) raf = requestAnimationFrame(frame); }

    function enqueue(j) { jobs.push(j); kick(); }
    function finish() {
      // Skip to the end of everything queued.
      while (job || jobs.length) { if (!job) start(jobs.shift()); job.i = job.trace.length; stepJob(); }
      glows = []; drawAll(shown.slice());
    }
    function cellAt(evt) {
      const r = canvas.getBoundingClientRect();
      const x = Math.floor((evt.clientX - r.left) / r.width * dims.w), y = Math.floor((evt.clientY - r.top) / r.height * dims.h);
      return x >= 0 && y >= 0 && x < dims.w && y < dims.h ? [x, y] : null;
    }
    function mark(x, y, color) {
      ctx.strokeStyle = color; ctx.lineWidth = Math.max(2, C * 0.12);
      ctx.strokeRect(x * C + ctx.lineWidth / 2, y * C + ctx.lineWidth / 2, C - ctx.lineWidth, C - ctx.lineWidth);
    }

    return {
      C, ctx, draw, drawAll, fog, enqueue, finish, cellAt, mark, glow,
      busy: () => !!job || jobs.length > 0,
      shown: () => shown,
      setShown: (ids) => { shown = ids.slice(); },
      setSpeed: (s) => { speed = s; },
      redraw: () => drawAll(shown.slice()),
    };
  }

  root.TesseraRender = { create };
})(typeof window !== "undefined" ? window : globalThis);

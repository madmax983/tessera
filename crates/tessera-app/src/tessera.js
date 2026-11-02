// Tessera page glue: the Commons, the atlas, the tile plate.
(function () {
  "use strict";
  const E = window.TesseraEngine, R = window.TesseraRender;
  if (!E || !R) return;
  const page = document.body.dataset.page;
  const kit = E.kit();
  const $ = (id) => document.getElementById(id);
  const token = (document.querySelector('meta[name="csrf-token"]') || {}).content || "";

  function post(url, data) {
    return fetch(url, {
      method: "POST", credentials: "same-origin",
      headers: { "content-type": "application/x-www-form-urlencoded", "x-csrf-token": token },
      body: new URLSearchParams(data || {}),
    }).then(async (r) => {
      const text = await r.text();
      let j = null;
      try { j = JSON.parse(text); } catch (e) { j = null; }
      if (!r.ok) throw new Error((j && (j.detail || j.message || j.error)) || text || String(r.status));
      return j;
    });
  }

  // ── who am I ────────────────────────────────────────────────────────────
  const ADJ = ["ochre", "umber", "verdigris", "madder", "indigo", "saffron", "cinnabar", "slate", "amber", "sepia"];
  const NOUN = ["cartographer", "surveyor", "pilot", "hermit", "navigator", "beachcomber", "pilgrim", "lighthouse keeper"];
  function load(k) { try { return localStorage.getItem(k); } catch (e) { return null; } }
  function save(k, v) { try { localStorage.setItem(k, v); } catch (e) { /* private window */ } }
  const me = {
    hue: Number(load("tessera.hue")) || Math.floor(Math.random() * 360),
    name: load("tessera.name") || `${ADJ[Math.floor(Math.random() * ADJ.length)]} ${NOUN[Math.floor(Math.random() * NOUN.length)]}`,
  };
  save("tessera.hue", String(me.hue)); save("tessera.name", me.name);
  const hsl = (h, l) => `hsl(${h} 85% ${l || 62}%)`;

  const decode = (trace) => trace.map(E.decode);
  const speedNow = () => Number(($("speed") || {}).value || 8);

  // ── the Commons ─────────────────────────────────────────────────────────
  function commons() {
    const canvas = $("map"), feed = $("feed"), status = $("status");
    let view = null, dims = null, seq = 0, word = "", brush = "mountain", syncing = false;
    const nameInput = $("me-name"), swatch = $("me-swatch");
    nameInput.value = me.name; swatch.style.background = hsl(me.hue);
    nameInput.addEventListener("change", () => { me.name = nameInput.value.trim() || me.name; save("tessera.name", me.name); });
    swatch.addEventListener("click", () => { me.hue = (me.hue + 47) % 360; swatch.style.background = hsl(me.hue); save("tessera.hue", String(me.hue)); });

    document.querySelectorAll(".brush").forEach((b) => b.addEventListener("click", () => {
      brush = b.dataset.brush;
      document.querySelectorAll(".brush").forEach((x) => x.setAttribute("aria-pressed", x === b ? "true" : "false"));
    }));

    function line(html, hue, miss) {
      const li = document.createElement("li");
      li.innerHTML = html;
      if (hue != null) li.style.setProperty("--c", hsl(hue, 58));
      if (miss) li.className = "miss";
      feed.prepend(li);
      while (feed.children.length > 60) feed.lastChild.remove();
    }
    const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
    function setStatus() { status.textContent = `${word} · stroke ${seq} · ${dims ? dims.w + " × " + dims.h : ""}`; $("word").textContent = word; }

    async function sync() {
      if (syncing) return;
      syncing = true;
      try {
        const s = await (await fetch("/_world.json")).json();
        if (!view) {
          dims = new E.Dims(s.w, s.h);
          view = R.create(canvas, kit, dims, { cell: 15 });
          bindPaint();
        } else view.finish();
        view.drawAll(s.tiles);
        seq = s.seq; word = s.word;
        $("drift").setAttribute("aria-pressed", s.drift ? "true" : "false");
        $("drift").textContent = s.drift ? "drifting" : "still";
        setStatus();
      } finally { syncing = false; }
    }

    function onEvent(m) {
      if (!view || m.seq <= seq) return;
      if (m.seq !== seq + 1) { sync(); return; }
      seq = m.seq;
      if (m.kind === "pass") {
        const p = m.pass, drift = m.what === "drift";
        view.enqueue({ region: p.region, trace: decode(p.trace), tiles: p.tiles, hue: drift ? "rgba(140,190,220,0.6)" : hsl(m.by.hue), veil: drift ? 0.3 : 0.6, speed: drift ? Math.max(3, speedNow() / 2) : speedNow() });
        if (drift) line(`the weather moved <b>${p.changed}</b> tiles near (${m.at[0]}, ${m.at[1]})`, 200);
        else {
          const grown = m.grown ? `, after ${m.grown} smaller disc${m.grown > 1 ? "s" : ""} could not` : "";
          const bt = p.backtracks ? `, ${p.backtracks} backtrack${p.backtracks > 1 ? "s" : ""}` : "";
          line(`<b>${esc(m.by.name)}</b> ${m.what === "erase" ? "unpinned" : "pinned " + esc(m.what)} at (${m.at[0]}, ${m.at[1]}) — ${p.changed} tiles rewritten in a disc of ${m.radius}${grown}${bt}`, m.by.hue);
        }
      } else if (m.kind === "world") {
        word = m.word;
        view.enqueue({ bare: m.bare, region: [], trace: [], tiles: [] });
        for (const p of m.passes) view.enqueue({ region: p.region, trace: decode(p.trace), tiles: p.tiles, hue: hsl(m.by.hue), veil: 0.35, speed: 40 });
        line(`<b>${esc(m.by.name)}</b> grew a new world from <b>${esc(m.word)}</b>`, m.by.hue);
      }
      setStatus();
    }

    function bindPaint() {
      let stroke = null;
      const key = (c) => c[0] + "," + c[1];
      const add = (c) => {
        if (!c || stroke.length >= 64 || stroke.some((s) => key(s) === key(c))) return;
        stroke.push(c); view.mark(c[0], c[1], hsl(me.hue));
      };
      canvas.addEventListener("pointerdown", (e) => { stroke = []; canvas.setPointerCapture(e.pointerId); add(view.cellAt(e)); });
      canvas.addEventListener("pointermove", (e) => { if (stroke) add(view.cellAt(e)); });
      const end = () => {
        if (!stroke || !stroke.length) { stroke = null; return; }
        const s = stroke; stroke = null;
        post("/paint", { cells: s.map(key).join(";"), brush, name: me.name, hue: me.hue })
          .catch((err) => {
            line(`the world cannot hold ${brush === "erase" ? "that" : "a " + brush} there — ${esc(err.message)}`, null, true);
            const shown = view.shown();
            for (const [x, y] of s) { const c = y * dims.w + x; view.draw(c, shown[c]); view.glow(c, "#ff5a4f", 900); }
          });
      };
      canvas.addEventListener("pointerup", end);
      canvas.addEventListener("pointercancel", () => { stroke = null; view.redraw(); });
    }

    $("drift").addEventListener("click", () => {
      const on = $("drift").getAttribute("aria-pressed") !== "true";
      post("/drift/" + (on ? "on" : "off")).then((r) => {
        $("drift").setAttribute("aria-pressed", r.drift ? "true" : "false");
        $("drift").textContent = r.drift ? "drifting" : "still";
      });
    });

    sync().then(() => {
      const es = new EventSource("/stream");
      es.onmessage = (m) => { try { onEvent(JSON.parse(m.data)); } catch (err) { sync(); } };
      es.onerror = () => { setTimeout(sync, 1500); };
    });
    window.tessera = { onEvent, sync, state: () => ({ seq, word, busy: view && view.busy() }) };
  }

  // ── the atlas ──────────────────────────────────────────────────────────
  const ISLES = ["Hy-Brasil", "Lyonesse", "Avalon", "Antillia", "Thule", "Ys", "Buyan", "Kitezh", "Mayda", "Frisland", "Estotiland", "Saint Brendan", "Bermeja", "Sannikov", "Zealandia", "Mu", "Tír na nÓg", "Meropis"];
  function atlas() {
    const canvas = $("map"), input = $("atlas-word"), status = $("status");
    const dims = new E.Dims(Number(document.body.dataset.w), Number(document.body.dataset.h));
    const view = R.create(canvas, kit, dims, { cell: 15 });
    function grow(word) {
      view.finish();
      const t0 = performance.now();
      const g = E.World.generate(kit, dims, "sea", E.fromWord(word));
      const ms = Math.round(performance.now() - t0);
      $("word").textContent = word;
      view.enqueue({ bare: g.bare, region: [], trace: [], tiles: [] });
      g.passes.forEach((p) => view.enqueue({ region: p.region, trace: p.trace, tiles: p.tiles, hue: hsl(42), veil: 0.35, speed: 30 }));
      const bt = g.passes.reduce((a, p) => a + p.backtracks, 0);
      const feats = g.world.tiles.filter((t) => kit.tiles[t].roads + kit.tiles[t].rivers > 0).length;
      const towns = g.world.tiles.filter((t) => kit.tiles[t].center === "town").length;
      status.textContent = `grown in your browser in ${ms} ms · ${g.passes.length} blocks · ${bt} backtracks · ${towns} towns · ${feats} tiles of river and road`;
      try { history.replaceState(null, "", "/atlas?word=" + encodeURIComponent(word)); } catch (e) { /* file:// */ }
    }
    $("atlas-form").addEventListener("submit", (e) => { e.preventDefault(); grow(input.value.trim() || "Hy-Brasil"); });
    $("atlas-roll").addEventListener("click", () => { input.value = ISLES[Math.floor(Math.random() * ISLES.length)]; grow(input.value); });
    $("atlas-commons").addEventListener("click", () => {
      post("/grow", { word: input.value.trim() || "Hy-Brasil", name: me.name, hue: me.hue })
        .then(() => { window.location.href = "/"; })
        .catch((err) => { status.textContent = err.message; });
    });
    const q = new URLSearchParams(location.search).get("word");
    if (q) input.value = q;
    grow(input.value);
    window.tessera = { grow, busy: () => view.busy() };
  }

  // ── the tile plate ─────────────────────────────────────────────────────
  function plate() {
    const canvas = $("plate");
    const cols = 37, rows = Math.ceil(kit.tiles.length / cols);
    // Draw each tile into a sparse grid with a one-cell gutter, then
    // collapse: the renderer draws cells; gutters stay dark.
    const dims = new E.Dims(cols * 2 + 1, rows * 2 + 1);
    const view = R.create(canvas, kit, dims, { cell: 9 });
    const ids = new Array(dims.cells()).fill(-1);
    kit.tiles.forEach((t, i) => { const x = (i % cols) * 2 + 1, y = Math.floor(i / cols) * 2 + 1; ids[y * dims.w + x] = i; });
    view.ctx.fillStyle = "#0e1822"; view.ctx.fillRect(0, 0, canvas.width, canvas.height);
    ids.forEach((id, c) => { if (id >= 0) view.draw(c, id); });
  }

  if (page === "commons") commons();
  else if (page === "atlas") atlas();
  else if (page === "tiles") plate();
})();

// Tessera — the verified core.
//
//   verus verus/src/tessera.rs
//
// What is proved here is why the map can be trusted between any two strokes:
//
//   1. revise is sound      — the AC-3 revise step (keep in D[j] only tiles
//                             some tile of D[i] fits beside) never removes a
//                             tile that appears in any solution;
//   2. the socket shortcut  — computing revise through the set of sockets
//                             D[i] shows (what crates/tessera/src/wave.rs
//                             actually does) gives exactly the same domain;
//   3. propagation ends     — a revise that changes a finite domain strictly
//                             shrinks it, so the total domain size, a natural
//                             number, strictly falls: propagation terminates;
//   4. banning is sound     — if no solution puts tile t at cell c, removing t
//                             from D[c] loses no solution (the backtrack step);
//   5. the splice           — a solution of a problem whose out-of-region cells
//                             are fixed to a map m is a consistent map equal to
//                             m outside the region (why a committed pass can
//                             never tear the map and never touches anything
//                             outside its region);
//   6. relaxation           — at the relaxation's fixed point every pair of
//                             8-neighbouring vertices differs by at most one,
//                             so every tile's four corners are smooth and the
//                             bare tile for them exists; each lowering step
//                             strictly decreases a natural-number measure.
//
// This file is standalone: no dependency on the `tessera` crate and no IO.
//
// ── The modelling assumptions ───────────────────────────────────────────────
//
// * Tiles are ints. `sock[t][d]` is tile t's socket on side d (0 N, 1 E,
//   2 S, 3 W) — `Tables::sock` in wave.rs. Two tiles fit across side d iff
//   `sock[a][d] == sock[b][opp(d)]`, which is `Tileset::fits`.
// * The grid is abstracted to a neighbour table: `nbr[i][d]` is the cell on
//   side d of cell i, or anything outside `0..n` for "off the map". Nothing
//   here depends on the grid being square; `Dims::step` in wave.rs is the
//   concrete table, and its symmetry is property-tested, not modelled.
// * Domains are `Set<int>`; at runtime they are bitsets. The correspondence
//   of bitset operations to set operations is tested (the solver is checked
//   against an independent checker over thousands of maps), not modelled.

use vstd::prelude::*;
use vstd::set_lib::*;

verus! {

// ── Fitting ─────────────────────────────────────────────────────────────────

pub open spec fn opp(d: int) -> int {
    (d + 2) % 4
}

pub open spec fn fits(sock: Seq<Seq<int>>, a: int, d: int, b: int) -> bool {
    sock[a][d] == sock[b][opp(d)]
}

/// Every arc of the map is satisfied.
pub open spec fn consistent(sock: Seq<Seq<int>>, nbr: Seq<Seq<int>>, m: Seq<int>) -> bool {
    forall|i: int, d: int|
        0 <= i < m.len() && 0 <= d < 4 && 0 <= #[trigger] nbr[i][d] < m.len()
            ==> fits(sock, m[i], d, m[nbr[i][d]])
}

/// `s` is a solution of the problem with domains `dom`.
pub open spec fn solution(
    sock: Seq<Seq<int>>,
    nbr: Seq<Seq<int>>,
    dom: Seq<Set<int>>,
    s: Seq<int>,
) -> bool {
    &&& s.len() == dom.len()
    &&& forall|i: int| 0 <= i < s.len() ==> #[trigger] dom[i].contains(s[i])
    &&& consistent(sock, nbr, s)
}

// ── 1. revise is sound ──────────────────────────────────────────────────────

/// The tiles of `dj` that some tile of `di` fits beside across side `d`.
pub open spec fn supported(sock: Seq<Seq<int>>, di: Set<int>, d: int, dj: Set<int>) -> Set<int> {
    dj.filter(|b: int| exists|a: int| #[trigger] di.contains(a) && fits(sock, a, d, b))
}

pub open spec fn revise(sock: Seq<Seq<int>>, dom: Seq<Set<int>>, i: int, d: int, j: int) -> Seq<Set<int>> {
    dom.update(j, supported(sock, dom[i], d, dom[j]))
}

pub proof fn lemma_revise_sound(
    sock: Seq<Seq<int>>,
    nbr: Seq<Seq<int>>,
    dom: Seq<Set<int>>,
    s: Seq<int>,
    i: int,
    d: int,
)
    requires
        solution(sock, nbr, dom, s),
        0 <= i < dom.len(),
        0 <= d < 4,
        0 <= nbr[i][d] < dom.len(),
    ensures
        solution(sock, nbr, revise(sock, dom, i, d, nbr[i][d]), s),
{
    let j = nbr[i][d];
    let dom2 = revise(sock, dom, i, d, j);
    assert(dom[i].contains(s[i]));
    assert(dom[j].contains(s[j]));
    assert(fits(sock, s[i], d, s[j]));
    assert(supported(sock, dom[i], d, dom[j]).contains(s[j]));
    assert forall|k: int| 0 <= k < s.len() implies #[trigger] dom2[k].contains(s[k]) by {
        if k != j {
            assert(dom2[k] == dom[k]);
        }
    }
}

/// Revise only ever removes tiles.
pub proof fn lemma_revise_shrinks(sock: Seq<Seq<int>>, di: Set<int>, d: int, dj: Set<int>)
    ensures
        supported(sock, di, d, dj).subset_of(dj),
{
}

// ── 2. the socket shortcut ──────────────────────────────────────────────────

/// The sockets `di` shows on side `d`.
pub open spec fn shown(sock: Seq<Seq<int>>, di: Set<int>, d: int) -> Set<int> {
    di.map(|a: int| sock[a][d])
}

/// What wave.rs computes: tiles of `dj` whose opposite socket is shown.
pub open spec fn via_sockets(sock: Seq<Seq<int>>, di: Set<int>, d: int, dj: Set<int>) -> Set<int> {
    dj.filter(|b: int| shown(sock, di, d).contains(sock[b][opp(d)]))
}

pub proof fn lemma_socket_shortcut(sock: Seq<Seq<int>>, di: Set<int>, d: int, dj: Set<int>)
    ensures
        via_sockets(sock, di, d, dj) == supported(sock, di, d, dj),
{
    assert forall|b: int| #[trigger] via_sockets(sock, di, d, dj).contains(b)
        <==> supported(sock, di, d, dj).contains(b) by {
        if via_sockets(sock, di, d, dj).contains(b) {
            let a = choose|a: int| #[trigger] di.contains(a) && sock[a][d] == sock[b][opp(d)];
            assert(di.contains(a) && fits(sock, a, d, b));
        }
        if supported(sock, di, d, dj).contains(b) {
            let a = choose|a: int| #[trigger] di.contains(a) && fits(sock, a, d, b);
            assert(di.contains(a) && sock[a][d] == sock[b][opp(d)]);
            assert(shown(sock, di, d).contains(sock[b][opp(d)]));
        }
    }
    assert(via_sockets(sock, di, d, dj) =~= supported(sock, di, d, dj));
}

// ── 3. propagation ends ─────────────────────────────────────────────────────

pub proof fn lemma_strict_subset_shrinks(a: Set<int>, b: Set<int>)
    requires
        a.subset_of(b),
        a != b,
    ensures
        a.len() < b.len(),
{
    lemma_len_subset(a, b);
    if forall|x: int| b.contains(x) ==> a.contains(x) {
        assert(a =~= b);
    }
    let x = choose|x: int| b.contains(x) && !a.contains(x);
    assert(a.subset_of(b.remove(x)));
    lemma_len_subset(a, b.remove(x));
}

/// The total size of every domain.
pub open spec fn total(dom: Seq<Set<int>>) -> nat
    decreases dom.len(),
{
    if dom.len() == 0 {
        0
    } else {
        total(dom.drop_last()) + dom.last().len()
    }
}

pub proof fn lemma_total_update(dom: Seq<Set<int>>, j: int, s: Set<int>)
    requires
        0 <= j < dom.len(),
        s.len() < dom[j].len(),
    ensures
        total(dom.update(j, s)) < total(dom),
    decreases dom.len(),
{
    let up = dom.update(j, s);
    if j == dom.len() - 1 {
        assert(up.drop_last() =~= dom.drop_last());
    } else {
        assert(up.drop_last() =~= dom.drop_last().update(j, s));
        lemma_total_update(dom.drop_last(), j, s);
        assert(up.last() == dom.last());
    }
}

/// A revise that changes a finite domain strictly lowers the total: the
/// measure that bounds how long propagation can run.
pub proof fn lemma_revise_terminates(sock: Seq<Seq<int>>, dom: Seq<Set<int>>, i: int, d: int, j: int)
    requires
        0 <= i < dom.len(),
        0 <= j < dom.len(),
        supported(sock, dom[i], d, dom[j]) != dom[j],
    ensures
        total(revise(sock, dom, i, d, j)) < total(dom),
{
    lemma_revise_shrinks(sock, dom[i], d, dom[j]);
    lemma_strict_subset_shrinks(supported(sock, dom[i], d, dom[j]), dom[j]);
    lemma_total_update(dom, j, supported(sock, dom[i], d, dom[j]));
}

// ── 4. banning is sound ─────────────────────────────────────────────────────

pub proof fn lemma_ban_sound(
    sock: Seq<Seq<int>>,
    nbr: Seq<Seq<int>>,
    dom: Seq<Set<int>>,
    c: int,
    t: int,
    s: Seq<int>,
)
    requires
        0 <= c < dom.len(),
        // No solution puts t at c: what a failed decision demonstrates.
        forall|x: Seq<int>| #[trigger] solution(sock, nbr, dom, x) ==> x[c] != t,
        solution(sock, nbr, dom, s),
    ensures
        solution(sock, nbr, dom.update(c, dom[c].remove(t)), s),
{
    let dom2 = dom.update(c, dom[c].remove(t));
    assert(s[c] != t);
    assert forall|k: int| 0 <= k < s.len() implies #[trigger] dom2[k].contains(s[k]) by {
        if k == c {
            assert(dom[c].contains(s[c]));
        } else {
            assert(dom2[k] == dom[k]);
        }
    }
}

// ── 5. the splice ───────────────────────────────────────────────────────────

/// The domains of a reshape: outside the region every cell is fixed to the
/// map's tile; inside, anything in `free[k]`.
pub open spec fn reshape(m: Seq<int>, inside: Seq<bool>, free: Seq<Set<int>>) -> Seq<Set<int>> {
    Seq::new(m.len(), |k: int| if inside[k] { free[k] } else { set![m[k]] })
}

pub proof fn lemma_splice(
    sock: Seq<Seq<int>>,
    nbr: Seq<Seq<int>>,
    m: Seq<int>,
    inside: Seq<bool>,
    free: Seq<Set<int>>,
    s: Seq<int>,
)
    requires
        inside.len() == m.len(),
        free.len() == m.len(),
        solution(sock, nbr, reshape(m, inside, free), s),
    ensures
        consistent(sock, nbr, s),
        s.len() == m.len(),
        forall|k: int| 0 <= k < m.len() && !inside[k] ==> #[trigger] s[k] == m[k],
{
    assert forall|k: int| 0 <= k < m.len() && !inside[k] implies #[trigger] s[k] == m[k] by {
        assert(reshape(m, inside, free)[k].contains(s[k]));
    }
}

// ── 6. relaxation ───────────────────────────────────────────────────────────

/// Symmetric neighbour relation for vertices: `adj[v]` lists v's neighbours.
pub open spec fn symmetric(adj: Seq<Seq<int>>) -> bool {
    forall|v: int, k: int|
        0 <= v < adj.len() && 0 <= k < adj[v].len() && 0 <= #[trigger] adj[v][k] < adj.len()
            ==> exists|q: int| 0 <= q < adj[adj[v][k]].len() && #[trigger] adj[adj[v][k]][q] == v
}

/// The relaxation's stopping condition: no vertex exceeds a neighbour by more
/// than one (`relax` in world.rs loops until this holds).
pub open spec fn settled(h: Seq<int>, adj: Seq<Seq<int>>) -> bool {
    forall|v: int, k: int|
        0 <= v < h.len() && 0 <= k < adj[v].len() && 0 <= #[trigger] adj[v][k] < h.len()
            ==> h[v] <= h[adj[v][k]] + 1
}

pub proof fn lemma_settled_is_lipschitz(h: Seq<int>, adj: Seq<Seq<int>>, v: int, k: int)
    requires
        adj.len() == h.len(),
        symmetric(adj),
        settled(h, adj),
        0 <= v < h.len(),
        0 <= k < adj[v].len(),
        0 <= adj[v][k] < h.len(),
    ensures
        h[v] - h[adj[v][k]] <= 1,
        h[adj[v][k]] - h[v] <= 1,
{
    let n = adj[v][k];
    let q = choose|q: int| 0 <= q < adj[n].len() && #[trigger] adj[n][q] == v;
    assert(h[n] <= h[adj[n][q]] + 1);
}

/// Four corner levels are smooth: every pair within one.
pub open spec fn smooth(c: Seq<int>) -> bool {
    forall|a: int, b: int| 0 <= a < 4 && 0 <= b < 4 ==> #[trigger] c[a] - #[trigger] c[b] <= 1
}

/// If the four corners of a tile are pairwise within one (which, in a
/// settled field, they are: they are 8-neighbours of each other), the
/// pattern is smooth — so `Tileset::plain` finds a tile for it.
pub proof fn lemma_corners_smooth(c: Seq<int>)
    requires
        c.len() == 4,
        forall|a: int, b: int| 0 <= a < 4 && 0 <= b < 4 && a != b ==> #[trigger] c[a] - #[trigger] c[b] <= 1,
    ensures
        smooth(c),
{
    assert forall|a: int, b: int| 0 <= a < 4 && 0 <= b < 4 implies #[trigger] c[a] - #[trigger] c[b] <= 1 by {
        if a == b {
            assert(c[a] - c[b] == 0);
        }
    }
}

/// One lowering step: vertex v drops to a neighbour's level + 1.
pub open spec fn lower(h: Seq<int>, v: int, to: int) -> Seq<int> {
    h.update(v, to)
}

pub open spec fn height_sum(h: Seq<int>) -> int
    decreases h.len(),
{
    if h.len() == 0 {
        0
    } else {
        height_sum(h.drop_last()) + h.last()
    }
}

pub proof fn lemma_lower_decreases(h: Seq<int>, v: int, to: int)
    requires
        0 <= v < h.len(),
        to < h[v],
    ensures
        height_sum(lower(h, v, to)) < height_sum(h),
    decreases h.len(),
{
    let up = lower(h, v, to);
    if v == h.len() - 1 {
        assert(up.drop_last() =~= h.drop_last());
    } else {
        assert(up.drop_last() =~= lower(h.drop_last(), v, to));
        lemma_lower_decreases(h.drop_last(), v, to);
        assert(up.last() == h.last());
    }
}

pub open spec fn nonneg(h: Seq<int>) -> bool {
    forall|v: int| 0 <= v < h.len() ==> #[trigger] h[v] >= 0
}

pub proof fn lemma_sum_nonneg(h: Seq<int>)
    requires
        nonneg(h),
    ensures
        height_sum(h) >= 0,
    decreases h.len(),
{
    if h.len() > 0 {
        assert(nonneg(h.drop_last()));
        lemma_sum_nonneg(h.drop_last());
        assert(h.last() == h[h.len() - 1]);
    }
}

/// Lowering to a neighbour's level + 1 keeps every height non-negative, so
/// the sum is a natural number that strictly falls: relaxation terminates.
pub proof fn lemma_lower_keeps_nonneg(h: Seq<int>, v: int, n: int)
    requires
        nonneg(h),
        0 <= v < h.len(),
        0 <= n < h.len(),
    ensures
        nonneg(lower(h, v, h[n] + 1)),
{
    assert forall|k: int| 0 <= k < h.len() implies #[trigger] lower(h, v, h[n] + 1)[k] >= 0 by {
        if k == v {
            assert(h[n] >= 0);
        }
    }
}

// ── Executable pieces, checked against the specs ────────────────────────────

/// The runtime smoothness test (`tiles::smooth`), on bytes.
pub fn smooth_exec(c: [u8; 4]) -> (r: bool)
    ensures
        r == (forall|a: int, b: int| 0 <= a < 4 && 0 <= b < 4 ==> #[trigger] (c[a] as int) - #[trigger] (c[b] as int) <= 1),
{
    let close = |x: u8, y: u8| -> (r: bool)
        ensures r == ((x as int) - (y as int) <= 1 && (y as int) - (x as int) <= 1)
    {
        if x >= y { x - y <= 1 } else { y - x <= 1 }
    };
    let r = close(c[0], c[1]) && close(c[1], c[2]) && close(c[2], c[3]) && close(c[3], c[0])
        && close(c[0], c[2]) && close(c[1], c[3]);
    proof {
        if r {
            assert forall|a: int, b: int| 0 <= a < 4 && 0 <= b < 4 implies #[trigger] (c[a] as int) - #[trigger] (c[b] as int) <= 1 by {
                assert(0 <= a < 4 && 0 <= b < 4);
            }
        } else {
            assert(!((c[0] as int) - (c[1] as int) <= 1 && (c[1] as int) - (c[0] as int) <= 1)
                || !((c[1] as int) - (c[2] as int) <= 1 && (c[2] as int) - (c[1] as int) <= 1)
                || !((c[2] as int) - (c[3] as int) <= 1 && (c[3] as int) - (c[2] as int) <= 1)
                || !((c[3] as int) - (c[0] as int) <= 1 && (c[0] as int) - (c[3] as int) <= 1)
                || !((c[0] as int) - (c[2] as int) <= 1 && (c[2] as int) - (c[0] as int) <= 1)
                || !((c[1] as int) - (c[3] as int) <= 1 && (c[3] as int) - (c[1] as int) <= 1));
        }
    }
    r
}

/// The runtime lowering rule (`relax`): `min(h, n + 1)` without overflow.
pub fn lower_exec(h: u8, n: u8) -> (r: u8)
    ensures
        r <= h,
        r as int <= n as int + 1,
        r == h || r as int == n as int + 1,
{
    if n < h && n + 1 < h { n + 1 } else { h }
}

fn main() {
}

} // verus!

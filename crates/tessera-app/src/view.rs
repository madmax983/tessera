//! The shell every page is rendered into, and the pages themselves.
//!
//! An atlas plate at night: deep ink around the map, parchment type, and
//! colour that means something — the sea is blue because it is sea, a glow
//! is the colour of whoever painted it, red is the solver changing its mind.
//!
//! Autumn sets `script-src 'self'`, so every script is served from this app
//! and wired by `data-` attributes, never inline.

use autumn_web::{Markup, PreEscaped, html};
use tessera::tiles::{Center, Tileset};

use crate::commons::{HEIGHT, WIDTH};

/// The stylesheet.
pub const STYLE: &str = include_str!("tessera.css");
/// The engine port.
pub const ENGINE: &str = include_str!("engine.js");
/// The renderer.
pub const RENDER: &str = include_str!("render.js");
/// The page glue.
pub const SCRIPT: &str = include_str!("tessera.js");

fn page(title: &str, here: &str, csrf: Option<&str>, data_page: &str, content: &Markup) -> Markup {
    let nav = [
        ("/", "The Commons"),
        ("/atlas", "Atlas"),
        ("/tiles", "The tiles"),
        ("/about", "How it works"),
    ];
    html! {
        (PreEscaped("<!DOCTYPE html>"))
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) " — Tessera" }
                @if let Some(t) = csrf { meta name="csrf-token" content=(t); }
                style { (PreEscaped(STYLE)) }
                script src="/engine.js" defer {}
                script src="/render.js" defer {}
                script src="/tessera.js" defer {}
            }
            body data-page=(data_page) data-w=(WIDTH) data-h=(HEIGHT) {
                div class="wrap" {
                    header class="top" {
                        h1 { a href="/" { "Tessera" } }
                        span class="tag" { "a map that redraws itself to keep its promises" }
                        nav {
                            @for (href, label) in nav {
                                a href=(href) class=(if href == here { "here" } else { "" }) { (label) }
                            }
                        }
                    }
                    (content)
                    footer {
                        "Two tiles may sit side by side only when the edge they share agrees: the same "
                        "two heights at its ends, the same road or river across it. That is the only "
                        "rule. Coasts, rivers that find the sea and roads that find a town all fall out of it."
                    }
                }
            }
        }
    }
}

fn brushes() -> Markup {
    let list = [
        ("mountain", "Mountain", "▲"),
        ("forest", "Forest", "♣"),
        ("grass", "Meadow", "≈"),
        ("sea", "Sea", "∿"),
        ("river", "River", "〰"),
        ("road", "Road", "═"),
        ("town", "Town", "⌂"),
        ("erase", "Unpin", "○"),
    ];
    html! {
        div class="brushes" role="toolbar" aria-label="Brushes" {
            @for (id, label, glyph) in list {
                button type="button" class=(format!("brush b-{id}")) data-brush=(id) aria-pressed=(if id == "mountain" { "true" } else { "false" }) {
                    span class="glyph" { (glyph) } span { (label) }
                }
            }
        }
    }
}

/// The Commons.
pub fn commons_page(csrf: Option<&str>) -> Markup {
    let content = html! {
        main class="commons" {
            section class="plate" {
                div class="frame" {
                    canvas id="map" aria-label="The shared island" {}
                    div class="cartouche" {
                        span class="label" { "The Commons" }
                        span id="word" class="word" { "…" }
                    }
                }
                div class="under" {
                    span id="status" { "growing…" }
                    span class="spacer" {}
                    label class="speed" { "replay " input id="speed" type="range" min="1" max="60" value="8"; }
                }
            }
            aside class="panel" {
                h2 { "Paint" }
                p class="hint" { "Pick a brush and drag across the map. The solver tears up a disc around your stroke and mends it — growing the disc until the world can hold what you asked for." }
                (brushes())
                div class="me" {
                    span class="swatch" id="me-swatch" {}
                    input id="me-name" type="text" maxlength="32" aria-label="Your name";
                }
                h2 { "Weather" }
                p class="hint" { "Left alone, the world keeps redrawing its rivers and roads. Pinned cells hold." }
                button id="drift" type="button" class="toggle" aria-pressed="true" { "drifting" }
                h2 { "Chronicle" }
                ol id="feed" class="feed" {}
            }
        }
    };
    page("The Commons", "/", csrf, "commons", &content)
}

/// The atlas.
pub fn atlas_page(csrf: Option<&str>) -> Markup {
    let content = html! {
        main class="atlas" {
            section class="plate" {
                form id="atlas-form" class="ask" {
                    label for="atlas-word" { "Every word is an island:" }
                    input id="atlas-word" type="text" maxlength="40" value="Hy-Brasil";
                    button type="submit" { "Grow" }
                    button id="atlas-roll" type="button" class="quiet" { "another" }
                }
                div class="frame" {
                    canvas id="map" aria-label="An island" {}
                    div class="cartouche" { span class="label" { "Atlas" } span id="word" class="word" { "…" } }
                }
                div class="under" {
                    span id="status" { "" }
                    span class="spacer" {}
                    button id="atlas-commons" type="button" class="quiet" { "make this the Commons" }
                }
            }
        }
    };
    page("Atlas", "/atlas", csrf, "atlas", &content)
}

/// The tileset plate.
pub fn tiles_page(ts: &Tileset) -> Markup {
    let count = |c: Center| ts.tiles().iter().filter(|t| t.center == c).count();
    let rivers = ts.tiles().iter().filter(|t| t.rivers() > 0).count();
    let roads = ts.tiles().iter().filter(|t| t.roads() > 0).count();
    let content = html! {
        main class="tiles" {
            p class="lede" {
                (ts.len()) " tiles. Corners are heights — sea, meadow, forest, mountain — and no two corners "
                "of one tile differ by more than one. Edges carry nothing, a road, or a river. "
                (roads) " tiles carry a road, " (rivers) " a river; " (count(Center::Town)) " are towns, "
                (count(Center::Spring)) " springs, " (count(Center::Mouth)) " river mouths and "
                (count(Center::Bridge)) " bridges. Every road dead end is a town, and a river only ever "
                "ends at a spring or a mouth — so nothing in a finished map ever stops in the middle of a field."
            }
            canvas id="plate" aria-label="Every tile" {}
        }
    };
    page("The tiles", "/tiles", None, "tiles", &content)
}

/// How it works.
pub fn about_page() -> Markup {
    let content = html! {
        main class="about" {
            h2 { "One rule" }
            p { "A tile has a height at each corner and, on each edge, nothing, a road or a river. Two tiles may share an edge only if they agree about it completely. The solver never draws a river and never routes a road; it only ever refuses neighbours that disagree, and the rivers fall out." }
            h2 { "The solver" }
            p { "Every cell starts as the set of tiles it could still be. " em { "Propagation" } " removes a tile from a cell when no tile left next door could share the edge with it; that never removes a tile that appears in any finished map. " em { "Decision" } " picks the cell with the fewest tiles left and chooses one by weight. When propagation empties a cell, the solver rewinds to its last decision and bans the tile it chose — that is the red flash." }
            h2 { "A world that is always finished" }
            p { "A world begins as bare geology — heights from integer noise, relaxed until no two neighbouring corners differ by more than one, which is already a consistent map. After that it only ever changes by re-solving a region with everything outside held fixed, and a region is committed only if it solves. So the map keeps every promise between every two strokes, not just at the end." }
            p { "Generation sweeps overlapping blocks across the geology in a shuffled order, laying rivers, roads and towns over land it keeps. A paint pins a brush to your stroke and re-solves a disc around it, terrain and all, growing the disc until the world can hold the pin. The weather re-solves a small disc every few seconds with the land held still, so rivers wander and towns come and go — everywhere except where someone has pinned something." }
            h2 { "What is proved" }
            p { "In " code { "verus/" } ": that propagation never loses a solution, that it always shrinks a domain when it changes one (so it terminates), that banning a tile which led nowhere loses nothing, that splicing a solved region into a consistent map gives a consistent map, and that the relaxation leaves every tile's corners smooth. " code { "./verify.sh" } " runs it." }
        }
    };
    page("How it works", "/about", None, "about", &content)
}

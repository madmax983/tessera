//! The HTTP surface against a real Autumn app with a real Commons.

use autumn_web::config::AutumnConfig;
use autumn_web::test::{TestApp, TestClient};

fn client() -> TestClient {
    let mut config = AutumnConfig {
        profile: Some("test".into()),
        ..AutumnConfig::default()
    };
    // The browser sends the token in a header; the CSRF layer is Autumn's own
    // and is exercised by the end-to-end run in verify.sh, not here.
    config.security.csrf.enabled = false;
    TestApp::new()
        .config(config)
        .routes(tessera_app::all_routes!())
        .state_initializer(|state| {
            tessera_app::install(state, "Tessera").expect("the Commons grows");
        })
        .build()
}

fn json(r: &autumn_web::test::TestResponse) -> serde_json::Value {
    serde_json::from_str(&r.text()).expect("a JSON body")
}

#[tokio::test]
async fn every_page_serves() {
    let c = client();
    c.get("/")
        .send()
        .await
        .assert_status(200)
        .assert_body_contains("canvas id=\"map\"");
    c.get("/atlas")
        .send()
        .await
        .assert_status(200)
        .assert_body_contains("Every word is an island");
    c.get("/tiles")
        .send()
        .await
        .assert_status(200)
        .assert_body_contains("444 tiles");
    c.get("/about")
        .send()
        .await
        .assert_status(200)
        .assert_body_contains("What is proved");
    for (path, marker) in [
        ("/engine.js", "TesseraEngine"),
        ("/render.js", "TesseraRender"),
        ("/tessera.js", "window.tessera"),
    ] {
        c.get(path)
            .send()
            .await
            .assert_status(200)
            .assert_body_contains(marker);
    }
}

#[tokio::test]
async fn the_world_starts_whole() {
    let c = client();
    let r = c.get("/_world.json").send().await;
    r.assert_status(200);
    let s = json(&r);
    assert_eq!(s["w"], 64);
    assert_eq!(s["h"], 40);
    assert_eq!(s["seq"], 0);
    assert_eq!(s["word"], "Tessera");
    assert_eq!(s["tiles"].as_array().unwrap().len(), 64 * 40);
    assert_eq!(s["pins"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn a_paint_lands_pins_and_advances_the_sequence() {
    let c = client();
    let r = c
        .post("/paint")
        .form("cells=20,12;21,12&brush=mountain&name=ochre&hue=30")
        .send()
        .await;
    r.assert_status(200);
    let reply = json(&r);
    assert_eq!(reply["seq"], 1);
    assert!(reply["radius"].as_u64().unwrap() >= 3);
    let s = json(&c.get("/_world.json").send().await);
    assert_eq!(s["seq"], 1);
    let pins = s["pins"].as_array().unwrap();
    assert_eq!(pins.len(), 2);
    assert_eq!(pins[0][1], "mountain");
    // Unpinning is a paint too.
    let r = c
        .post("/paint")
        .form("cells=20,12&brush=erase")
        .send()
        .await;
    r.assert_status(200);
    let s = json(&c.get("/_world.json").send().await);
    assert_eq!(s["pins"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn an_impossible_or_malformed_paint_is_refused() {
    let c = client();
    // A town on the shore of an island map cannot exist.
    c.post("/paint")
        .form("cells=0,0&brush=town")
        .send()
        .await
        .assert_status(422);
    c.post("/paint")
        .form("cells=0,0&brush=lava")
        .send()
        .await
        .assert_status(400);
    c.post("/paint")
        .form("cells=zero&brush=sea")
        .send()
        .await
        .assert_status(400);
    c.post("/paint")
        .form("cells=99,0&brush=sea")
        .send()
        .await
        .assert_status(422);
    let s = json(&c.get("/_world.json").send().await);
    assert_eq!(s["seq"], 0);
}

#[tokio::test]
async fn growing_a_word_replaces_the_world_and_the_weather_toggles() {
    let c = client();
    let before = json(&c.get("/_world.json").send().await);
    let r = c.post("/grow").form("word=Lyonesse&name=me").send().await;
    r.assert_status(200);
    assert_eq!(json(&r)["word"], "Lyonesse");
    let after = json(&c.get("/_world.json").send().await);
    assert_eq!(after["word"], "Lyonesse");
    assert_ne!(after["tiles"], before["tiles"]);
    c.post("/grow")
        .form("word=")
        .send()
        .await
        .assert_status(422);
    let r = c.post("/drift/off").send().await;
    r.assert_status(200);
    assert_eq!(json(&r)["drift"], false);
    assert_eq!(json(&c.get("/_world.json").send().await)["drift"], false);
    c.post("/drift/sideways").send().await.assert_status(400);
}

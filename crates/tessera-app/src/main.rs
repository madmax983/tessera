// Tessera — a map that redraws itself to keep its promises.
//
// Run with:  cargo run --release -p tessera-app
// Then open: http://localhost:3000   (open it twice: the Commons is shared)

#[autumn_web::main]
async fn main() {
    Box::pin(
        autumn_web::app()
            .on_startup(tessera_app::boot)
            .routes(tessera_app::all_routes!())
            .run(),
    )
    .await;
}

#[tokio::main]
async fn main() {
    let res = motis_openapi_progenitor::Client::new("http://localhost:8080")
        .plan()
        .from_place("52.5173885,13.3951309")
        .to_place("53.550341,10.000654")
        .detailed_transfers(false)
        .send()
        .await
        .unwrap();
    println!("{res:?}");
}


use tracing::{info, warn};

pub async fn fetch_text(source_url: &String) -> Result<String, Box<dyn std::error::Error>> {
    info!("Fetching text from '{source_url}' ...");

    let response = reqwest::get(source_url).await?;
    match response.status() {
        reqwest::StatusCode::OK => {
            info!("Response status: OK");

            let text = response.text().await?;
            info!("Fetched text");
            Ok(text)
        },
        status => {
            warn!("Response status: NOT OK: {status}");
            Err("failed to fetch geojson".into())
        }
    }
}

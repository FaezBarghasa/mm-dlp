#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    mm_dlp_cli::run_cli().await
}

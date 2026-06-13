use clap::Parser;

/// Command line arguments for mm-dlp-cli.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The URL to download from.
    #[arg(short, long)]
    url: String,
}

/// The main entry point of the CLI application.
#[tokio::main]
async fn main() {
    // Parse the command line arguments.
    let args = Args::parse();
    // Print a message indicating the download URL.
    println!("Downloading from: {}", args.url);
}

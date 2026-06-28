use clap::Parser;

/// Command line arguments for the `mm-dlp-cli` tool.
///
/// This struct defines the CLI interface using `clap`. It automatically 
/// generates terminal help messages and parses incoming arguments.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The target URL to download media from.
    /// 
    /// This expects a valid URL string pointing to a supported media platform.
    #[arg(short, long)]
    url: String,
}

/// The main entry point of the CLI application.
///
/// Uses the `tokio` runtime to enable asynchronous operations, which is
/// essential for making non-blocking network requests during concurrent downloads.
#[tokio::main]
async fn main() {
    // Parse the command line arguments provided by the user into the `Args` struct.
    let args = Args::parse();

    // Log the initiation of the download process to standard output.
    println!("Downloading from: {}", args.url);
}

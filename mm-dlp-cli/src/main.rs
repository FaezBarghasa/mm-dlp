use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(short, long)]
    url: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    println!("Downloading from: {}", args.url);
}

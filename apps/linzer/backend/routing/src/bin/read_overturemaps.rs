use clap::Parser;
use overturemaps::overturemaps::OvertureMaps;

/// Load some data from overturemaps
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// input partitioned root directory
    #[arg(long)]
    overturemaps: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    println!("{:?}", args);

    let mut om = OvertureMaps::load_from_base(args.overturemaps).await?;
    om.do_something().await?;

    Ok(())
}

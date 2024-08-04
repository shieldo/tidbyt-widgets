use crate::Args as LibArgs;
use clap::Parser;
use dotenvy::dotenv;
use std::time::Duration;
use tidbyt_rs::render;
use tokio::time::sleep;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Filename of the debug file
    #[arg(short, long)]
    debug: Option<String>,
    #[arg(short, long)]
    retry: Option<u64>,
}

impl From<Args> for LibArgs {
    fn from(value: LibArgs) -> Self {
        Self { ..value }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = dotenv();
    let args = Args::parse();
    let duration = args.retry.map(|retry_time| Duration::from_secs(retry_time));

    loop {
        render(args.into()).await?;

        if args.debug.is_some() {
            break;
        }

        match duration {
            Some(duration) => sleep(duration).await,
            None => break,
        }
    }

    Ok(())
}

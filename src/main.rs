use anyhow::Result;
use clap::Parser;

mod canonical;
mod cli;
mod config;
mod errors;
mod fetcher;
mod output;
mod process;
mod state;
mod transcribe;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    init_tracing(cli.global.log_format);
    let cfg = config::Config::from_args(&cli.global);
    tracing::info!(profile = ?cfg.profile, state_db = ?cfg.state_db, "config resolved");

    match cli.command {
        cli::Command::Init => {
            tracing::info!("init: not yet implemented (Task 7+)");
        }
        cli::Command::Ingest { dry_run: _ } => {
            tracing::info!("ingest: not yet implemented (Task 13)");
        }
        cli::Command::Process { max_videos: _ } => {
            tracing::info!("process: not yet implemented (Task 14)");
        }
    }

    Ok(())
}

fn init_tracing(format: cli::LogFormat) {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    match format {
        cli::LogFormat::Human => {
            tracing_subscriber::fmt().with_env_filter(filter).init();
        }
        cli::LogFormat::Json => {
            tracing_subscriber::fmt()
                .json()
                .with_env_filter(filter)
                .init();
        }
    }
}

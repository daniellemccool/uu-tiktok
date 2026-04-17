use anyhow::{Context, Result};
use clap::Parser;

mod canonical;
mod cli;
mod config;
mod errors;
mod fetcher;
mod ingest;
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
        cli::Command::Ingest { dry_run } => {
            let mut store = state::Store::open(&cfg.state_db).context("opening state DB")?;
            if dry_run {
                tracing::info!("dry-run: not yet implemented; running real ingest");
            }
            let stats = ingest::ingest(&cfg.inbox, &mut store).context("ingest failed")?;
            tracing::info!(
                files = stats.files_processed,
                videos = stats.unique_videos_seen,
                history = stats.watch_history_rows_processed,
                duplicates = stats.watch_history_duplicates,
                short_links_skipped = stats.short_links_skipped,
                invalid_urls_skipped = stats.invalid_urls_skipped,
                "ingest complete"
            );
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

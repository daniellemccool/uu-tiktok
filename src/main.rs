use anyhow::{Context, Result};
use clap::Parser;

mod canonical;
mod cli;
mod config;
mod errors;
mod fetcher;
mod ingest;
mod output;
mod pipeline;
mod process;
mod state;
mod transcribe;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();
    init_tracing(cli.global.log_format);
    let cfg = config::Config::from_args(&cli.global);
    tracing::info!(
        profile = ?cfg.profile,
        state_db = ?cfg.state_db,
        whisper_model_path = ?cfg.whisper_model_path,
        "config resolved"
    );

    match cli.command {
        cli::Command::Init => {
            let path = &cfg.state_db;
            if path.exists() {
                let store = state::Store::open(path)?;
                if let Some(version) = store.read_meta("schema_version")? {
                    tracing::info!(
                        path = %path.display(),
                        version = version.as_str(),
                        "state.sqlite already initialized; nothing to do"
                    );
                    return Ok(());
                }
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).context("creating state.sqlite parent dir")?;
            }
            let _store = state::Store::open(path)?;
            tracing::info!(path = %path.display(), "state.sqlite initialized");
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
        cli::Command::Process { max_videos } => {
            let mut store = state::Store::open(&cfg.state_db).context("opening state DB")?;
            std::fs::create_dir_all(&cfg.transcripts).context("creating transcripts dir")?;
            // Tmp cleanup at startup
            let removed = output::artifacts::cleanup_tmp_files(&cfg.transcripts)?;
            if removed > 0 {
                tracing::info!(removed, "cleaned up leftover .tmp files");
            }

            let work_dir = cfg.transcripts.join(".work");
            std::fs::create_dir_all(&work_dir).context("creating work dir")?;

            let fetcher = fetcher::ytdlp::YtDlpFetcher::new(&work_dir, cfg.ytdlp_timeout);
            let model_path = cfg.whisper_model_path.clone();
            let use_gpu = cfg.whisper_use_gpu;
            let threads = cfg.whisper_threads;
            let timeout = cfg.transcribe_timeout;

            let opts = pipeline::ProcessOptions {
                worker_id: format!("{}-{}", hostname_or_default(), std::process::id()),
                transcripts_root: cfg.transcripts.clone(),
                max_videos,
                transcriber: Box::new(move |path| {
                    let opts = transcribe::TranscribeOptions {
                        model_path: model_path.clone(),
                        use_gpu,
                        threads,
                        timeout,
                    };
                    let path = path.to_path_buf();
                    Box::pin(async move { transcribe::transcribe(&path, &opts).await })
                }),
            };

            let stats = pipeline::run_serial(&mut store, &fetcher, opts).await?;
            tracing::info!(
                claimed = stats.claimed,
                succeeded = stats.succeeded,
                failed = stats.failed,
                "process complete"
            );
            if stats.claimed == 0 {
                std::process::exit(3);
            }
        }
    }

    Ok(())
}

fn hostname_or_default() -> String {
    std::env::var("HOSTNAME").unwrap_or_else(|_| "host".to_string())
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

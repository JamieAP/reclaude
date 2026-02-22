use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "reclaude",
    version,
    about = "Semantic event capture and analysis for Claude Code sessions"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Hook entry point for event capture (perf-critical, reads JSON from stdin)
    Capture {
        /// Hook type (e.g., UserPromptSubmit, PostToolUse, Stop, SessionStart:startup)
        hook_type: String,
    },

    /// Show recent events
    Events(EventsArgs),

    /// List recent sessions
    Sessions(SessionsArgs),

    /// Show capture statistics
    Status,

    /// View conversation around an event
    Chat(ChatArgs),

    /// Full-text and semantic search over events
    Search(SearchArgs),

    /// Find files modified by Claude
    Files(FilesArgs),

    /// Time-bucketed activity summary across repos
    Recap(RecapArgs),

    /// Synthesize focus summary via Gemini
    Focus(FocusArgs),

    /// Archive and extract session transcripts
    Transcripts {
        #[command(subcommand)]
        command: TranscriptsCommand,
    },

    /// Manage plan file capture
    Plans {
        #[command(subcommand)]
        command: PlansCommand,
    },

    /// Manage embedding model and backfill vectors
    Embed {
        #[command(subcommand)]
        command: EmbedCommand,
    },

    /// Launch web UI server
    Ui(UiArgs),

    /// Tail the capture log
    Log(LogArgs),

    /// Tag current session for later retrieval
    #[command(name = "tag-session")]
    TagSession,

    /// Get session ID by tag
    #[command(name = "get-session")]
    GetSession(GetSessionArgs),

    /// Backfill events from legacy capture.db
    Backfill(BackfillArgs),
}

// ── Backfill ────────────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct BackfillArgs {
    /// Path to legacy capture.db (default: ~/.reclaude/capture.db)
    #[arg(long)]
    pub from: Option<String>,

    /// Dry run: show counts without modifying anything
    #[arg(long)]
    pub dry_run: bool,
}

// ── Events ──────────────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct EventsArgs {
    /// Max events to show (default varies by type)
    #[arg(short = 'n', long)]
    pub limit: Option<usize>,

    /// Filter by session ID (prefix match)
    #[arg(short, long)]
    pub session: Option<String>,

    /// Filter by event type(s): prompt, diff, plan, tool, compaction
    #[arg(short = 't', long = "type")]
    pub event_type: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Show full content (no truncation)
    #[arg(long)]
    pub full: bool,

    /// Semantic search query
    #[arg(long)]
    pub semantic: Option<String>,

    /// Scope to this directory (default: current directory)
    #[arg(long)]
    pub cwd: Option<String>,

    /// Search all repos (not just current)
    #[arg(long)]
    pub all: bool,

    /// Compact output for preview panes
    #[arg(long)]
    pub compact: bool,

    /// Interactive event browser with fzf
    #[arg(long)]
    pub fzf: bool,

    /// Show a single event by ID
    #[arg(long)]
    pub id: Option<i64>,
}

// ── Sessions ────────────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct SessionsArgs {
    /// Max sessions to show (default: 10)
    #[arg(short = 'n', long, default_value = "10")]
    pub limit: usize,

    /// Show sessions from all directories (default: cwd only)
    #[arg(long)]
    pub all: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Interactive select with fzf
    #[arg(long)]
    pub fzf: bool,

    /// Show full session IDs
    #[arg(long)]
    pub full_sid: bool,

    /// Emit synthetic session_end for crashed sessions
    #[arg(long)]
    pub heal: bool,
}

// ── Chat ────────────────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct ChatArgs {
    /// Event ID to navigate to
    pub event_id: Option<i64>,

    /// Filter by session ID (prefix match)
    #[arg(short, long)]
    pub session: Option<String>,

    /// Max events to show (0 = all in session)
    #[arg(short = 'n', long, default_value = "0")]
    pub limit: usize,

    /// Print to stdout instead of less
    #[arg(long)]
    pub no_pager: bool,

    /// Show all sessions across all repos
    #[arg(long)]
    pub all: bool,

    /// Pick session interactively with fzf
    #[arg(long)]
    pub fzf: bool,

    /// Show full session IDs
    #[arg(long)]
    pub full_sid: bool,
}

// ── Search ──────────────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct SearchArgs {
    /// Search terms
    pub query: Vec<String>,

    /// Require additional term (repeatable)
    #[arg(long = "and")]
    pub and_terms: Vec<String>,

    /// Include alternative term (repeatable)
    #[arg(long = "or")]
    pub or_terms: Vec<String>,

    /// Exclude term (repeatable)
    #[arg(long = "not")]
    pub not_terms: Vec<String>,

    /// Filter by event type
    #[arg(short = 't', long = "type")]
    pub event_type: Option<String>,

    /// Show full event content
    #[arg(short, long)]
    pub verbose: bool,

    /// Max results (default: 20)
    #[arg(short = 'n', long, default_value = "20")]
    pub limit: usize,

    /// Open results in fzf with chat preview
    #[arg(long)]
    pub fzf: bool,

    /// Rebuild FTS index first
    #[arg(long)]
    pub rebuild: bool,

    /// Scope search to this directory (default: current directory)
    #[arg(long)]
    pub cwd: Option<String>,

    /// Search all projects, not just current directory
    #[arg(long)]
    pub all: bool,

    /// Filter by session ID (prefix match)
    #[arg(short, long)]
    pub session: Option<String>,

    /// Semantic search (vector similarity)
    #[arg(long)]
    pub semantic: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Show full session IDs
    #[arg(long)]
    pub full_sid: bool,
}

// ── Files ───────────────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct FilesArgs {
    /// Filter by file path substring
    pub pattern: Option<String>,

    /// Filter by file extension (e.g. md, rs, tsx)
    #[arg(short, long)]
    pub ext: Option<String>,

    /// Filter by session ID (prefix match)
    #[arg(short, long)]
    pub session: Option<String>,

    /// Max results (default: 50)
    #[arg(short = 'n', long, default_value = "50")]
    pub limit: usize,

    /// Max events to scan (default: 5000)
    #[arg(long, default_value = "5000")]
    pub scan_limit: usize,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// Show full content
    #[arg(long)]
    pub full: bool,

    /// Include reads (default: only writes/edits)
    #[arg(long)]
    pub reads: bool,

    /// Show files from all directories (default: cwd only)
    #[arg(long)]
    pub all: bool,

    /// Stream paths live, poll for new (Ctrl+C to stop)
    #[arg(long)]
    pub stream: bool,
}

// ── Recap ───────────────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct RecapArgs {
    /// Time window: 2h, 24h, 7d (default: 24h)
    #[arg(default_value = "24h")]
    pub zoom: String,

    /// Show multi-bucket overview instead of zoomed detail
    #[arg(long)]
    pub overview: bool,

    /// Minimum events to show a repo (default: 10)
    #[arg(long, default_value = "10")]
    pub min_events: usize,

    /// Compact output (no file paths or prompts)
    #[arg(long)]
    pub compact: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

// ── Focus ───────────────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct FocusArgs {
    /// Time window: 15min, hour, 8hour, day, week
    #[arg(default_value = "hour")]
    pub scale: String,

    /// Gemini model (default: gemini-3-pro-preview)
    #[arg(long, default_value = "gemini-3-pro-preview")]
    pub model: String,

    /// Show what would be sent without calling Gemini
    #[arg(long)]
    pub dry_run: bool,

    /// Character budget per synthesis call (default: 120000, 0=unlimited)
    #[arg(long, default_value = "120000")]
    pub budget: usize,
}

// ── Embed ───────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum EmbedCommand {
    /// Download the ONNX embedding model
    Download,
    /// Show embedding model status
    Status,
    /// Backfill vectors for historical events
    Backfill {
        /// Max events to process
        #[arg(long, default_value = "1000")]
        limit: usize,
        /// Dry run (show counts only)
        #[arg(long)]
        dry_run: bool,
    },
}

// ── Transcripts ─────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum TranscriptsCommand {
    /// Scan and archive all transcripts
    Sync,

    /// List archived transcripts
    List(TranscriptsListArgs),

    /// Show transcript content
    Show {
        /// Session ID (prefix match)
        session_id: String,
    },

    /// Extract semantic events from transcripts
    Extract(TranscriptsExtractArgs),

    /// Show archive statistics
    Stats,
}

#[derive(clap::Args)]
pub struct TranscriptsListArgs {
    /// Max results (default: 20)
    #[arg(short = 'n', long, default_value = "20")]
    pub limit: usize,

    /// Include subagent transcripts
    #[arg(long)]
    pub subagents: bool,

    /// Interactive select with fzf
    #[arg(long)]
    pub fzf: bool,
}

#[derive(clap::Args)]
pub struct TranscriptsExtractArgs {
    /// Extract from specific file
    #[arg(short, long)]
    pub file: Option<String>,

    /// Hours to look back (default: 24)
    #[arg(long)]
    pub since: Option<u64>,

    /// Reprocess from beginning
    #[arg(long)]
    pub force: bool,

    /// Show what would be processed
    #[arg(long)]
    pub dry_run: bool,
}

// ── Plans ───────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum PlansCommand {
    /// Discover and ingest ~/.claude/plans/*.md
    Sync,
}

// ── UI Server ───────────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct UiArgs {
    /// Host to bind (default: 127.0.0.1)
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to bind (default: 8420)
    #[arg(long, default_value = "8420")]
    pub port: u16,

    /// Don't open browser
    #[arg(long)]
    pub no_open: bool,

    /// Enable auto-reload (dev mode)
    #[arg(long)]
    pub reload: bool,
}

// ── Log ─────────────────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct LogArgs {
    /// Initial lines to show (default: 20)
    #[arg(short = 'n', long = "lines", default_value = "20")]
    pub lines: usize,
}

// ── Tag/Get Session ─────────────────────────────────────────────────

#[derive(clap::Args)]
pub struct GetSessionArgs {
    /// Tag to look up
    pub tag: String,

    /// Exit silently if not found
    #[arg(short, long)]
    pub quiet: bool,
}

// ── Command Dispatch ────────────────────────────────────────────────

pub async fn dispatch(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Commands::Capture { ref hook_type } => {
            crate::capture::process_hook(hook_type).await
        }
        Commands::Events(ref args) => {
            let db = crate::db::Database::open().await?;
            crate::cmd::events::run(args, &db).await
        }
        Commands::Sessions(ref args) => {
            let db = crate::db::Database::open().await?;
            crate::cmd::sessions::run(args, &db).await
        }
        Commands::Status => {
            let db = crate::db::Database::open().await?;
            crate::cmd::status::run(&db).await
        }
        Commands::Chat(ref args) => {
            let db = crate::db::Database::open().await?;
            crate::cmd::chat::run(args, &db).await
        }
        Commands::Files(ref args) => {
            let db = crate::db::Database::open().await?;
            crate::cmd::files::run(args, &db).await
        }
        Commands::Search(ref args) => {
            let db = crate::db::Database::open().await?;
            crate::cmd::search::run(args, &db).await
        }
        Commands::Recap(ref args) => {
            let db = crate::db::Database::open().await?;
            crate::cmd::recap::run(args, &db).await
        }
        Commands::Focus(ref args) => {
            let db = crate::db::Database::open().await?;
            crate::cmd::focus::run(args, &db).await
        }
        Commands::Embed { ref command } => {
            let db = crate::db::Database::open().await?;
            crate::cmd::embed::run(command, &db).await
        }
        Commands::Transcripts { ref command } => {
            let db = crate::db::Database::open().await?;
            crate::cmd::transcripts::run(command, &db).await
        }
        Commands::Plans { ref command } => {
            match command {
                crate::cli::PlansCommand::Sync => {
                    let db = crate::db::Database::open().await?;
                    crate::extract::sync_plans(&db).await
                }
            }
        }
        Commands::Ui(ref args) => {
            crate::api::serve(&args.host, args.port, args.no_open).await
        }
        Commands::Log(ref args) => {
            let path = crate::logging::log_file_path();
            if !path.exists() {
                eprintln!("No log file at {}", path.display());
                return Ok(());
            }
            // Tail the log file
            let _ = std::process::Command::new("tail")
                .args(["-n", &args.lines.to_string(), "-f"])
                .arg(&path)
                .status();
            Ok(())
        }
        Commands::TagSession => {
            let db = crate::db::Database::open().await?;
            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            let tag = db.tag_session(&cwd).await?;
            println!("{tag}");
            Ok(())
        }
        Commands::GetSession(ref args) => {
            let db = crate::db::Database::open().await?;
            match db.get_session_by_tag(&args.tag).await? {
                Some(sid) => println!("{sid}"),
                None => {
                    if !args.quiet {
                        eprintln!("No session found for tag: {}", args.tag);
                    }
                }
            }
            Ok(())
        }
        Commands::Backfill(ref args) => {
            let db = crate::db::Database::open().await?;
            crate::cmd::backfill::run(args, &db).await
        }
    }
}

use crate::cli::EmbedCommand;
use crate::db::Database;
use crate::embed::NomicEmbedder;

pub async fn run(command: &EmbedCommand, db: &Database) -> anyhow::Result<()> {
    match command {
        EmbedCommand::Download => cmd_download().await,
        EmbedCommand::Status => cmd_status(),
        EmbedCommand::Backfill { limit, dry_run } => cmd_backfill(*limit, *dry_run, db).await,
    }
}

async fn cmd_download() -> anyhow::Result<()> {
    if NomicEmbedder::is_available() {
        println!("Model already downloaded at: {}", NomicEmbedder::model_dir().display());
        return Ok(());
    }
    NomicEmbedder::download().await?;
    Ok(())
}

fn cmd_status() -> anyhow::Result<()> {
    let dir = NomicEmbedder::model_dir();
    if NomicEmbedder::is_available() {
        println!("Model: nomic-embed-text-v1.5 (quantized)");
        println!("Path: {}", dir.display());
        println!("Status: ready");
    } else {
        println!("Model: not downloaded");
        println!("Run: reclaude embed download");
    }
    Ok(())
}

async fn cmd_backfill(limit: usize, dry_run: bool, db: &Database) -> anyhow::Result<()> {
    let events = db.query_unembedded(limit).await?;

    if events.is_empty() {
        println!("No unembedded events found.");
        return Ok(());
    }

    if dry_run {
        println!("Would embed {} events", events.len());
        return Ok(());
    }

    let mut embedder = match NomicEmbedder::load()? {
        Some(e) => e,
        None => {
            eprintln!("Model not downloaded. Run: reclaude embed download");
            return Ok(());
        }
    };

    eprintln!("Backfilling {} events...", events.len());
    let mut count = 0usize;
    let mut batch: Vec<(i64, Vec<f32>)> = Vec::with_capacity(500);

    for (i, event) in events.iter().enumerate() {
        eprint!("\r  [{}/{}]", i + 1, events.len());
        let text: String = event.content.chars().take(4000).collect();
        match embedder.embed_document(&text) {
            Ok(vec) => {
                batch.push((event.id, vec));
                count += 1;
            }
            Err(e) => {
                tracing::debug!("embed failed for event {}: {e}", event.id);
            }
        }

        if batch.len() >= 500 {
            db.update_vectors_batch(&batch).await?;
            batch.clear();
        }
    }

    // Flush remaining
    if !batch.is_empty() {
        db.update_vectors_batch(&batch).await?;
    }
    eprintln!();

    println!("Embedded: {count}/{}", events.len());
    Ok(())
}

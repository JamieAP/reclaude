use crate::db::Database;

/// Show capture statistics: total events and counts by type.
pub async fn run(db: &Database) -> anyhow::Result<()> {
    let total = db.events.count().await?;
    let counts = db.events.counts_by_type().await?;

    println!("Total events: {total}");
    println!();
    println!("By type:");
    for (event_type, count) in &counts {
        println!("  {event_type}: {count}");
    }

    Ok(())
}

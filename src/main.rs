mod api;
mod capture;
mod cli;
mod cmd;
mod db;
mod embed;
mod extract;
mod fzf;
mod gemini;
mod git;
mod logging;
mod models;

use clap::Parser;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logging::init();
    let cli = cli::Cli::parse();
    cli::dispatch(cli).await
}

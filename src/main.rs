mod cli;
mod client;
mod config;
mod output;
mod stdin;

use anyhow::Result;
use clap::Parser;
use cli::Cli;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{}: {:#}", colored::Colorize::red("error"), e);
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();

    // Load config, then overlay CLI flags on top
    let mut cfg = config::AppConfig::load()?;
    cfg.apply_cli_overrides(&cli);

    // Build the HTTP client
    let client = client::EsClient::new(&cfg)?;

    // Dispatch to subcommand handler
    cli::dispatch(cli.command, &client, &cfg).await
}

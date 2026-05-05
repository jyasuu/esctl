pub mod alias;
pub mod cluster;
pub mod document;
pub mod index;
pub mod mapping;
pub mod profile;
pub mod reindex;
pub mod search;
pub mod security;
pub mod settings;
pub mod snapshot;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::client::EsClient;
use crate::config::AppConfig;
use crate::output::OutputFormat;

/// esctl — A full-featured Elasticsearch CLI
#[derive(Parser, Debug)]
#[command(
    name = "esctl",
    version,
    about = "A full-featured Elasticsearch CLI written in Rust",
    long_about = None,
)]
pub struct Cli {
    /// Connection profile name (from ~/.esctl/config.toml)
    #[arg(long, short = 'p', global = true, env = "ESCTL_PROFILE")]
    pub profile: Option<String>,

    /// Elasticsearch host URL (overrides profile)
    #[arg(long, global = true, env = "ESCTL_HOST")]
    pub host: Option<String>,

    /// Basic auth username (overrides profile)
    #[arg(long, short = 'u', global = true, env = "ESCTL_USER")]
    pub user: Option<String>,

    /// Basic auth password (overrides profile)
    #[arg(long, global = true, env = "ESCTL_PASSWORD")]
    pub password: Option<String>,

    /// API key for authentication (overrides profile)
    #[arg(long, global = true, env = "ESCTL_API_KEY")]
    pub api_key: Option<String>,

    /// Output format
    #[arg(long, short = 'o', global = true, default_value = "json", value_enum)]
    pub output: OutputFormat,

    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Manage connection profiles
    #[command(subcommand)]
    Profile(profile::ProfileCommands),

    /// Cluster health, info, and stats
    #[command(subcommand)]
    Cluster(cluster::ClusterCommands),

    /// Index management
    #[command(subcommand)]
    Index(index::IndexCommands),

    /// Mapping management
    #[command(subcommand)]
    Mapping(mapping::MappingCommands),

    /// Index settings
    #[command(subcommand)]
    Settings(settings::SettingsCommands),

    /// Alias management
    #[command(subcommand)]
    Alias(alias::AliasCommands),

    /// Document operations
    #[command(subcommand)]
    Doc(document::DocCommands),

    /// Search and query
    #[command(subcommand)]
    Search(search::SearchCommands),

    /// Snapshot repository and snapshot management
    #[command(subcommand)]
    Snapshot(snapshot::SnapshotCommands),

    /// Reindex operations
    Reindex(reindex::ReindexArgs),

    /// Security: users, roles, API keys
    #[command(subcommand)]
    User(security::UserCommands),

    /// Role management
    #[command(subcommand)]
    Role(security::RoleCommands),

    /// API key management
    #[command(subcommand)]
    Apikey(security::ApikeyCommands),
}

/// Top-level dispatch — routes each subcommand to its handler module.
pub async fn dispatch(cmd: Commands, client: &EsClient, cfg: &AppConfig) -> Result<()> {
    match cmd {
        Commands::Profile(c)   => profile::handle(c, cfg).await,
        Commands::Cluster(c)   => cluster::handle(c, client, cfg).await,
        Commands::Index(c)     => index::handle(c, client, cfg).await,
        Commands::Mapping(c)   => mapping::handle(c, client, cfg).await,
        Commands::Settings(c)  => settings::handle(c, client, cfg).await,
        Commands::Alias(c)     => alias::handle(c, client, cfg).await,
        Commands::Doc(c)       => document::handle(c, client, cfg).await,
        Commands::Search(c)    => search::handle(c, client, cfg).await,
        Commands::Snapshot(c)  => snapshot::handle(c, client, cfg).await,
        Commands::Reindex(a)   => reindex::handle(a, client, cfg).await,
        Commands::User(c)      => security::handle_user(c, client, cfg).await,
        Commands::Role(c)      => security::handle_role(c, client, cfg).await,
        Commands::Apikey(c)    => security::handle_apikey(c, client, cfg).await,
    }
}

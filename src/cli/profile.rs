use anyhow::Result;
use clap::{Args, Subcommand};

use crate::config::AppConfig;
use crate::output;

#[derive(Subcommand, Debug)]
pub enum ProfileCommands {
    /// List all configured profiles
    List,

    /// Add or update a profile
    Add(AddArgs),

    /// Remove a profile
    Remove { name: String },

    /// Set the default profile
    Use { name: String },

    /// Show details of a profile
    Show { name: String },
}

#[derive(Args, Debug)]
pub struct AddArgs {
    /// Profile name
    pub name: String,

    /// Elasticsearch host URL
    #[arg(long)]
    pub host: String,

    /// Auth method: basic | apikey
    #[arg(long, default_value = "basic")]
    pub auth: String,

    /// Username (for basic auth)
    #[arg(long)]
    pub user: Option<String>,

    /// API key (for apikey auth)
    #[arg(long)]
    pub api_key: Option<String>,
}

pub async fn handle(cmd: ProfileCommands, cfg: &AppConfig) -> Result<()> {
    match cmd {
        ProfileCommands::List => {
            let profiles = cfg.list_profiles();
            if profiles.is_empty() {
                println!("No profiles configured. Use `esctl profile add` to create one.");
            } else {
                for (name, is_default) in profiles {
                    if is_default {
                        println!("* {} (default)", name);
                    } else {
                        println!("  {}", name);
                    }
                }
            }
        }
        ProfileCommands::Add(args) => {
            let mut cfg = cfg.clone();
            cfg.add_profile(&args)?;
            cfg.save()?;
            println!("Profile '{}' saved.", args.name);
        }
        ProfileCommands::Remove { name } => {
            let mut cfg = cfg.clone();
            cfg.remove_profile(&name)?;
            cfg.save()?;
            println!("Profile '{}' removed.", name);
        }
        ProfileCommands::Use { name } => {
            let mut cfg = cfg.clone();
            cfg.set_default_profile(&name)?;
            cfg.save()?;
            println!("Default profile set to '{}'.", name);
        }
        ProfileCommands::Show { name } => {
            let profile = cfg.get_profile(&name)?;
            let val = serde_json::to_value(&profile)?;
            output::print_json(&val, false);
        }
    }
    Ok(())
}

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "steamctl", about = "Steam game manager CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Fuzzy search and launch a game
    Launch { query: String },
    /// List all installed games
    List,
    /// List recently played games
    Recent,
    /// Show playtime for all games
    Playtime,
    /// Show info about a game
    Info { query: String },
    /// Check shader compilation status for a game
    ShaderStatus { query: String },
    /// Verify integrity of game files
    Verify { query: String },
    /// Safely clear Proton prefix registry (preserves save data)
    ResetPrefix { query: String },
    /// Delete and reinstall shader cache for a game
    ReinstallShaders { query: String },
    /// List installed Proton and GE-Proton versions
    ListProton,
    /// Set the Proton version for a game
    SetProton { query: String, tool: String },
    /// Install a game by searching the Steam store
    Install { query: String },
    /// Look up a Steam App ID by game name
    Appid { query: String },
    /// Stop a running game (fuzzy match), or all running games if no name given
    Kill { query: Option<String> },
    /// Reset prefix, reinstall shaders, and verify a game in one shot
    Fixall { query: String },
    /// Launch a randomly selected installed game
    Random,
}

mod cli;
mod commands;

use clap::Parser;
use cli::Command;
use steamctl::create_games_map;

fn main() {
    let cli = cli::Cli::parse();

    let games = match create_games_map(|g| g.installed) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error loading games: {}", e);
            std::process::exit(1);
        }
    };

    match cli.command {
        Command::List => commands::run_list(&games),
        Command::Recent => commands::run_recent(&games),
        Command::Playtime => commands::run_playtime(&games),
        Command::Launch { query } => commands::run_launch(&games, &query),
        Command::Info { query } => commands::run_info(&games, &query),
        Command::Verify { query } => commands::run_verify(&games, &query),
        Command::ShaderStatus { query } => commands::run_shader_status(&games, &query),
        Command::ResetPrefix { query } => commands::run_reset_prefix(&games, &query),
        Command::ReinstallShaders { query } => commands::run_reinstall_shaders(&games, &query),
        Command::ListProton => commands::run_list_proton(&games),
        Command::SetProton { query, tool } => commands::run_set_proton(&games, &query, &tool),
        Command::Install { query } => commands::run_install(&query),
        Command::Appid { query } => commands::run_appid(&query),
        Command::Kill { query } => commands::run_kill(&games, query.as_deref().unwrap_or("")),
        Command::Fixall { query } => commands::run_fixall(&games, &query),
        Command::Random => commands::run_random(&games),
    }
}

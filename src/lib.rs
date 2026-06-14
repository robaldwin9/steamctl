mod acf;

pub use acf::Game;
pub use acf::{proton_prefix_path, proton_registry_files, shader_cache_path,
              parse_playtimes, steam_log, compatibility_tools};

use std::collections::HashMap;
use crate::acf::{manifest_glob_for_library, parse_steam_library_paths};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

pub enum LauncherError {
    ConfigError(acf::SteamConfigParsingError),
    NoGamesFound,
    NoSteamLibraryFound,
    SteamLaunchError(String),
}

impl std::fmt::Display for LauncherError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            LauncherError::ConfigError(e) => write!(f, "Config error: {}", e),
            LauncherError::NoGamesFound => write!(f, "No games found"),
            LauncherError::NoSteamLibraryFound => write!(f, "No Steam library found"),
            LauncherError::SteamLaunchError(e) => write!(f, "Failed to launch Steam: {}", e),
        }
    }
}

impl From<acf::SteamConfigParsingError> for LauncherError {
    fn from(e: acf::SteamConfigParsingError) -> Self {
        LauncherError::ConfigError(e)
    }
}

pub fn create_games_map<F>(filter: F) -> Result<HashMap<u32, Game>, LauncherError>
where
    F: Fn(&Game) -> bool,
{
    let steam_libraries = parse_steam_library_paths()?;

    let map: HashMap<u32, Game> = steam_libraries
        .iter()
        .flat_map(|lib_path| {
            let pattern = manifest_glob_for_library(lib_path);
            glob::glob(&pattern)
                .into_iter()
                .flatten()
                .filter_map(|entry| entry.ok())
                .filter_map(|path| {
                    acf::parse_steam_game(path.to_string_lossy().into_owned()).ok()
                })
                .filter(|game| filter(game))
        })
        .map(|game| (game.appid, game))
        .collect();

    Ok(map)
}

pub fn fuzzy_search<'a>(games: &'a HashMap<u32, Game>, query: &str) -> Vec<&'a Game> {
    let matcher = SkimMatcherV2::default();

    let mut results: Vec<(&'a Game, i64)> = games
        .values()
        .filter_map(|game| {
            matcher
                .fuzzy_match(&game.name, query)
                .map(|score| (game, score))
        })
        .collect();

    results.sort_by(|a, b| b.1.cmp(&a.1));
    results.into_iter().map(|(game, _)| game).collect()
}

pub fn launch_game(appid: u32) -> Result<(), LauncherError> {
    ensure_steam_running()?;
    // Use -ifrunning to route through the existing Steam instance — avoids raising the Steam UI
    std::process::Command::new("steam")
        .arg("-ifrunning")
        .arg(format!("steam://rungameid/{}", appid))
        .env("STEAM_FRAME_FORCE_CLOSE", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| LauncherError::SteamLaunchError(e.to_string()))?;
    Ok(())
}

/// Launch via steam://run URL — works for uninstalled games (Steam may prompt to install)
pub fn launch_game_url(appid: u32) -> Result<(), LauncherError> {
    ensure_steam_running()?;
    std::process::Command::new("steam")
        .arg("-ifrunning")
        .arg(format!("steam://run/{}", appid))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| LauncherError::SteamLaunchError(e.to_string()))?;
    Ok(())
}

pub enum LaunchState {
    Pending,
    Running,
    Failed(i32),
}

/// Scan /proc for processes running from Steam library paths.
/// Returns (pid, appid) for each matched game process.
pub fn running_games(games: &HashMap<u32, Game>) -> Vec<(u32, u32)> {
    let mut results = Vec::new();
    let proc_dir = std::path::Path::new("/proc");
    let entries = match std::fs::read_dir(proc_dir) {
        Ok(e) => e,
        Err(_) => return results,
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let pid_str = entry.file_name();
        let pid: u32 = match pid_str.to_string_lossy().parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let cmdline_path = entry.path().join("cmdline");
        let cmdline = match std::fs::read(&cmdline_path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        // cmdline is NUL-separated; first arg is the executable path
        let exe = cmdline.split(|&b| b == 0).next().unwrap_or(&[]);
        let exe_str = String::from_utf8_lossy(exe);
        for game in games.values() {
            if exe_str.contains(&game.install_dir) {
                results.push((pid, game.appid));
                break;
            }
        }
    }
    results
}


pub fn launch_state(appid: u32) -> LaunchState {
    let home = std::env::var("HOME").unwrap_or_default();
    let content_log = format!("{}/.local/share/Steam/logs/content_log.txt", home);
    if let Ok(content) = std::fs::read_to_string(&content_log) {
        let tag = format!("AppID {} state changed", appid);
        if let Some(last) = content.lines().filter(|l| l.contains(&tag)).last() {
            if last.contains("App Running") {
                return LaunchState::Running;
            }
        }
    }

    // use gameprocess_log for failure detection (non-zero exit codes only)
    let proc_log = format!("{}/.local/share/Steam/logs/gameprocess_log.txt", home);
    if let Ok(content) = std::fs::read_to_string(&proc_log) {
        let tag = format!("AppID {}", appid);
        if let Some(last) = content.lines().filter(|l| l.contains(&tag)
            && l.contains("exit code")).last() {
            let code: i32 = last.split("exit code")
                .nth(1)
                .and_then(|s| s.trim().trim_end_matches('\r').parse().ok())
                .unwrap_or(0);
            if code != 0 {
                return LaunchState::Failed(code);
            }
        }
    }

    LaunchState::Pending
}

pub fn verify_game(appid: u32) -> Result<(), LauncherError> {
    std::process::Command::new("steam")
        .arg(format!("steam://validate/{}", appid))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| LauncherError::SteamLaunchError(e.to_string()))?;
    Ok(())
}

/// Convert a display name (from list-proton) to the internal name used in config.vdf.
/// GE-Proton and custom tools use their directory name as-is.
/// Official Proton uses "proton_<version>" e.g. "proton_experimental", "proton_9".
pub fn proton_internal_name(display: &str) -> String {
    if display.starts_with("Proton ") {
        let rest = &display["Proton ".len()..];
        let key = rest.split('.').next()
            .unwrap_or(rest)
            .to_lowercase()
            .replace(' ', "_");
        format!("proton_{}", key)
    } else {
        display.to_string()
    }
}

/// Read the current compat tool internal name for a given appid from config.vdf.
/// Returns None if no entry exists.
pub fn get_compat_tool(appid: u32) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let config_path = format!("{}/.local/share/Steam/config/config.vdf", home);
    let content = std::fs::read_to_string(&config_path).ok()?;

    let appid_key = format!("\"{}\"", appid);
    let compat_idx = content.lines().position(|l| l.contains("\"CompatToolMapping\""))?;
    let lines: Vec<&str> = content.lines().collect();
    let search_limit = (compat_idx + 2000).min(lines.len());

    let appid_idx = lines[compat_idx..search_limit].iter().position(|l| {
        l.trim() == appid_key.as_str()
    })?;
    let abs = compat_idx + appid_idx;

    lines[abs..abs + 10].iter()
        .find(|l| l.trim_start().starts_with("\"name\""))
        .and_then(|l| l.splitn(4, '"').nth(3))
        .map(|v| v.trim_matches('"').trim().to_string())
}

/// Write the compat tool for a given appid into config.vdf.
/// Updates an existing entry or inserts a new one.
/// Returns an error string on failure.
pub fn set_compat_tool(appid: u32, internal_name: &str) -> Result<(), String> {
    let home = std::env::var("HOME").map_err(|e| e.to_string())?;
    let config_path = format!("{}/.local/share/Steam/config/config.vdf", home);

    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Cannot read config.vdf: {}", e))?;

    // Backup before any modification
    std::fs::copy(&config_path, format!("{}.bak", config_path))
        .map_err(|e| format!("Cannot backup config.vdf: {}", e))?;

    let appid_key = format!("\"{}\"", appid);
    let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    // Find the CompatToolMapping section
    let compat_idx = lines.iter().position(|l| l.contains("\"CompatToolMapping\""))
        .ok_or("CompatToolMapping section not found in config.vdf")?;

    // Search for an existing appid entry within CompatToolMapping
    // We scan up to 2000 lines ahead to stay within the block
    let search_limit = (compat_idx + 2000).min(lines.len());
    let appid_idx = lines[compat_idx..search_limit].iter().position(|l| {
        l.trim() == appid_key.as_str()
    });

    if let Some(rel) = appid_idx {
        // Found existing entry — replace the "name" line in its block
        let abs = compat_idx + rel;
        let name_idx = lines[abs..abs + 10].iter().position(|l| {
            l.trim_start().starts_with("\"name\"")
        }).ok_or("Malformed CompatToolMapping entry: no name field")?;

        let abs_name = abs + name_idx;
        let indent: String = lines[abs_name].chars().take_while(|c| c.is_whitespace()).collect();
        lines[abs_name] = format!("{}\"name\"\t\t\"{}\"", indent, internal_name);
    } else {
        // No existing entry — insert new block before CompatToolMapping's closing brace
        let mut depth: i32 = 0;
        let mut close_idx = None;

        for (i, line) in lines[compat_idx..].iter().enumerate() {
            match line.trim() {
                "{" => depth += 1,
                "}" if depth == 1 => { close_idx = Some(compat_idx + i); break; }
                "}" => depth -= 1,
                _ => {}
            }
        }

        let ci = close_idx.ok_or("Could not find closing brace for CompatToolMapping")?;

        // Infer indentation from an existing entry or fall back to 5 tabs
        let entry_indent: String = lines[compat_idx..ci].iter()
            .find(|l| {
                let t = l.trim();
                t.starts_with('"') && t.ends_with('"') && t[1..t.len()-1].parse::<u32>().is_ok()
            })
            .map(|l| l.chars().take_while(|c| c.is_whitespace()).collect())
            .unwrap_or_else(|| "\t\t\t\t\t".to_string());

        let field_indent = format!("{}\t", entry_indent);

        let new_block = vec![
            format!("{}\"{}\"", entry_indent, appid),
            format!("{}{{", entry_indent),
            format!("{}\"name\"\t\t\"{}\"", field_indent, internal_name),
            format!("{}\"config\"\t\t\"\"", field_indent),
            format!("{}\"priority\"\t\t\"250\"", field_indent),
            format!("{}}}", entry_indent),
        ];

        for (j, block_line) in new_block.into_iter().enumerate() {
            lines.insert(ci + j, block_line);
        }
    }

    std::fs::write(&config_path, lines.join("\n"))
        .map_err(|e| format!("Cannot write config.vdf: {}", e))?;

    Ok(())
}

pub fn is_steam_running() -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else { return false };
    entries.filter_map(|e| e.ok()).any(|entry| {
        std::fs::read_to_string(entry.path().join("comm"))
            .map(|s| s.trim() == "steam")
            .unwrap_or(false)
    })
}

fn spawn_steam_silent() -> Result<std::process::Child, LauncherError> {
    std::process::Command::new("steam")
        .arg("-silent")
        .arg("-no-browser")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| LauncherError::SteamLaunchError(e.to_string()))
}

/// Ensures Steam is running. If we spawn it, we use the Child handle directly —
/// no pgrep needed. A short grace period allows Steam's IPC to initialize.
fn ensure_steam_running() -> Result<(), LauncherError> {
    if !is_steam_running() {
        let mut child = spawn_steam_silent()?;
        // Confirm the process is still alive after a brief moment
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Ok(Some(status)) = child.try_wait() {
            return Err(LauncherError::SteamLaunchError(
                format!("Steam exited immediately ({})", status)
            ));
        }
        // Process is alive; brief pause for Steam's IPC to become ready
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    Ok(())
}

/// Spawns Steam silently in the background without waiting.
pub fn start_steam() -> Result<(), LauncherError> {
    spawn_steam_silent()?; // Child dropped — Steam runs independently
    Ok(())
}

/// Shuts down Steam and waits up to 15 seconds for it to exit.
/// Returns `true` if Steam stopped, `false` if it timed out.
/// Returns `true` immediately if Steam was not running.
pub fn stop_steam() -> bool {
    if !is_steam_running() {
        return true;
    }
    let _ = std::process::Command::new("steam").arg("-shutdown").output();
    let deadline = std::time::Instant::now();
    while is_steam_running() {
        if deadline.elapsed() > std::time::Duration::from_secs(15) {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    true
}

pub fn install_game(appid: u32) -> Result<(), LauncherError> {
    ensure_steam_running()?;
    // -ifrunning routes the URL to the already-running Steam instance
    std::process::Command::new("steam")
        .arg("-ifrunning")
        .arg(format!("steam://install/{}", appid))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| LauncherError::SteamLaunchError(e.to_string()))?;
    Ok(())
}

/// Search the Steam store by name. Returns up to 5 (appid, name) results.
pub fn store_search(query: &str) -> Vec<(u32, String)> {
    let body = match ureq::get("https://store.steampowered.com/api/storesearch/")
        .query("term", query)
        .query("l", "english")
        .query("cc", "us")
        .timeout(std::time::Duration::from_secs(8))
        .call()
    {
        Ok(response) => response.into_string().unwrap_or_default(),
        Err(_) => return vec![],
    };

    let json: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    json["items"]
        .as_array()
        .map(|items| {
            items.iter()
                .filter(|item| item["type"].as_str() == Some("app"))
                .filter_map(|item| {
                    let id = item["id"].as_u64()? as u32;
                    let name = item["name"].as_str()?.to_string();
                    Some((id, name))
                })
                .take(5)
                .collect()
        })
        .unwrap_or_default()
}

/// Returns (percent, done, total) if shaders are currently compiling for appid
pub fn shader_status(appid: u32) -> Option<(u32, u32, u32)> {
    let home = std::env::var("HOME").unwrap_or_default();
    let log_path = format!("{}/.local/share/Steam/logs/shader_log.txt", home);
    let content = std::fs::read_to_string(log_path).ok()?;

    // find the last "Still replaying <appid>" line
    let tag = format!("Still replaying {}", appid);
    let line = content.lines()
        .filter(|l| l.contains(&tag))
        .last()?
        .trim_end_matches('\r')
        .to_string();

    // parse: "[timestamp] Still replaying 3321460 (75%, 17355/67209)."
    // find the '(' after the appid, not the one in the timestamp
    let tag_pos = line.find(&tag)?;
    let after_tag = &line[tag_pos + tag.len()..];
    let paren = after_tag.find('(')?;
    let inner = &after_tag[paren + 1..];

    let pct: u32 = inner.split('%').next()?.trim().parse().ok()?;
    let slash_part = inner.split(',').nth(1)?;
    let mut parts = slash_part.trim().split('/');
    let done: u32 = parts.next()?.trim().parse().ok()?;
    let total: u32 = parts.next()?.trim_matches(')').trim().trim_matches('.')
        .parse().ok()?;

    Some((pct, done, total))
}


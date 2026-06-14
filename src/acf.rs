use std::fs;
use std::collections::HashMap;

const STEAM_RELATIVE_PATH: &str = ".local/share/Steam";

pub fn steam_root() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    format!("{}/{}", home, STEAM_RELATIVE_PATH)
}

pub fn steam_library_vdf_path() -> String {
    library_folders_vdf(&steam_root())
}

pub enum SteamConfigParsingError {
    FileNotFound(String),
    MissingField(String),
    ExpectedU32Field(String),
    IoError(std::io::Error)
}

impl std::fmt::Display for SteamConfigParsingError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SteamConfigParsingError::FileNotFound(p) => write!(f, "File not found: {}", p),
            SteamConfigParsingError::MissingField(s) => write!(f, "Missing field: {}", s),
            SteamConfigParsingError::ExpectedU32Field(s) => write!(f, "Expected u32: {}", s),
            SteamConfigParsingError::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl From<std::io::Error> for SteamConfigParsingError {
    fn from(e: std::io::Error) -> Self {
        SteamConfigParsingError::IoError(e)
    }
}

pub struct Game {
    pub appid: u32,
    pub name: String,
    pub installed: bool,
    pub install_dir: String,
    pub last_updated: u64,      // unix timestamp
    pub last_played: u64,       // unix timestamp
    pub size_on_disk: u64,      // bytes
    pub build_id: String,
    pub bytes_to_download: u64,
    pub bytes_downloaded: u64,
    pub auto_update_behavior: u32,
}

pub fn parse_steam_game(path : String) -> Result<Game, SteamConfigParsingError> {
    let content = fs::read_to_string(&path)
        .map_err(|_| SteamConfigParsingError::FileNotFound(path))?;

    let appid = extract_field(&content, "appid")
        .ok_or_else(|| SteamConfigParsingError::MissingField("appid".into()))?
        .parse::<u32>()
        .map_err(|e| SteamConfigParsingError::ExpectedU32Field(e.to_string()))?;

    let name = extract_field(&content, "name")
        .ok_or_else(|| SteamConfigParsingError::MissingField("name".into()))?;

    let state_flags = extract_field(&content, "StateFlags")
        .ok_or_else(|| SteamConfigParsingError::MissingField("StateFlags".into()))?;

    let installed: bool = state_flags == "4";

    let install_dir = extract_field(&content, "installdir").unwrap_or_default();
    let build_id = extract_field(&content, "buildid").unwrap_or_default();

    let last_updated = extract_field(&content, "LastUpdated")
        .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let last_played = extract_field(&content, "LastPlayed")
        .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let size_on_disk = extract_field(&content, "SizeOnDisk")
        .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let bytes_to_download = extract_field(&content, "BytesToDownload")
        .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let bytes_downloaded = extract_field(&content, "BytesDownloaded")
        .and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let auto_update_behavior = extract_field(&content, "AutoUpdateBehavior")
        .and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);

    Ok(Game {
        appid,
        name,
        installed,
        install_dir,
        build_id,
        last_updated,
        last_played,
        size_on_disk,
        bytes_to_download,
        bytes_downloaded,
        auto_update_behavior,
    })
}

pub fn manifest_glob_for_library(lib_path: & str) -> String {
    format!("{}/steamapps/appmanifest_*.acf", lib_path)
}

pub fn steam_log(root_path: &str) -> String {
    format!("{}/.local/share/Steam/logs/content_log.txt", root_path)
}

pub fn library_folders_vdf(steam_root: &str) -> String {
    format!("{}/steamapps/libraryfolders.vdf", steam_root)
}

pub fn compatibility_tools(root_path: &str) -> String  {
    format!("{}/.local/share/Steam/compatibilitytools.d", root_path)
}

pub fn proton_prefix_path(appid: u32) -> Option<String> {
    // search all library paths for the compatdata directory
    let paths = parse_steam_library_paths().unwrap_or_default();
    for lib_path in paths {
        let pfx = format!("{}/steamapps/compatdata/{}/pfx", lib_path, appid);
        if std::path::Path::new(&pfx).exists() {
            return Some(pfx);
        }
    }
    None
}

pub fn proton_registry_files(appid: u32) -> Vec<String> {
    match proton_prefix_path(appid) {
        None => vec![],
        Some(pfx) => vec![
            format!("{}/system.reg", pfx),
            format!("{}/user.reg", pfx),
            format!("{}/userdef.reg", pfx),
        ],
    }
}

pub fn shader_cache_path(appid: u32) -> Option<String> {
    let paths = parse_steam_library_paths().unwrap_or_default();
    for lib_path in paths {
        let cache = format!("{}/steamapps/shadercache/{}", lib_path, appid);
        if std::path::Path::new(&cache).exists() {
            return Some(cache);
        }
    }
    None
}

pub fn extract_field(content: &str, key: &str) -> Option<String> {
    let key_str = format!("\"{}\"", key);
    content
        .lines()
        .find(|line| line.contains(&key_str))
        .and_then(|line| line.splitn(4, '"').nth(3))
        .map(|value| value.trim_matches('"').trim().to_string())
}

pub fn extract_all_fields(content: &str, key: &str) -> Vec<String> {
    let key_str = format!("\"{}\"", key);
    content
        .lines()
        .filter(|line| line.contains(&key_str))
        .filter_map(|line| line.splitn(4, '"').nth(3))
        .map(|value| value.trim_matches('"').trim().to_string())
        .collect()
}

/// Parse localconfig.vdf and return a map of appid → playtime in minutes.
pub fn parse_playtimes() -> HashMap<u32, u32> {
    let mut map = HashMap::new();
    let root = steam_root();
    let userdata = format!("{}/userdata", root);
    let entries = match fs::read_dir(&userdata) {
        Ok(e) => e,
        Err(_) => return map,
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let cfg = entry.path().join("config/localconfig.vdf");
        let content = match fs::read_to_string(&cfg) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // Walk lines: when we see a bare quoted integer key (no value on same line), track it as current appid
        let mut current_appid: Option<u32> = None;
        for line in content.lines() {
            let trimmed = line.trim();
            // Bare appid key: exactly `"<number>"` with no tab-separated value
            let quote_count = trimmed.chars().filter(|&c| c == '"').count();
            if quote_count == 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
                let inner = &trimmed[1..trimmed.len() - 1];
                current_appid = inner.parse::<u32>().ok();
            } else if trimmed.starts_with("\"Playtime\"") {
                if let Some(appid) = current_appid {
                    if let Some(mins) = trimmed.splitn(4, '"').nth(3)
                        .and_then(|s| s.trim_matches('"').trim().parse::<u32>().ok())
                    {
                        map.entry(appid).or_insert(mins);
                    }
                }
            }
        }
    }
    map
}

pub fn parse_steam_library_paths() -> Result<Vec<String>, SteamConfigParsingError> {
    let vdf_path = steam_library_vdf_path();

    let content = fs::read_to_string(&vdf_path)
        .map_err(|_| SteamConfigParsingError::FileNotFound(vdf_path))?;

    let paths = extract_all_fields(&content, "path");

    if paths.is_empty() {
        return Err(SteamConfigParsingError::MissingField("No library paths found".into()));
    }

    Ok(paths)
}

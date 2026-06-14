use std::collections::HashMap;
use fuzzy_matcher::FuzzyMatcher;
use steamctl::Game;
use steamctl::fuzzy_search;
use steamctl::compatibility_tools;
use steamctl::launch_game;
use steamctl::{proton_registry_files, proton_prefix_path, shader_cache_path};
use steamctl::steam_log;
use steamctl::shader_status;
use steamctl::{LaunchState, launch_state};
use steamctl::verify_game;
use steamctl::{proton_internal_name, set_compat_tool, get_compat_tool};


/// Print a y/N prompt and return `true` only if the user types "y".
/// Returns `false` on EOF or I/O error (treats them as cancel).
fn prompt_confirm(msg: &str) -> bool {
    use std::io::Write;
    print!("{}", msg);
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).unwrap_or(0) == 0 {
        println!();
        return false;
    }
    input.trim().to_lowercase() == "y"
}

pub fn run_list(games: &HashMap<u32, Game>) {
    let mut names: Vec<&str> = games.values().map(|g| g.name.as_str()).collect();
    names.sort();
    for name in names {
        println!("{}", name);
    }
}

pub fn run_recent(games: &HashMap<u32, Game>) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut played: Vec<&Game> = games.values()
        .filter(|g| g.last_played > 0)
        .collect();
    played.sort_by(|a, b| b.last_played.cmp(&a.last_played));

    if played.is_empty() {
        println!("No recently played games found.");
        return;
    }
    for game in played.iter().take(20) {
        let secs = now.saturating_sub(game.last_played);
        let ago = fmt_duration(secs);
        println!("{:<50} {}", game.name, ago);
    }
}

pub fn run_playtime(games: &HashMap<u32, Game>) {
    let playtimes = steamctl::parse_playtimes();

    let mut entries: Vec<(&Game, u32)> = games.values()
        .filter_map(|g| {
            let mins = *playtimes.get(&g.appid).unwrap_or(&0);
            if mins > 0 { Some((g, mins)) } else { None }
        })
        .collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1));

    if entries.is_empty() {
        println!("No playtime data found.");
        return;
    }
    for (game, mins) in &entries {
        let hrs = *mins as f64 / 60.0;
        println!("{:<50} {:.1}h", game.name, hrs);
    }
}

fn fmt_duration(secs: u64) -> String {
    if secs < 3600 {
        format!("{} min ago", secs / 60)
    } else if secs < 86400 {
        format!("{} hr ago", secs / 3600)
    } else if secs < 86400 * 7 {
        format!("{} days ago", secs / 86400)
    } else if secs < 86400 * 30 {
        format!("{} weeks ago", secs / (86400 * 7))
    } else if secs < 86400 * 365 {
        format!("{} months ago", secs / (86400 * 30))
    } else {
        format!("{} years ago", secs / (86400 * 365))
    }
}

pub fn run_launch(games: &HashMap<u32, Game>, query: &str) {
    let results = fuzzy_search(games, query);
    match results.len() {
        0 => {
            eprintln!("No installed games found for '{}'. Try: steamctl install \"{}\"", query,
                      query);
            std::process::exit(1);
        }
        1 => {
            let game = results[0];
            println!("Launching {}...", game.name);
            if let Err(e) = launch_game(game.appid) {
                eprintln!("{}", e);
                std::process::exit(1);
            }
            monitor_launch(game.appid, &game.name);
        }
        _ => {
            println!("Multiple matches for '{}':", query);
            for (i, game) in results.iter().take(5).enumerate() {
                println!("  {}: {}", i + 1, game.name);
            }
            println!("Be more specific to launch directly.");
        }
    }
}

fn monitor_launch(appid: u32, name: &str) {
    use std::time::{Duration, Instant};
    let timeout = Instant::now();
    let mut last_shader_pct = 0u32;

    loop {
        std::thread::sleep(Duration::from_secs(2));

        match launch_state(appid) {
            LaunchState::Running => {
                println!("{} is running!", name);
                close_steam_window();
                return;
            }
            LaunchState::Failed(code) => {
                eprintln!("Launch failed (exit code {}). Try running from Steam GUI to diagnose.",
                          code);
                return;
            }
            LaunchState::Pending => {
                if let Some((pct, done, total)) = shader_status(appid) {
                    if pct != last_shader_pct {
                        println!("Shaders: {}% ({}/{})", pct, done, total);
                        last_shader_pct = pct;
                    }
                }
            }
        }

        if timeout.elapsed() > Duration::from_secs(60) {
            println!("Game launched — timed out waiting for confirmation.");
            close_steam_window();
            return;
        }
    }
}

fn close_steam_window() {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    // steamwebhelper is the CEF process that renders Steam's UI windows.
    // Killing it closes the window while leaving Steam and the game running.
    let Ok(entries) = std::fs::read_dir("/proc") else { return };
    let pids: Vec<i32> = entries.filter_map(|e| e.ok()).filter_map(|entry| {
        let pid: i32 = entry.file_name().to_string_lossy().parse().ok()?;
        let comm = std::fs::read_to_string(entry.path().join("comm")).ok()?;
        if comm.trim() == "steamwebhelper" { Some(pid) } else { None }
    }).collect();
    for pid in pids {
        let _ = kill(Pid::from_raw(pid), Signal::SIGTERM);
    }
}

pub fn run_random(games: &HashMap<u32, Game>) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let games_vec: Vec<&Game> = games.values().filter(|g| {
        let n = g.name.as_str();
        !n.starts_with("Proton")
            && !n.starts_with("Steam Linux Runtime")
            && !n.starts_with("Steamworks")
    }).collect();
    if games_vec.is_empty() {
        eprintln!("No installed games found.");
        return;
    }
    let seed = SystemTime::now().duration_since(UNIX_EPOCH)
        .unwrap_or_default().subsec_nanos() as usize;
    let game = games_vec[seed % games_vec.len()];
    println!("🎲 Random pick: {}", game.name);
    if let Err(e) = launch_game(game.appid) {
        eprintln!("{}", e);
        std::process::exit(1);
    }
    monitor_launch(game.appid, &game.name);
}


pub fn run_verify(games: &HashMap<u32, Game>, query: &str) {
    let results = fuzzy_search(games, query);
    match results.first() {
        None => eprintln!("No games found for '{}'", query),
        Some(game) => {
            println!("Verifying integrity of {}...", game.name);
            // Capture log offset BEFORE triggering verify to avoid the race where
            // Steam writes "Start validating" before we record start_offset
            let home = std::env::var("HOME").unwrap_or_default();
            let log_path = steam_log(&home);
            let start_offset = std::fs::metadata(&log_path).map(|m| m.len())
                .unwrap_or(0);
            if let Err(e) = verify_game(game.appid) {
                eprintln!("{}", e);
                return;
            }
            monitor_verify(game.appid, &game.name, start_offset);
        }
    }
}

fn monitor_verify(appid: u32, name: &str, start_offset: u64) {
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::time::{Duration, Instant};
    use std::fs::File;

    let home = std::env::var("HOME").unwrap_or_default();
    let log_path = steam_log(&home);
    let tag = format!("AppID {}", appid);
    let timeout = Instant::now();
    let mut last_status = String::new();

    loop {
        std::thread::sleep(Duration::from_secs(2));

        let new_content = match File::open(&log_path) {
            Ok(mut f) => {
                let _ = f.seek(SeekFrom::Start(start_offset));
                let mut buf = String::new();
                let _ = f.read_to_string(&mut buf);
                buf
            }
            Err(_) => continue,
        };

        // Only consider lines for this appid written after we started
        let relevant: Vec<&str> = new_content
            .lines()
            .filter(|l| l.contains(&tag))
            .map(|l| l.trim_end_matches('\r'))
            .collect();

        let last = match relevant.last() {
            Some(l) => *l,
            None => continue,
        };

        // Check for completion/failure via scheduler line
        if last.contains("scheduler finished") && last.contains("result") {
            if last.contains("No Error") {
                println!("\r{}: Verification complete!              ", name);
            } else {
                // Extract result string
                let result = last.split("result ").nth(1)
                    .unwrap_or("unknown error");
                println!("\r{}: Verification failed: {}     ", name, result);
            }
            return;
        }

        // Check for files that failed validation
        if let Some(fail_line) = relevant.iter().rev().find(|l| l
            .contains("files failed validation")) {
            let detail = fail_line.split("] ").nth(1).unwrap_or(fail_line);
            let status = format!("Issues found: {}", detail);
            if status != last_status {
                println!("\r  {}", status);
                last_status = status;
            }
        }

        // Parse current phase from "App update changed"
        let phase = relevant
            .iter()
            .rev()
            .find(|l| l.contains("App update changed"))
            .and_then(|l| l.split(" : ").nth(1))
            .map(|s| s.trim_end_matches(',').replace(",", " → "))
            .unwrap_or_else(|| "Starting...".to_string());

        let status = if phase.contains("None") {
            // "App update changed : None" — done
            println!("\r{}: Verification complete!              ", name);
            return;
        } else if phase.contains("Verifying Installed") {
            "Verifying files...".to_string()
        } else if phase.contains("Reconfiguring") {
            "Reconfiguring...".to_string()
        } else if phase.contains("Downloading") {
            // Try to extract byte progress from "update started" line
            let progress = relevant
                .iter()
                .rev()
                .find(|l| l.contains("update started"))
                .and_then(|l| {
                    let dl = l.split("download ").nth(1)?;
                    let parts: Vec<&str> = dl.split('/').collect();
                    if parts.len() >= 2 {
                        let done: u64 = parts[0].trim().parse().ok()?;
                        let total: u64 = parts[1].split(',').next()?.trim().parse().ok()?;
                        if total > 0 {
                            Some(format!("{:.1} / {:.1} MB", done as f64 / 1_048_576.0, total as f64
                                / 1_048_576.0))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .unwrap_or_default();
            format!("Downloading fixes... {}", progress)
        } else if phase.contains("Committing") {
            "Committing changes...".to_string()
        } else if phase.contains("Preallocating") {
            "Preallocating...".to_string()
        } else {
            format!("{}...", phase)
        };

        if status != last_status {
            print!("\r  {:<60}", status);
            let _ = std::io::stdout().flush();
            last_status = status;
        }

        if timeout.elapsed() > Duration::from_secs(300) {
            println!("\nTimed out waiting for verification.");
            return;
        }
    }
}

pub fn run_info(games: &HashMap<u32, Game>, query: &str) {
    let results = fuzzy_search(games, query);
    match results.first() {
        None => eprintln!("No games found for '{}'", query),
        Some(game) => {
            let size_gb = game.size_on_disk as f64 / 1_073_741_824.0;
            let last_played = if game.last_played == 0 {
                "Never".to_string()
            } else {
                format_timestamp(game.last_played)
            };
            let last_updated = if game.last_updated == 0 {
                "Unknown".to_string()
            } else {
                format_timestamp(game.last_updated)
            };
            let downloading = if game.bytes_to_download > 0 {
                format!("{:.1} MB remaining", game.bytes_to_download as f64 / 1_048_576.0)
            } else {
                "Up to date".to_string()
            };

            println!("Name:         {}", game.name);
            println!("App ID:       {}", game.appid);
            println!("Installed:    {}", game.installed);
            println!("Install Dir:  {}", game.install_dir);
            println!("Size:         {:.1} GB", size_gb);
            println!("Build ID:     {}", game.build_id);
            println!("Last Played:  {}", last_played);
            println!("Last Updated: {}", last_updated);
            println!("Download:     {}", downloading);
        }
    }
}

pub fn run_shader_status(games: &HashMap<u32, Game>, query: &str) {
    let results = fuzzy_search(games, query);
    let game = match results.first() {
        None => { eprintln!("No games found for '{}'", query); return; }
        Some(g) => g,
    };

    match shader_status(game.appid) {
        None => println!("{}: No shader compilation in progress.", game.name),
        Some((pct, done, total)) => {
            println!("{}: Shaders compiling... {}% ({}/{})", game.name, pct, done, total);
        }
    }
}

pub fn run_reset_prefix(games: &HashMap<u32, Game>, query: &str) {
    let results = fuzzy_search(games, query);
    let game = match results.first() {
        None => { eprintln!("No games found for '{}'", query); return; }
        Some(g) => g,
    };

    let pfx = match proton_prefix_path(game.appid) {
        None => {
            eprintln!("No Proton prefix found for '{}' (may be a native Linux game)", game.name);
            return;
        }
        Some(p) => p,
    };

    println!("Safely resetting Proton prefix for: {}", game.name);
    println!("Prefix: {}", pfx);
    println!("This will delete registry files only — save data in drive_c/ is preserved.");
    if !prompt_confirm("Continue? [y/N] ") {
        println!("Cancelled.");
        return;
    }

    let mut deleted = 0;
    for reg_file in proton_registry_files(game.appid) {
        if std::path::Path::new(&reg_file).exists() {
            match std::fs::remove_file(&reg_file) {
                Ok(_) => { println!("Deleted: {}", reg_file); deleted += 1; }
                Err(e) => eprintln!("Failed to delete {}: {}", reg_file, e),
            }
        }
    }
    println!("Done — {} registry file(s) removed. Launch the game to rebuild the prefix.",
             deleted);
}

pub fn run_reinstall_shaders(games: &HashMap<u32, Game>, query: &str) {
    let results = fuzzy_search(games, query);
    let game = match results.first() {
        None => { eprintln!("No games found for '{}'", query); return; }
        Some(g) => g,
    };

    let cache_path = match shader_cache_path(game.appid) {
        None => {
            eprintln!("No shader cache found for '{}' — nothing to clear.", game.name);
            return;
        }
        Some(p) => p,
    };

    // Calculate total size
    let size_mb = dir_size_mb(&cache_path);
    println!("Shader cache for {}: {} ({:.0} MB)", game.name, cache_path, size_mb);
    println!("This will delete all compiled shaders. Steam will redownload them on next launch.");
    if !prompt_confirm("Continue? [y/N] ") {
        println!("Cancelled.");
        return;
    }

    match std::fs::remove_dir_all(&cache_path) {
        Ok(_) => println!("Shader cache cleared. Launch {} to redownload shaders.", game.name),
        Err(e) => eprintln!("Failed to clear shader cache: {}", e),
    }
}

pub fn run_fixall(games: &HashMap<u32, Game>, query: &str) {
    let results = fuzzy_search(games, query);
    let game = match results.first() {
        None => { eprintln!("No games found for '{}'", query); return; }
        Some(g) => g,
    };

    println!("Fix all for: {}", game.name);
    println!("  1. Reset Proton prefix (registry only, saves preserved)");
    println!("  2. Reinstall shader cache");
    println!("  3. Verify game files");
    if !prompt_confirm("Continue? [y/N] ") {
        println!("Cancelled.");
        return;
    }

    // 1. Reset prefix
    println!("\n── Reset Proton prefix ──");
    match proton_prefix_path(game.appid) {
        None => println!("No Proton prefix found (may be a native Linux game) — skipping."),
        Some(_) => {
            let mut deleted = 0;
            for reg_file in proton_registry_files(game.appid) {
                if std::path::Path::new(&reg_file).exists() {
                    match std::fs::remove_file(&reg_file) {
                        Ok(_) => { println!("Deleted: {}", reg_file); deleted += 1; }
                        Err(e) => eprintln!("Failed: {}", e),
                    }
                }
            }
            println!("{} registry file(s) removed.", deleted);
        }
    }

    // 2. Reinstall shaders
    println!("\n── Reinstall shader cache ──");
    match shader_cache_path(game.appid) {
        None => println!("No shader cache found — skipping."),
        Some(cache_path) => {
            let size_mb = dir_size_mb(&cache_path);
            println!("Clearing {:.0} MB shader cache...", size_mb);
            match std::fs::remove_dir_all(&cache_path) {
                Ok(_) => println!("Shader cache cleared."),
                Err(e) => eprintln!("Failed: {}", e),
            }
        }
    }

    // 3. Verify
    println!("\n── Verify game files ──");
    let home = std::env::var("HOME").unwrap_or_default();
    let log_path = steam_log(&home);
    let start_offset = std::fs::metadata(&log_path).map(|m| m.len())
        .unwrap_or(0);
    if let Err(e) = steamctl::verify_game(game.appid) {
        eprintln!("Failed to start verify: {}", e);
        return;
    }
    monitor_verify(game.appid, &game.name, start_offset);
}


fn dir_size_mb(path: &str) -> f64 {
    fn walk(path: &std::path::Path) -> u64 {
        match std::fs::read_dir(path) {
            Err(_) => 0,
            Ok(entries) => entries.filter_map(|e| e.ok()).map(|e| {
                let p = e.path();
                if p.is_dir() { walk(&p) } else { e.metadata().map(|m| m.len())
                    .unwrap_or(0) }
            }).sum(),
        }
    }
    walk(std::path::Path::new(path)) as f64 / 1_048_576.0
}

pub fn run_kill(games: &HashMap<u32, Game>, query: &str) {
    let running = steamctl::running_games(games);
    if running.is_empty() {
        println!("No running games detected.");
        return;
    }

    // Filter by query if provided, otherwise kill all
    let targets: Vec<(u32, u32)> = if query.is_empty() {
        running.clone()
    } else {
        let matched = fuzzy_search(games, query);
        let matched_ids: std::collections::HashSet<u32> = matched.iter().map(|g| g.appid)
            .collect();
        running.into_iter().filter(|(_, appid)| matched_ids.contains(appid)).collect()
    };

    if targets.is_empty() {
        println!("No running games matched '{}'.", query);
        return;
    }

    for (pid, appid) in targets {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        let name = games.get(&appid).map(|g| g.name.as_str())
            .unwrap_or("unknown");
        match kill(Pid::from_raw(pid as i32), Signal::SIGTERM) {
            Ok(_) => println!("Stopped {} (PID {})", name, pid),
            Err(_) => eprintln!("Failed to stop {} (PID {})", name, pid),
        }
    }
}


pub fn run_install(query: &str) {
    let results = steamctl::store_search(query);
    if results.is_empty() {
        eprintln!("No results found for '{}'.", query);
        return;
    }

    let (appid, name) = if results.len() == 1 {
        println!("Found: {} ({})", results[0].1, results[0].0);
        (results[0].0, results[0].1.clone())
    } else {
        println!("Results for '{}':", query);
        for (i, (id, name)) in results.iter().enumerate() {
            println!("  {}: {} (AppID: {})", i + 1, name, id);
        }
        use std::io::Write;
        print!("Select [1-{}]: ", results.len());
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).unwrap_or(0) == 0 {
            println!("\nCancelled.");
            return;
        }
        let pick: usize = input.trim().parse().unwrap_or(0);
        if pick == 0 || pick > results.len() {
            println!("Cancelled.");
            return;
        }
        let (id, name) = &results[pick - 1];
        println!("Selected: {} ({})", name, id);
        (*id, name.clone())
    };

    println!("Sending {} to Steam... (confirm install in Steam window)", name);
    if let Err(e) = steamctl::launch_game_url(appid) {
        eprintln!("{}", e);
    }
}

pub fn run_appid(query: &str) {
    let results = steamctl::store_search(query);
    if results.is_empty() {
        eprintln!("No results found for '{}'.", query);
        return;
    }
    if results.len() == 1 {
        println!("{:<50} AppID: {}", results[0].1, results[0].0);
        return;
    }
    println!("Results for '{}':", query);
    for (id, name) in &results {
        println!("  {:<50} AppID: {}", name, id);
    }
}

pub fn run_set_proton(games: &HashMap<u32, Game>, query: &str, tool: &str) {
    let results = fuzzy_search(games, query);
    let game = match results.first() {
        None => { eprintln!("No games found for '{}'", query); return; }
        Some(g) => g,
    };

    // Build full list of available tool display names
    let home = std::env::var("HOME").unwrap_or_default();
    let mut all_tools: Vec<String> = games.values()
        .filter(|g| g.name.starts_with("Proton") && !g.name.contains("Runtime"))
        .map(|g| g.name.clone())
        .collect();
    let compat_dir = compatibility_tools(&home);
    if let Ok(entries) = std::fs::read_dir(&compat_dir) {
        all_tools.extend(
            entries.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().to_string())
        );
    }

    let matcher = fuzzy_matcher::skim::SkimMatcherV2::default();
    let normalize = |s: &str| s.replace('-', "").to_lowercase();
    let query_norm = normalize(tool);
    let matched = all_tools.iter()
        .filter_map(|t| {
            let t_norm = normalize(t);
            matcher.fuzzy_match(&t_norm, &query_norm).map(|s| (s, t))
        })
        .max_by_key(|(s, _)| *s)
        .map(|(_, t)| t.clone());

    let display_name = match matched {
        Some(n) => n,
        None => {
            eprintln!("No Proton version found matching '{}'. Run `list-proton` to see options.",
                      tool);
            return;
        }
    };

    let internal = proton_internal_name(&display_name);

    // Check if already set to avoid unnecessary Steam restart
    if let Some(current) = get_compat_tool(game.appid) {
        if current == internal {
            println!("{} is already using {}.", game.name, display_name);
            return;
        }
    }

    println!("{}: setting Proton → {}", game.name, display_name);

    // Steam holds config.vdf in memory and overwrites external edits.
    // Shut it down, write the change, then restart silently.
    use std::io::Write;
    let was_running = steamctl::is_steam_running();
    if was_running {
        print!("Stopping Steam...");
        let _ = std::io::stdout().flush();
        if !steamctl::stop_steam() {
            eprintln!("\nSteam didn't stop in time — aborting to avoid data loss.");
            return;
        }
        println!(" stopped.");
    }

    match set_compat_tool(game.appid, &internal) {
        Ok(_) => {
            println!("Done.");
            if was_running {
                print!("Restarting Steam...");
                let _ = std::io::stdout().flush();
                let _ = steamctl::start_steam();
                println!(" started.");
            }
        }
        Err(e) => {
            eprintln!("Failed to write config: {}", e);
            if was_running {
                // Restart Steam even on failure so the user isn't left without it
                let _ = steamctl::start_steam();
            }
        }
    }
}

pub fn run_list_proton(games: &HashMap<u32, Game>) {
    let home = std::env::var("HOME").unwrap_or_default();

    // Official Proton from ACF manifests — already loaded in games map
    let mut official: Vec<&str> = games.values()
        .filter(|g| g.name.starts_with("Proton") && !g.name.contains("Runtime"))
        .map(|g| g.name.as_str())
        .collect();
    official.sort();

    // GE-Proton and custom tools from compatibilitytools.d
    let compat_dir = compatibility_tools(&home);
    let mut custom: Vec<String> = std::fs::read_dir(&compat_dir)
        .map(|entries| {
            entries.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    custom.sort();

    println!("Official Proton:");
    for name in &official {
        println!("  {}", name);
    }

    if !custom.is_empty() {
        println!("\nCustom / GE-Proton:");
        for name in &custom {
            println!("  {}", name);
        }
    }
}

fn format_timestamp(ts: u64) -> String {
    let days = ts / 86400;
    let years = 1970 + days / 365;
    let remaining_days = days % 365;
    let months = remaining_days / 30 + 1;
    let day = remaining_days % 30 + 1;
    format!("{}-{:02}-{:02}", years, months, day)
}

# Steamctl
utility for controlling steam via CLI. Allowws user to carry out common actions like launching a game without keeping 
the steam UI process open.

# OS Details
* Linux os with steam installed via standard package manager.
* Mac is a maybe, do not have one to test on
* First version does not support flatpack version of steam

# Usage
```Steam game manager CLI
Usage: steamctl <COMMAND>

Commands:
  launch             Fuzzy search and launch a game
  list               List all installed games
  recent             List recently played games
  playtime           Show playtime for all games
  info               Show info about a game
  shader-status      Check shader compilation status for a game
  verify             Verify integrity of game files
  reset-prefix       Safely clear Proton prefix registry (preserves save data)
  reinstall-shaders  Delete and reinstall shader cache for a game
  list-proton        List installed Proton and GE-Proton versions
  set-proton         Set the Proton version for a game
  install            Install a game by searching the Steam store
  appid              Look up a Steam App ID by game name
  kill               Stop a running game (fuzzy match), or all running games if no name given
  fixall             Reset prefix, reinstall shaders, and verify a game in one shot
  random             Launch a randomly selected installed game
  help               Print this message or the help of the given subcommand(s)

Options:
  -h, --help  Print help
  ```

## AI Usage
Copilot CLI was used in creation of this application. My rust is a bit rusty so its helpful as an advisory role,
and for quick prototyping.


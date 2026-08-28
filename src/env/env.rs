// src/env/env.rs — service overlay. Edit defaults here; regenerate generated.rs from flags-2-env.

use super::generated;

/// Code-level defaults. Overlay values from flags-2-env (`.env` vs process env vs argv) win.
pub fn defaults() -> std::collections::BTreeMap<String, String> {
    std::collections::BTreeMap::from([
        ("OPTO_SYNC_WEB_BIND".to_string(), "127.0.0.1:8081".to_string()),
    ])
}

/// Merge service defaults under the flags-2-env overlay.
/// Default rank: argv `flags` > `env_shell` > `env_file` (`.env`).
/// `dotenv_override` / `[env] override` lifts `.env` over the process environment.
/// Servers should set `[env] load = false` so a hostile CWD `.env` cannot inject values.
pub fn load() -> Result<std::collections::BTreeMap<String, String>, generated::MissingEnv> {
    let mut merged = defaults();
    merged.extend(generated::load_env_map_from_os()?);
    Ok(merged)
}

pub fn get<'a>(env: &'a std::collections::BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    env.get(key).map(String::as_str).map(str::trim).filter(|value| !value.is_empty())
}

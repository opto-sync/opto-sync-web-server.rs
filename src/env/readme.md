# `src/env` — service environment overlay

Standard path for process configuration, with or without flags-2-env:

| Language | File |
| --- | --- |
| Rust | `src/env/env.rs` |
| TypeScript | `src/env/env.ts` |
| Dart | `src/env/env.dart` |

`main.rs` / `main.ts` / `main.dart` should import this module instead of
scattering `std::env::var` / `process.env` / `Platform.environment` reads.

## Layers (lowest → highest)

1. **Code defaults** in `env.rs` / `env.ts` / `env.dart` (safe local values).
2. **`.env` files** (`env_file`) — `./.env` by default, later files win.
3. **Process environment** (`env_shell`) — `PORT=7777 mycli` outranks `.env`.
4. **Argv flags** (`flags`) — only when flags-2-env parses a CLI.

flags-2-env is what ranks (2)–(4). Default rank is `flags > env_shell > env_file`.
That is why a live variable overrides `.env`, and why `--port` overrides both.

To lift `.env` over the process environment (still letting argv win):

```toml
[flags.token]
env = "API_TOKEN"
dotenv_override = true

# or for every key:
[env]
override = true
```

Per-key ranking:

```toml
[order-of-preference]
API_TOKEN = ["env_file", "env_shell", "flags"]
```

`FLAGS2ENV_DOTENV=0` can only **disable** file loading, never enable it.
Servers, MCP processes, and workers should set `[env] load = false` so a
hostile working-directory `.env` cannot inject values. Those processes take
deployment values from the real environment (and code defaults).

## Required values

If a required key is missing or empty after the overlay, throw and name:

- the exact env var (`DATABASE_URL`)
- the expected type (`string`, `int`, `bool`, …)
- examples of good values (`postgres://user:pass@127.0.0.1:5432/app`)

Do not depend on flags-2-env at runtime if the service has no CLI. Keep this
directory as the contract either way. When flags-2-env *is* used, `f2e generate
--src-env` writes `generated.rs` / `generated.ts` / `generated.dart` (do not
edit those) and scaffolds `env.*` once.

## Generate

```sh
f2e generate --src-env
f2e generate --src-env=src/env --lang rust,typescript,dart
```

# opto-sync-web-server.rs

Rust web server for Opto-Sync. The target web stack is MASH (Maud + Axum + HTMX), with thin Leptos and Dioxus server-side adapters where richer Rust UI composition is justified.

## Sync ownership

All Rust renderers must share the framework-neutral `web_sync::WebSyncService` application boundary:

```text
MASH / Maud + HTMX ─┐
Leptos server funcs ├─> WebSyncService ─> Opto-Sync durable queue + local readback
Dioxus server funcs ┘                         │
                                              └─> privacy-bounded SyncTelemetry
```

Framework code may extract requests and render responses. It must not implement a second mutation queue, replay loop, conflict resolver, retry policy, or optimistic projection engine.

`WebSyncService` consumes only an `AdmittedMutation`: form/field validation belongs to `ores-forms` and server-side contract admission before queueing. The service intentionally defines no serializable public wire schema; `opto-sync-interfaces` remains the contract authority with independent human-maintained TypeSpec and JSON Schema Draft 2020-12 peers admitted fail-closed by `ORESoftware/typespec-json-schema-validator`.

The queue and local-readback capabilities intentionally match the generic seam used by the custom RPC runtime in `ORESoftware/api-docs`. Dependency direction stays one-way: RPC may adapt to Opto-Sync; Opto-Sync must not depend on `api-docs`.

Telemetry is similarly dependency-inverted. `SyncTelemetry` receives correlation/lifecycle metadata but cannot receive request bodies, table/record identifiers, cookies, authorization headers, or form values. The application can adapt this sink to `ores-otel` without introducing a second telemetry vocabulary or exporter lifecycle inside sync core.

## HTTP transport

Four API avenues live in `src/transport`. The repository still needs its executable Axum listener/router wired over the shared service boundary before the MASH label should be considered production-complete; that work must preserve the committed `Cargo.lock` and pass locked clippy/tests/container checks.

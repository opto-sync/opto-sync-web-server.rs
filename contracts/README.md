# Web-server contract boundary

This repository is a Rust web application adapter for Opto Sync. It must not become a second wire/schema authority or invent synchronization semantics independently.

## Authority split

- Public Opto Sync wire/data contracts remain independently authored in `opto-sync-interfaces` as TypeSpec and JSON Schema Draft 2020-12 peers, admitted fail-closed through TJSV.
- Domain/form admission belongs to the owning application / `ores-forms` before `AdmittedMutation` reaches this seam.
- `src/web_sync.rs` owns this repository's implementation boundary: queue an already-admitted mutation, retain its mutation identity, and expose only a truthful optimistic local projection.
- Backend/server schema evolution belongs to Declarative Migrations, not web-server startup.

Generated OpenAPI, Contract IR, SDK types, schemas, and other projections are evidence only.

## Current sync boundary

The current implementation exposes `MutationQueue`, `LocalReadback`, and `WebSyncService`. It distinguishes durable queue admission from optimistic local projection and fails closed when a queued mutation has no truthful local projection.

There is no implemented authoritative checkpoint-view endpoint in this repository today. Monotonic/non-mutating checkpoint observation remains a future contract requirement until a concrete endpoint and storage read path exist and can be tested directly.

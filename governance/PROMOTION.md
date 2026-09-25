# Web-server promotion governance

Promotion is claim-scoped and fail-closed.

- Every conformance claim must identify the concrete implementation and exact-head test/check that exercises it.
- Queued, skipped, zero-step, stale-head, historical-only, runner-admission, billing-blocked, and missing-run results are not green evidence.
- Web frameworks/renderers must terminate at the shared `WebSyncService` seam rather than fork queue/reconciliation behavior.
- The web layer must not create a second public wire/schema authority; TypeSpec and Draft 2020-12 JSON Schema remain independent authored peers in `opto-sync-interfaces` and TJSV disagreements stop promotion.
- An optimistic local projection must never be labeled authoritative server success.
- Application startup does not own database schema migration; use Declarative Migrations and disposable-database migration qualification.

A future checkpoint/read-view feature requires a concrete route plus storage read path. Promotion then requires tests for non-mutation, declared ordering semantics, stale/reordered response handling, tenant/filter scope, and negative controls. A standalone state model cannot substitute for those implementation tests.

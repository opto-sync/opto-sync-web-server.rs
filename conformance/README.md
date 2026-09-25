# Web-server conformance

Conformance claims in this repository must refine executable web-server behavior.

## Current implementation-linked properties

1. Upserts and deletes are admitted through one `MutationQueue` seam rather than renderer-specific sync logic.
2. Queue admission produces a stable mutation identifier supplied by the queue implementation.
3. Optimistic UI/RPC output comes from `LocalReadback`; the web layer does not fabricate an authoritative server result.
4. A missing optimistic projection fails closed as `MissingLocalProjection` instead of being reported as success.
5. Delete admission does not invent an upsert payload.
6. Sync telemetry receives bounded lifecycle/correlation metadata rather than payloads, table/record values, auth data, cookies, or headers.

Evidence for these properties is in `src/web_sync.rs` and its Rust tests. Exact-head promotion must execute the applicable Rust format, Clippy, and test lanes plus contract/source-policy gates.

## Future checkpoint-view qualification

If an authoritative/read-only checkpoint endpoint is added, its implementation-linked tests must prove reads do not mutate queue/server state, observed checkpoint semantics match the authored ordering contract, stale/reordered responses cannot regress a newer accepted view, tenant/filter scope is explicit, and a checkpoint cannot be inferred from wall clock or transport liveness.

An abstract monotonic-counter model alone is design evidence, not endpoint conformance.

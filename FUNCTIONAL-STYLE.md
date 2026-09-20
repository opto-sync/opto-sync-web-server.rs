# Functional-oriented Rust server style

Canonical: https://github.com/ORESoftware/ores-middleware/blob/main/FUNCTIONAL-STYLE.md

**Build values; do not mutate caller-owned state.** Avoid `&mut T` except state-owner `&mut self` and forced APIs; prefer complete returned values, consuming builders, iterator pipelines, and explicit state transitions.

Keep `main.rs` a composition root. Load config/runtime concerns at the edge; keep deterministic policy/transforms pure; keep HTTP parse/map glue thin; isolate DB/queue/fs/clock/random/network effects in adapters; keep lifecycle/orchestration in `server`. Split mixed handlers into `parse -> pure validation/decision -> effect -> pure response mapping`. Extract modules for ownership/testability/reuse/effect boundaries, not file length.

Forced/measured mutation must be confined and marked `// HOT-PATH (imperative by design): ...`; use `TODO(measure)` for unmeasured performance claims; constructors return complete values.

//! Framework-neutral Opto-Sync application boundary for Rust web surfaces.
//!
//! MASH (Maud + Axum + HTMX), Leptos, and Dioxus adapters should terminate
//! framework-specific request extraction/rendering here instead of implementing
//! queueing, replay, local projection, retry, or conflict behavior themselves.
//!
//! This module deliberately does **not** define a serializable wire authority.
//! The public wire/data contracts remain owned by `opto-sync-interfaces` and
//! admitted through the independent TypeSpec + JSON Schema peer-authority gate.
//! Likewise, form/field validation belongs to `ores-forms` (and server admission)
//! before an [`AdmittedMutation`] is constructed.

use std::fmt;

/// A mutation that has already passed the caller's domain/interface admission.
///
/// `payload` is intentionally an opaque JSON string here. This web application
/// seam must not grow a second form validator or a second copy of the public
/// Opto-Sync wire schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmittedMutation<'a> {
    Upsert {
        table: &'a str,
        record_id: &'a str,
        payload: &'a str,
        base_revision: Option<&'a str>,
    },
    Delete {
        table: &'a str,
        record_id: &'a str,
        base_revision: Option<&'a str>,
    },
}

impl AdmittedMutation<'_> {
    #[must_use]
    pub fn operation(self) -> SyncOperation {
        match self {
            Self::Upsert { .. } => SyncOperation::Upsert,
            Self::Delete { .. } => SyncOperation::Delete,
        }
    }

    #[must_use]
    pub fn table(self) -> &str {
        match self {
            Self::Upsert { table, .. } | Self::Delete { table, .. } => table,
        }
    }

    #[must_use]
    pub fn record_id(self) -> &str {
        match self {
            Self::Upsert { record_id, .. } | Self::Delete { record_id, .. } => record_id,
        }
    }
}

/// Stable operation vocabulary shared by renderer adapters and telemetry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncOperation {
    Upsert,
    Delete,
}

/// Correlation identifiers supplied by the HTTP/RPC application boundary.
///
/// No request body, record id, table value, cookies, authorization headers, or
/// form values are admitted into this type. An `ores-otel` adapter can implement
/// [`SyncTelemetry`] without accidentally receiving those values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorrelationContext<'a> {
    pub request_id: &'a str,
    pub trace_id: Option<&'a str>,
    pub sync_cycle_id: Option<&'a str>,
}

/// Queue capability intentionally shaped like the generic queue consumed by the
/// custom RPC runtime in `ORESoftware/api-docs`.
///
/// `api-docs` may adapt *to* Opto-Sync. Opto-Sync must not depend on `api-docs`.
pub trait MutationQueue {
    type Error;

    fn queue_upsert(
        &mut self,
        table: &str,
        record_id: &str,
        payload: &str,
        base_revision: Option<&str>,
    ) -> Result<String, Self::Error>;

    fn queue_delete(
        &mut self,
        table: &str,
        record_id: &str,
        base_revision: Option<&str>,
    ) -> Result<String, Self::Error>;
}

/// Optimistic local projection capability shared by web and RPC adapters.
pub trait LocalReadback {
    type Error;

    fn local_json(&self, table: &str, record_id: &str) -> Result<Option<String>, Self::Error>;
}

/// Privacy-bounded lifecycle event suitable for an `ores-otel` adapter.
///
/// Deliberately excluded: payload/body, table, record id, base revision, auth
/// data, cookies, headers, and form values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncTelemetryEvent<'a> {
    pub stage: SyncTelemetryStage,
    pub operation: SyncOperation,
    pub request_id: &'a str,
    pub trace_id: Option<&'a str>,
    pub sync_cycle_id: Option<&'a str>,
    pub mutation_id: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncTelemetryStage {
    Queued,
    LocalProjectionAvailable,
    LocalProjectionMissing,
}

/// Small adapter point for `ores-otel`; exporter setup remains application-owned.
pub trait SyncTelemetry {
    fn record(&self, event: SyncTelemetryEvent<'_>);
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NoopSyncTelemetry;

impl SyncTelemetry for NoopSyncTelemetry {
    fn record(&self, _event: SyncTelemetryEvent<'_>) {}
}

/// Result of durable queue admission before optimistic readback is required.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueueReceipt {
    pub mutation_id: String,
}

/// Durable queue receipt plus the truthful optimistic local projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptimisticMutation {
    pub mutation_id: String,
    pub local_json: String,
}

#[derive(Debug)]
pub enum WebSyncError<Q, R> {
    Queue(Q),
    Readback(R),
    MissingLocalProjection,
}

impl<Q: fmt::Debug, R: fmt::Debug> fmt::Display for WebSyncError<Q, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queue(error) => write!(f, "opto-sync queue rejected mutation: {error:?}"),
            Self::Readback(error) => write!(f, "opto-sync local readback failed: {error:?}"),
            Self::MissingLocalProjection => {
                f.write_str("queued mutation has no truthful local projection")
            }
        }
    }
}

/// One application service for every Rust renderer.
///
/// MASH, Leptos, and Dioxus handlers may differ in request extraction and
/// response rendering, but all mutation paths should call this service so they
/// cannot fork queue/reconciliation semantics.
pub struct WebSyncService<Q, R, T = NoopSyncTelemetry> {
    queue: Q,
    readback: R,
    telemetry: T,
}

impl<Q, R> WebSyncService<Q, R, NoopSyncTelemetry> {
    #[must_use]
    pub fn new(queue: Q, readback: R) -> Self {
        Self {
            queue,
            readback,
            telemetry: NoopSyncTelemetry,
        }
    }
}

impl<Q, R, T> WebSyncService<Q, R, T> {
    #[must_use]
    pub fn with_telemetry(queue: Q, readback: R, telemetry: T) -> Self {
        Self {
            queue,
            readback,
            telemetry,
        }
    }

    #[must_use]
    pub fn into_parts(self) -> (Q, R, T) {
        (self.queue, self.readback, self.telemetry)
    }
}

impl<Q, R, T> WebSyncService<Q, R, T>
where
    Q: MutationQueue,
    R: LocalReadback,
    T: SyncTelemetry,
{
    /// Persist an already-admitted mutation without requiring a local projection.
    pub fn enqueue(
        &mut self,
        mutation: AdmittedMutation<'_>,
        correlation: CorrelationContext<'_>,
    ) -> Result<QueueReceipt, WebSyncError<Q::Error, R::Error>> {
        let operation = mutation.operation();
        let mutation_id = match mutation {
            AdmittedMutation::Upsert {
                table,
                record_id,
                payload,
                base_revision,
            } => self
                .queue
                .queue_upsert(table, record_id, payload, base_revision)
                .map_err(WebSyncError::Queue)?,
            AdmittedMutation::Delete {
                table,
                record_id,
                base_revision,
            } => self
                .queue
                .queue_delete(table, record_id, base_revision)
                .map_err(WebSyncError::Queue)?,
        };

        self.telemetry.record(SyncTelemetryEvent {
            stage: SyncTelemetryStage::Queued,
            operation,
            request_id: correlation.request_id,
            trace_id: correlation.trace_id,
            sync_cycle_id: correlation.sync_cycle_id,
            mutation_id: Some(&mutation_id),
        });

        Ok(QueueReceipt { mutation_id })
    }

    /// Queue a mutation and require the optimistic local view that UI/RPC callers
    /// may truthfully render while the authoritative write is still pending.
    pub fn submit_and_project(
        &mut self,
        mutation: AdmittedMutation<'_>,
        correlation: CorrelationContext<'_>,
    ) -> Result<OptimisticMutation, WebSyncError<Q::Error, R::Error>> {
        let table = mutation.table();
        let record_id = mutation.record_id();
        let operation = mutation.operation();
        let receipt = self.enqueue(mutation, correlation)?;
        let local_json = self
            .readback
            .local_json(table, record_id)
            .map_err(WebSyncError::Readback)?;

        match local_json {
            Some(local_json) => {
                self.telemetry.record(SyncTelemetryEvent {
                    stage: SyncTelemetryStage::LocalProjectionAvailable,
                    operation,
                    request_id: correlation.request_id,
                    trace_id: correlation.trace_id,
                    sync_cycle_id: correlation.sync_cycle_id,
                    mutation_id: Some(&receipt.mutation_id),
                });
                Ok(OptimisticMutation {
                    mutation_id: receipt.mutation_id,
                    local_json,
                })
            }
            None => {
                self.telemetry.record(SyncTelemetryEvent {
                    stage: SyncTelemetryStage::LocalProjectionMissing,
                    operation,
                    request_id: correlation.request_id,
                    trace_id: correlation.trace_id,
                    sync_cycle_id: correlation.sync_cycle_id,
                    mutation_id: Some(&receipt.mutation_id),
                });
                Err(WebSyncError::MissingLocalProjection)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Debug, Default)]
    struct FakeQueue {
        calls: Vec<String>,
    }

    impl MutationQueue for FakeQueue {
        type Error = &'static str;

        fn queue_upsert(
            &mut self,
            table: &str,
            record_id: &str,
            payload: &str,
            base_revision: Option<&str>,
        ) -> Result<String, Self::Error> {
            self.calls.push(format!(
                "upsert:{table}:{record_id}:{payload}:{}",
                base_revision.unwrap_or("-")
            ));
            Ok("mutation-upsert".to_string())
        }

        fn queue_delete(
            &mut self,
            table: &str,
            record_id: &str,
            base_revision: Option<&str>,
        ) -> Result<String, Self::Error> {
            self.calls.push(format!(
                "delete:{table}:{record_id}:{}",
                base_revision.unwrap_or("-")
            ));
            Ok("mutation-delete".to_string())
        }
    }

    #[derive(Debug)]
    struct FakeReadback(Option<String>);

    impl LocalReadback for FakeReadback {
        type Error = &'static str;

        fn local_json(
            &self,
            _table: &str,
            _record_id: &str,
        ) -> Result<Option<String>, Self::Error> {
            Ok(self.0.clone())
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    struct RecordedEvent {
        stage: SyncTelemetryStage,
        operation: SyncOperation,
        request_id: String,
        trace_id: Option<String>,
        sync_cycle_id: Option<String>,
        mutation_id: Option<String>,
    }

    #[derive(Debug, Default)]
    struct RecordingTelemetry(RefCell<Vec<RecordedEvent>>);

    impl SyncTelemetry for RecordingTelemetry {
        fn record(&self, event: SyncTelemetryEvent<'_>) {
            self.0.borrow_mut().push(RecordedEvent {
                stage: event.stage,
                operation: event.operation,
                request_id: event.request_id.to_string(),
                trace_id: event.trace_id.map(str::to_string),
                sync_cycle_id: event.sync_cycle_id.map(str::to_string),
                mutation_id: event.mutation_id.map(str::to_string),
            });
        }
    }

    fn correlation() -> CorrelationContext<'static> {
        CorrelationContext {
            request_id: "req-1",
            trace_id: Some("trace-1"),
            sync_cycle_id: Some("cycle-1"),
        }
    }

    #[test]
    fn upsert_uses_one_queue_and_truthful_local_projection_boundary() {
        let telemetry = RecordingTelemetry::default();
        let mut service = WebSyncService::with_telemetry(
            FakeQueue::default(),
            FakeReadback(Some(r#"{"id":"42","name":"local"}"#.to_string())),
            telemetry,
        );

        let result = service
            .submit_and_project(
                AdmittedMutation::Upsert {
                    table: "widgets",
                    record_id: "42",
                    payload: r#"{"name":"local"}"#,
                    base_revision: Some("7"),
                },
                correlation(),
            )
            .expect("admitted mutation should queue and project");

        assert_eq!(result.mutation_id, "mutation-upsert");
        assert_eq!(result.local_json, r#"{"id":"42","name":"local"}"#);

        let (queue, _readback, telemetry) = service.into_parts();
        assert_eq!(
            queue.calls,
            [r#"upsert:widgets:42:{"name":"local"}:7"#.to_string()]
        );
        let events = telemetry.0.into_inner();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].stage, SyncTelemetryStage::Queued);
        assert_eq!(events[1].stage, SyncTelemetryStage::LocalProjectionAvailable);
    }

    #[test]
    fn delete_never_invents_a_payload() {
        let mut service = WebSyncService::new(
            FakeQueue::default(),
            FakeReadback(Some(r#"{"id":"42","deleted":true}"#.to_string())),
        );

        let result = service
            .submit_and_project(
                AdmittedMutation::Delete {
                    table: "widgets",
                    record_id: "42",
                    base_revision: None,
                },
                correlation(),
            )
            .expect("delete should use the delete queue operation");

        assert_eq!(result.mutation_id, "mutation-delete");
        let (queue, _readback, _telemetry) = service.into_parts();
        assert_eq!(queue.calls, ["delete:widgets:42:-".to_string()]);
    }

    #[test]
    fn missing_projection_is_not_reported_as_authoritative_success() {
        let telemetry = RecordingTelemetry::default();
        let mut service = WebSyncService::with_telemetry(
            FakeQueue::default(),
            FakeReadback(None),
            telemetry,
        );

        let error = service
            .submit_and_project(
                AdmittedMutation::Upsert {
                    table: "widgets",
                    record_id: "42",
                    payload: r#"{"name":"local"}"#,
                    base_revision: None,
                },
                correlation(),
            )
            .expect_err("missing local readback must fail closed");

        assert!(matches!(error, WebSyncError::MissingLocalProjection));
        let (_queue, _readback, telemetry) = service.into_parts();
        let events = telemetry.0.into_inner();
        assert_eq!(events.last().map(|event| event.stage), Some(SyncTelemetryStage::LocalProjectionMissing));
    }

    #[test]
    fn telemetry_surface_cannot_receive_body_or_record_identifiers() {
        let event = SyncTelemetryEvent {
            stage: SyncTelemetryStage::Queued,
            operation: SyncOperation::Upsert,
            request_id: "req-privacy",
            trace_id: Some("trace-privacy"),
            sync_cycle_id: Some("cycle-privacy"),
            mutation_id: Some("mutation-privacy"),
        };

        let debug = format!("{event:?}");
        assert!(!debug.contains("secret-body"));
        assert!(!debug.contains("record-123"));
        assert!(!debug.contains("authorization"));
    }
}

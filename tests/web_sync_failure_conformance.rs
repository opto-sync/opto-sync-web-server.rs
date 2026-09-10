use std::cell::{Cell, RefCell};

use opto_sync_web_server::web_sync::{
    AdmittedMutation, CorrelationContext, LocalReadback, MutationQueue, SyncTelemetry,
    SyncTelemetryEvent, SyncTelemetryStage, WebSyncError, WebSyncService,
};

#[derive(Default)]
struct FailingQueue;

impl MutationQueue for FailingQueue {
    type Error = &'static str;

    fn queue_upsert(
        &mut self,
        _table: &str,
        _record_id: &str,
        _payload: &str,
        _base_revision: Option<&str>,
    ) -> Result<String, Self::Error> {
        Err("queue unavailable")
    }

    fn queue_delete(
        &mut self,
        _table: &str,
        _record_id: &str,
        _base_revision: Option<&str>,
    ) -> Result<String, Self::Error> {
        Err("queue unavailable")
    }
}

#[derive(Default)]
struct SuccessQueue {
    upserts: usize,
    deletes: usize,
}

impl MutationQueue for SuccessQueue {
    type Error = &'static str;

    fn queue_upsert(
        &mut self,
        _table: &str,
        _record_id: &str,
        _payload: &str,
        _base_revision: Option<&str>,
    ) -> Result<String, Self::Error> {
        self.upserts += 1;
        Ok("mutation-1".to_owned())
    }

    fn queue_delete(
        &mut self,
        _table: &str,
        _record_id: &str,
        _base_revision: Option<&str>,
    ) -> Result<String, Self::Error> {
        self.deletes += 1;
        Ok("mutation-delete-1".to_owned())
    }
}

#[derive(Default)]
struct CountingReadback {
    calls: Cell<usize>,
}

impl LocalReadback for CountingReadback {
    type Error = &'static str;

    fn local_json(&self, _table: &str, _record_id: &str) -> Result<Option<String>, Self::Error> {
        self.calls.set(self.calls.get() + 1);
        Ok(Some(r#"{"id":"42"}"#.to_owned()))
    }
}

struct FailingReadback;

impl LocalReadback for FailingReadback {
    type Error = &'static str;

    fn local_json(&self, _table: &str, _record_id: &str) -> Result<Option<String>, Self::Error> {
        Err("readback unavailable")
    }
}

struct PanicReadback;

impl LocalReadback for PanicReadback {
    type Error = &'static str;

    fn local_json(&self, _table: &str, _record_id: &str) -> Result<Option<String>, Self::Error> {
        panic!("enqueue must not consult local readback")
    }
}

#[derive(Default)]
struct RecordingTelemetry {
    stages: RefCell<Vec<SyncTelemetryStage>>,
}

impl SyncTelemetry for RecordingTelemetry {
    fn record(&self, event: SyncTelemetryEvent<'_>) {
        self.stages.borrow_mut().push(event.stage);
    }
}

fn correlation() -> CorrelationContext<'static> {
    CorrelationContext {
        request_id: "req-failure-conformance",
        trace_id: Some("trace-failure-conformance"),
        sync_cycle_id: Some("cycle-failure-conformance"),
    }
}

fn upsert() -> AdmittedMutation<'static> {
    AdmittedMutation::Upsert {
        table: "widgets",
        record_id: "42",
        payload: r#"{"name":"queued"}"#,
        base_revision: Some("8"),
    }
}

#[test]
fn queue_failure_never_reads_local_projection_or_emits_queued_telemetry() {
    let telemetry = RecordingTelemetry::default();
    let mut service =
        WebSyncService::with_telemetry(FailingQueue, CountingReadback::default(), telemetry);

    let error = service
        .submit_and_project(upsert(), correlation())
        .expect_err("queue failure must stop before local projection");

    assert!(matches!(error, WebSyncError::Queue("queue unavailable")));

    let (_queue, readback, telemetry) = service.into_parts();
    assert_eq!(readback.calls.get(), 0, "readback ran after queue failure");
    assert!(
        telemetry.stages.into_inner().is_empty(),
        "queued telemetry was emitted before durable queue admission"
    );
}

#[test]
fn readback_failure_occurs_only_after_durable_queue_admission() {
    let telemetry = RecordingTelemetry::default();
    let mut service = WebSyncService::with_telemetry(SuccessQueue::default(), FailingReadback, telemetry);

    let error = service
        .submit_and_project(upsert(), correlation())
        .expect_err("readback failure must remain distinct from queue failure");

    assert!(matches!(
        error,
        WebSyncError::Readback("readback unavailable")
    ));

    let (queue, _readback, telemetry) = service.into_parts();
    assert_eq!(queue.upserts, 1, "mutation was not durably admitted first");
    assert_eq!(queue.deletes, 0);
    assert_eq!(
        telemetry.stages.into_inner(),
        vec![SyncTelemetryStage::Queued],
        "readback failure must not emit projection-available or projection-missing telemetry"
    );
}

#[test]
fn enqueue_is_a_projection_free_durability_boundary() {
    let telemetry = RecordingTelemetry::default();
    let mut service = WebSyncService::with_telemetry(SuccessQueue::default(), PanicReadback, telemetry);

    let receipt = service
        .enqueue(upsert(), correlation())
        .expect("durable admission should not require local readback");

    assert_eq!(receipt.mutation_id, "mutation-1");
    let (queue, _readback, telemetry) = service.into_parts();
    assert_eq!(queue.upserts, 1);
    assert_eq!(queue.deletes, 0);
    assert_eq!(
        telemetry.stages.into_inner(),
        vec![SyncTelemetryStage::Queued]
    );
}

#[test]
fn failed_delete_uses_only_the_delete_queue_path() {
    struct DeleteOnlyFailure {
        upsert_called: Cell<bool>,
        delete_called: Cell<bool>,
    }

    impl MutationQueue for DeleteOnlyFailure {
        type Error = &'static str;

        fn queue_upsert(
            &mut self,
            _table: &str,
            _record_id: &str,
            _payload: &str,
            _base_revision: Option<&str>,
        ) -> Result<String, Self::Error> {
            self.upsert_called.set(true);
            Err("wrong path")
        }

        fn queue_delete(
            &mut self,
            _table: &str,
            _record_id: &str,
            _base_revision: Option<&str>,
        ) -> Result<String, Self::Error> {
            self.delete_called.set(true);
            Err("delete unavailable")
        }
    }

    let queue = DeleteOnlyFailure {
        upsert_called: Cell::new(false),
        delete_called: Cell::new(false),
    };
    let mut service = WebSyncService::new(queue, CountingReadback::default());

    let error = service
        .submit_and_project(
            AdmittedMutation::Delete {
                table: "widgets",
                record_id: "42",
                base_revision: Some("8"),
            },
            correlation(),
        )
        .expect_err("delete queue failure must fail closed");

    assert!(matches!(error, WebSyncError::Queue("delete unavailable")));
    let (queue, readback, _telemetry) = service.into_parts();
    assert!(queue.delete_called.get());
    assert!(!queue.upsert_called.get());
    assert_eq!(readback.calls.get(), 0);
}

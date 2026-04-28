//! In-memory run registry.
//!
//! Owns the lifetime of every active run. The orchestrator returns a
//! `BoxStream<RunEvent>`; the server forks that into:
//!   1. **A buffered log**: a `Vec<(u64, RunEvent)>` we keep so SSE
//!      subscribers can resume with `Last-Event-ID` while the run is
//!      live and so late subscribers receive the prefix they missed.
//!   2. **A live broadcast channel**: subscribers tail the broadcast
//!      receiver after the buffered prefix has been replayed.
//!
//! TTL: when a run terminates with `RunEnd`, we record the moment and
//! retain the entry for [`RUN_RETENTION`]. A background task wakes up
//! periodically (`RUN_REAPER_TICK`) and reaps runs whose
//! `terminated_at + RUN_RETENTION < now`. After that point a SSE
//! request for that run id returns 410 Gone (per OpenAPI).
//!
//! Cancellation: the registry stores the run's `CancellationToken`
//! alongside the buffer. `POST /v1/runs/{id}/cancel` fires it, the
//! orchestrator observes it within ~100ms, emits
//! `RunEnd { status: Cancelled }`, and the registry transitions the
//! entry to its terminated state.

use std::sync::Arc;
use std::time::{Duration, Instant};

use harness_core::{RunEvent, RunId, RunStatus};
use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;

/// How long after termination a run's events stay subscribable.
pub const RUN_RETENTION: Duration = Duration::from_secs(60);

/// Reaper tick: how often the background task scans for expired runs.
pub const RUN_REAPER_TICK: Duration = Duration::from_secs(5);

/// Capacity of each run's broadcast channel. Subscribers that lag past
/// this point will see a `RecvError::Lagged`; they fall back to the
/// buffered log on reconnect.
pub const BROADCAST_CAPACITY: usize = 256;

/// Lifecycle state for a registered run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunLifecycle {
    Live,
    Terminated {
        status: RunStatus,
        at: Instant,
    },
    /// Reaper has cleared the entry — accessing it returns 410 Gone.
    /// Surfaced as a separate state so callers can distinguish
    /// "never existed" from "expired".
    Expired,
}

/// One sequenced event in the buffered log.
#[derive(Debug, Clone)]
pub struct SequencedEvent {
    pub seq: u64,
    pub event: RunEvent,
}

/// Per-run entry held by [`RunRegistry`]. All fields are owned by the
/// registry; subscribers receive cheap clones (the broadcast sender is
/// already `Arc` internally).
struct RunEntry {
    cancel: CancellationToken,
    /// Terminal status if the run has ended; `None` while live.
    lifecycle: RunLifecycle,
    /// Append-only buffered log so resume / late subscribe works.
    buffered: Vec<SequencedEvent>,
    /// Live broadcast for subscribers that have already replayed the
    /// buffered prefix. Each item carries its sequence number so a
    /// subscriber can de-duplicate against its own cursor.
    sender: broadcast::Sender<SequencedEvent>,
    next_seq: u64,
}

impl std::fmt::Debug for RunEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunEntry")
            .field("lifecycle", &self.lifecycle)
            .field("buffered_len", &self.buffered.len())
            .field("next_seq", &self.next_seq)
            .finish()
    }
}

/// Snapshot returned to subscribers: the prefix they should replay
/// immediately, followed by a live receiver they should drain
/// thereafter.
#[derive(Debug)]
pub struct Subscription {
    /// Events with `seq > last_event_id`. Empty when the caller is
    /// already up-to-date.
    pub backlog: Vec<SequencedEvent>,
    /// `None` when the run has already terminated and its terminal
    /// event is in `backlog`. Otherwise a live tail.
    pub live: Option<broadcast::Receiver<SequencedEvent>>,
    /// Terminal status if the run has ended; `None` while live.
    pub terminal: Option<RunStatus>,
}

#[derive(Debug, thiserror::Error)]
pub enum SubscribeError {
    #[error("run not found")]
    NotFound,
    #[error("run expired and its event log was reaped")]
    Expired,
}

#[derive(Debug, thiserror::Error)]
pub enum CancelError {
    #[error("run not found")]
    NotFound,
    #[error("run already terminated")]
    AlreadyTerminated,
    #[error("run expired")]
    Expired,
}

/// Thread-safe registry of in-flight + recently-terminated runs.
#[derive(Clone, Default)]
pub struct RunRegistry {
    inner: Arc<RwLock<std::collections::HashMap<String, RunEntry>>>,
}

impl std::fmt::Debug for RunRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunRegistry").finish_non_exhaustive()
    }
}

impl RunRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a new run. Returns the cancellation token the caller
    /// should pass to `Orchestrator::run`. The registry takes ownership
    /// of a clone of that token so `cancel()` from the HTTP layer fires
    /// the same token.
    pub async fn register(&self, run_id: &RunId) -> CancellationToken {
        let token = CancellationToken::new();
        let (sender, _) = broadcast::channel(BROADCAST_CAPACITY);
        let entry = RunEntry {
            cancel: token.clone(),
            lifecycle: RunLifecycle::Live,
            buffered: Vec::new(),
            sender,
            next_seq: 1,
        };
        let mut guard = self.inner.write().await;
        guard.insert(run_id.as_str().to_owned(), entry);
        token
    }

    /// Append an event to the run's buffered log and broadcast it. If
    /// the event is `RunEnd`, transition the entry to `Terminated`.
    pub async fn append(&self, run_id: &RunId, event: RunEvent) {
        let mut guard = self.inner.write().await;
        let Some(entry) = guard.get_mut(run_id.as_str()) else {
            return;
        };
        let seq = entry.next_seq;
        entry.next_seq += 1;
        let sequenced = SequencedEvent {
            seq,
            event: event.clone(),
        };
        entry.buffered.push(sequenced.clone());
        // Broadcast errors when there are no subscribers — that's fine.
        let _ = entry.sender.send(sequenced);

        if let RunEvent::RunEnd { status, .. } = event {
            entry.lifecycle = RunLifecycle::Terminated {
                status,
                at: Instant::now(),
            };
        }
    }

    /// Subscribe to a run's events, replaying from `last_event_id`. If
    /// `last_event_id` is `None` the caller gets the full buffered log.
    pub async fn subscribe(
        &self,
        run_id: &RunId,
        last_event_id: Option<u64>,
    ) -> Result<Subscription, SubscribeError> {
        let guard = self.inner.read().await;
        let entry = guard.get(run_id.as_str()).ok_or(SubscribeError::NotFound)?;
        if matches!(entry.lifecycle, RunLifecycle::Expired) {
            return Err(SubscribeError::Expired);
        }
        let cutoff = last_event_id.unwrap_or(0);
        let backlog: Vec<SequencedEvent> = entry
            .buffered
            .iter()
            .filter(|e| e.seq > cutoff)
            .cloned()
            .collect();
        let (terminal, live) = match entry.lifecycle {
            RunLifecycle::Live => (None, Some(entry.sender.subscribe())),
            RunLifecycle::Terminated { status, .. } => (Some(status), None),
            RunLifecycle::Expired => unreachable!("checked above"),
        };
        Ok(Subscription {
            backlog,
            live,
            terminal,
        })
    }

    /// Fire the run's cancellation token. Idempotent: cancelling an
    /// already-terminated run is a no-op (returns
    /// `CancelError::AlreadyTerminated`).
    pub async fn cancel(&self, run_id: &RunId) -> Result<(), CancelError> {
        let guard = self.inner.read().await;
        let entry = guard.get(run_id.as_str()).ok_or(CancelError::NotFound)?;
        match entry.lifecycle {
            RunLifecycle::Terminated { .. } => Err(CancelError::AlreadyTerminated),
            RunLifecycle::Expired => Err(CancelError::Expired),
            RunLifecycle::Live => {
                entry.cancel.cancel();
                Ok(())
            }
        }
    }

    /// Drop entries whose terminal `at` is older than `RUN_RETENTION`.
    /// Returns the count reaped (test helper).
    pub async fn reap_expired(&self, retention: Duration) -> usize {
        let now = Instant::now();
        let mut guard = self.inner.write().await;
        let to_remove: Vec<String> = guard
            .iter()
            .filter_map(|(id, entry)| match entry.lifecycle {
                RunLifecycle::Terminated { at, .. } if now.duration_since(at) >= retention => {
                    Some(id.clone())
                }
                _ => None,
            })
            .collect();
        let n = to_remove.len();
        for id in to_remove {
            guard.remove(&id);
        }
        n
    }

    /// Spawn the background reaper. Returns a guard that aborts the
    /// task on drop; in production we leak it (tied to process
    /// lifetime), in tests the caller drops it before tempdirs go.
    pub fn spawn_reaper(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(RUN_REAPER_TICK).await;
                self.reap_expired(RUN_RETENTION).await;
            }
        })
    }
}

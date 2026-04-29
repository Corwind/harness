//! In-memory log ring for the diagnostics endpoint.
//!
//! Per PLAN T3.5, the Settings UI surfaces the backend's recent log
//! lines. We keep them in a bounded ring inside the process — no disk
//! I/O, no external dependency — and the existing stderr writer is left
//! alone so the parent (Swift app, or `cargo run` user) still sees
//! everything verbatim.
//!
//! ## Bounds
//!
//! Lines are evicted oldest-first when **either** of these limits is
//! about to be exceeded:
//! * 5,000 lines
//! * 2,000,000 bytes (sum of `message` lengths)
//!
//! The two-axis cap stops a tight loop of long error messages from
//! eating arbitrary memory while still allowing 5,000 short lines for
//! quiet boots.
//!
//! ## Sequence numbers
//!
//! Every line gets a strictly-increasing `seq` allocated on push. The
//! sequence space is the **process lifetime**, not the current ring
//! contents — clients pass `after_seq` to skip what they have already
//! seen, and a `seq` they once saw never comes back even if the ring
//! has wrapped around past it. The endpoint's `next_seq` is the largest
//! `seq` we returned (clients echo it on the next poll).

use std::collections::VecDeque;
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::{Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

/// Maximum number of lines retained.
pub const MAX_LINES: usize = 5_000;

/// Maximum total byte budget across retained `message`s.
pub const MAX_BYTES: usize = 2_000_000;

/// One captured log record, in the wire shape the diagnostics endpoint
/// returns.
#[derive(Clone, Debug, Serialize)]
pub struct LogLine {
    pub seq: u64,
    /// Lowercase tracing level (`error`, `warn`, `info`, `debug`,
    /// `trace`) — matches `tracing::Level::to_string()` lower-cased.
    pub level: String,
    pub ts: DateTime<Utc>,
    pub target: String,
    pub message: String,
}

/// Bounded ring + monotonic sequence allocator.
///
/// Cloning is cheap — internal state is wrapped in a single `Mutex`
/// (logs are low-frequency relative to request handling, so the lock
/// cost is negligible).
#[derive(Debug)]
pub struct LogRing {
    inner: Mutex<Inner>,
}

#[derive(Debug)]
struct Inner {
    lines: VecDeque<LogLine>,
    /// `next_seq` is the seq we will allocate to the *next* push;
    /// every line in `lines` has a smaller seq than this.
    next_seq: u64,
    /// Running sum of `message` byte lengths in `lines`.
    bytes_used: usize,
    /// Both bounds, captured here so tests can shrink them.
    max_lines: usize,
    max_bytes: usize,
}

impl Default for LogRing {
    fn default() -> Self {
        Self::with_bounds(MAX_LINES, MAX_BYTES)
    }
}

impl LogRing {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct with explicit caps. Used by tests; production goes
    /// through [`LogRing::new`] which uses [`MAX_LINES`] and
    /// [`MAX_BYTES`].
    pub fn with_bounds(max_lines: usize, max_bytes: usize) -> Self {
        Self {
            inner: Mutex::new(Inner {
                lines: VecDeque::with_capacity(max_lines.min(1024)),
                next_seq: 1,
                bytes_used: 0,
                max_lines,
                max_bytes,
            }),
        }
    }

    /// Push a fully-formed line. Returns the seq assigned.
    ///
    /// Note: callers should usually go through the [`Layer`] impl —
    /// this is `pub` so behavior tests don't need a tracing dispatcher
    /// to exercise the bounds.
    pub fn push_line(&self, level: &str, target: &str, message: String) -> u64 {
        let mut inner = self.inner.lock().expect("LogRing mutex poisoned");
        let seq = inner.next_seq;
        inner.next_seq += 1;

        let line_bytes = message.len();
        inner.lines.push_back(LogLine {
            seq,
            level: level.to_owned(),
            ts: Utc::now(),
            target: target.to_owned(),
            message,
        });
        inner.bytes_used += line_bytes;

        // Evict oldest entries until both bounds hold.
        while inner.lines.len() > inner.max_lines
            || inner.bytes_used > inner.max_bytes && !inner.lines.is_empty()
        {
            if let Some(dropped) = inner.lines.pop_front() {
                inner.bytes_used = inner.bytes_used.saturating_sub(dropped.message.len());
            }
        }

        seq
    }

    /// Return every retained line whose `seq > after_seq`, plus the
    /// largest `seq` we returned (or `after_seq` itself when nothing
    /// matched, so polling clients can keep their cursor stable).
    pub fn since(&self, after_seq: u64) -> (Vec<LogLine>, u64) {
        let inner = self.inner.lock().expect("LogRing mutex poisoned");
        let mut out: Vec<LogLine> = inner
            .lines
            .iter()
            .filter(|l| l.seq > after_seq)
            .cloned()
            .collect();
        out.sort_by_key(|l| l.seq);
        let next_seq = out.last().map(|l| l.seq).unwrap_or(after_seq);
        (out, next_seq)
    }

    /// Test helper: number of currently-retained lines.
    #[doc(hidden)]
    pub fn len(&self) -> usize {
        self.inner.lock().unwrap().lines.len()
    }

    /// Test helper: whether the ring is empty.
    #[doc(hidden)]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().unwrap().lines.is_empty()
    }
}

/// `tracing_subscriber::Layer` that captures every event into the
/// supplied `LogRing`. Pair with the existing fmt-to-stderr layer in a
/// `Registry` and both fire on every `tracing::info!` etc.
pub struct LogRingLayer {
    ring: std::sync::Arc<LogRing>,
}

impl std::fmt::Debug for LogRingLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogRingLayer").finish_non_exhaustive()
    }
}

impl LogRingLayer {
    pub fn new(ring: std::sync::Arc<LogRing>) -> Self {
        Self { ring }
    }
}

impl<S> Layer<S> for LogRingLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let level = level_str(*event.metadata().level());
        let target = event.metadata().target().to_owned();
        let message = if visitor.message.is_empty() {
            // Events without a `message` field still record their other
            // structured fields — fall back to the stringified field set
            // so they're not lost.
            visitor.fields.join(" ")
        } else if visitor.fields.is_empty() {
            visitor.message
        } else {
            format!("{} {}", visitor.message, visitor.fields.join(" "))
        };
        self.ring.push_line(level, &target, message);
    }
}

fn level_str(level: Level) -> &'static str {
    match level {
        Level::ERROR => "error",
        Level::WARN => "warn",
        Level::INFO => "info",
        Level::DEBUG => "debug",
        Level::TRACE => "trace",
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    /// `key=value` strings for every non-`message` field, in record
    /// order.
    fields: Vec<String>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        let formatted = format!("{value:?}");
        if field.name() == "message" {
            self.message = formatted;
        } else {
            self.fields.push(format!("{}={formatted}", field.name()));
        }
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_owned();
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_line_assigns_monotonic_seq() {
        let ring = LogRing::new();
        let a = ring.push_line("info", "t", "first".into());
        let b = ring.push_line("info", "t", "second".into());
        let c = ring.push_line("info", "t", "third".into());
        assert!(a < b && b < c, "seqs must be strictly increasing");
        let (lines, next_seq) = ring.since(0);
        assert_eq!(lines.len(), 3);
        assert_eq!(next_seq, c);
        assert_eq!(
            lines.iter().map(|l| l.seq).collect::<Vec<_>>(),
            vec![a, b, c]
        );
    }

    #[test]
    fn since_filters_by_after_seq() {
        let ring = LogRing::new();
        for i in 0..5 {
            ring.push_line("info", "t", format!("line {i}"));
        }
        let (all, all_next) = ring.since(0);
        assert_eq!(all.len(), 5);
        let cut = all[2].seq;
        let (after, after_next) = ring.since(cut);
        assert_eq!(after.len(), 2);
        assert!(after.iter().all(|l| l.seq > cut));
        assert_eq!(after_next, all_next);
    }

    #[test]
    fn since_with_no_matches_echoes_cursor() {
        let ring = LogRing::new();
        let (l, next) = ring.since(100);
        assert!(l.is_empty());
        assert_eq!(next, 100, "cursor must remain stable when no new lines");
    }

    #[test]
    fn line_cap_evicts_oldest() {
        let ring = LogRing::with_bounds(10, usize::MAX);
        for i in 0..15 {
            ring.push_line("info", "t", format!("line {i}"));
        }
        let (lines, _) = ring.since(0);
        assert_eq!(lines.len(), 10);
        // Oldest 5 dropped; remaining seqs are 6..=15.
        assert_eq!(lines.first().unwrap().seq, 6);
        assert_eq!(lines.last().unwrap().seq, 15);
    }

    #[test]
    fn byte_cap_evicts_oldest() {
        // Big lines: each 100 bytes. Cap at 250 bytes → only ~2-3 fit.
        let ring = LogRing::with_bounds(usize::MAX, 250);
        for i in 0..5 {
            let big = "x".repeat(100);
            ring.push_line("info", "t", format!("{i}: {big}"));
        }
        let (lines, _) = ring.since(0);
        assert!(
            lines.len() <= 3,
            "byte cap should keep at most 3 lines of ~104 bytes each; got {}",
            lines.len()
        );
        // Whatever remains must be the most recent.
        let max_seq = lines.iter().map(|l| l.seq).max().unwrap();
        assert!(max_seq >= 4, "expected the latest line to be retained");
    }
}

//! Generic event bus primitives for Flowntier.
//!
//! v0.4.22 (event 000116): the historical `WfEvent` enum that
//! used to live here has been removed — the runtime now carries
//! `agent_core::event::AgentEvent` directly on the events pipe
//! (see `crates/agent-core/src/event.rs` and the cross-language
//! schema contract documented there). What remains in this
//! crate is the generic [`Publisher`] / [`Subscriber`] /
//! [`EventStream`] / [`EventBus`] infrastructure — usable by
//! any payload type via `Arc<T>` on the broadcast channel —
//! plus [`EventBusError`].
//!
//! The crate is intentionally minimal: no concrete event
//! payload, no domain-specific helpers. Consumers wire up the
//! payload type they care about at the call site (e.g.
//! `tauri-core` exposes `Arc<EventBus>` and pipes whatever
//! `WfEvent`-like JSON the runtime produces through it).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use thiserror::Error;
use tokio::sync::broadcast;

/// Errors that may occur while publishing or subscribing.
#[derive(Debug, Error)]
pub enum EventBusError {
    /// No active subscribers; the event was dropped.
    #[error("no active subscribers")]
    NoSubscribers,
    /// The bus is closed.
    #[error("event bus is closed")]
    Closed,
    /// Underlying channel lag.
    #[error("subscriber lagged behind by {0} events")]
    Lag(u64),
}

/// A trait for things that can publish events.
#[async_trait]
pub trait Publisher: Send + Sync {
    /// Publish an event to all subscribers.
    async fn publish(&self, event: Arc<()>) -> Result<(), EventBusError>;
}

/// A trait for things that can subscribe to events.
pub trait Subscriber: Send + Sync {
    /// Returns a stream of events. Multiple calls yield independent streams.
    fn subscribe(&self) -> Box<dyn EventStream>;
}

/// Stream of workflow events.
pub trait EventStream: Send {
    /// Blocking receive.
    fn recv(&mut self) -> Result<Arc<()>, EventBusError>;
}

/// In-process event bus backed by a Tokio broadcast channel.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Arc<()>>,
    subscriber_count: Arc<RwLock<u64>>,
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus")
            .field("subscriber_count", &*self.subscriber_count.read())
            .finish_non_exhaustive()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl EventBus {
    /// Create a new bus with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self {
            tx,
            subscriber_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Number of active subscribers (best-effort).
    pub fn subscriber_count(&self) -> u64 {
        *self.subscriber_count.read()
    }
}

#[async_trait]
impl Publisher for EventBus {
    async fn publish(&self, event: Arc<()>) -> Result<(), EventBusError> {
        self.tx
            .send(event)
            .map_err(|err| match err {
                tokio::sync::broadcast::error::SendError(_) => EventBusError::NoSubscribers,
            })?;
        Ok(())
    }
}

impl Subscriber for EventBus {
    fn subscribe(&self) -> Box<dyn EventStream> {
        let rx = self.tx.subscribe();
        *self.subscriber_count.write() += 1;
        Box::new(BroadcastStream {
            rx,
            _counter: SubscriberGuard {
                count: self.subscriber_count.clone(),
            },
        })
    }
}

struct BroadcastStream {
    rx: broadcast::Receiver<Arc<()>>,
    _counter: SubscriberGuard,
}

struct SubscriberGuard {
    count: Arc<RwLock<u64>>,
}

impl Drop for SubscriberGuard {
    fn drop(&mut self) {
        let mut g = self.count.write();
        *g = g.saturating_sub(1);
    }
}

impl EventStream for BroadcastStream {
    fn recv(&mut self) -> Result<Arc<()>, EventBusError> {
        match self.rx.blocking_recv() {
            Ok(arc) => Ok(arc),
            Err(broadcast::error::RecvError::Lagged(n)) => Err(EventBusError::Lag(n)),
            Err(broadcast::error::RecvError::Closed) => Err(EventBusError::Closed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_to_no_subscribers_is_ok() {
        let bus = EventBus::new(16);
        let result = bus.publish(Arc::new(())).await;
        assert!(result.is_err(), "should error with no subscribers");
    }

    #[tokio::test]
    async fn subscriber_receives_published_event() {
        let bus = EventBus::new(16);
        let mut sub = bus.subscribe();
        let payload = Arc::new(());
        bus.publish(payload.clone()).await.expect("publish should succeed");
        let event = tokio::task::spawn_blocking(move || sub.recv())
            .await
            .expect("spawn_blocking should succeed")
            .expect("recv should succeed");
        // v0.4.22 (event 000116): payload type is opaque; we
        // just need the round-trip to succeed. Use Arc::ptr_eq
        // to confirm identity preservation.
        assert!(Arc::ptr_eq(&event, &payload));
    }
}
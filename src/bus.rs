//! Message bus module for inter-component communication.

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;

/// Inbound message from channels to agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub channel: String,
    pub sender_id: String,
    pub chat_id: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<Vec<String>>,
    pub session_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

/// Outbound message from agent to channels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub channel: String,
    pub chat_id: String,
    pub content: String,
}

#[derive(Debug, Error)]
pub enum BusError {
    #[error("message bus is closed")]
    Closed,
    #[error("inbound receiver already taken")]
    InboundReceiverTaken,
    #[error("outbound receiver already taken")]
    OutboundReceiverTaken,
    #[error("inbound channel send failed")]
    InboundSendFailed,
    #[error("outbound channel send failed")]
    OutboundSendFailed,
}

/// Message bus for channel <-> agent communication.
#[derive(Clone)]
pub struct MessageBus {
    inner: Arc<MessageBusInner>,
}

struct MessageBusInner {
    inbound_tx: mpsc::Sender<InboundMessage>,
    outbound_tx: mpsc::Sender<OutboundMessage>,
    inbound_rx: Mutex<Option<mpsc::Receiver<InboundMessage>>>,
    outbound_rx: Mutex<Option<mpsc::Receiver<OutboundMessage>>>,
    closed: RwLock<bool>,
}

impl MessageBus {
    /// Create a new message bus.
    pub fn new() -> Self {
        let (inbound_tx, inbound_rx) = mpsc::channel(100);
        let (outbound_tx, outbound_rx) = mpsc::channel(100);

        Self {
            inner: Arc::new(MessageBusInner {
                inbound_tx,
                outbound_tx,
                inbound_rx: Mutex::new(Some(inbound_rx)),
                outbound_rx: Mutex::new(Some(outbound_rx)),
                closed: RwLock::new(false),
            }),
        }
    }

    /// Publish an inbound message asynchronously.
    pub async fn publish_inbound(&self, msg: InboundMessage) -> Result<(), BusError> {
        if *self.inner.closed.read() {
            return Err(BusError::Closed);
        }
        self.inner
            .inbound_tx
            .send(msg)
            .await
            .map_err(|_| BusError::InboundSendFailed)
    }

    /// Publish an outbound message asynchronously.
    pub async fn publish_outbound(&self, msg: OutboundMessage) -> Result<(), BusError> {
        if *self.inner.closed.read() {
            return Err(BusError::Closed);
        }
        self.inner
            .outbound_tx
            .send(msg)
            .await
            .map_err(|_| BusError::OutboundSendFailed)
    }

    /// Take the inbound receiver exactly once.
    pub fn take_inbound_receiver(&self) -> Result<mpsc::Receiver<InboundMessage>, BusError> {
        self.inner
            .inbound_rx
            .lock()
            .take()
            .ok_or(BusError::InboundReceiverTaken)
    }

    /// Take the outbound receiver exactly once.
    pub fn take_outbound_receiver(&self) -> Result<mpsc::Receiver<OutboundMessage>, BusError> {
        self.inner
            .outbound_rx
            .lock()
            .take()
            .ok_or(BusError::OutboundReceiverTaken)
    }

    /// Close the message bus.
    pub fn close(&self) {
        *self.inner.closed.write() = true;
    }
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inbound() -> InboundMessage {
        InboundMessage {
            channel: "test".to_string(),
            sender_id: "user".to_string(),
            chat_id: "chat".to_string(),
            content: "hello".to_string(),
            media: None,
            session_key: "test:chat".to_string(),
            metadata: None,
        }
    }

    fn outbound() -> OutboundMessage {
        OutboundMessage {
            channel: "test".to_string(),
            chat_id: "chat".to_string(),
            content: "world".to_string(),
        }
    }

    #[tokio::test]
    async fn publish_and_consume_inbound() {
        let bus = MessageBus::new();
        let mut rx = bus.take_inbound_receiver().expect("receiver should exist");
        bus.publish_inbound(inbound())
            .await
            .expect("send should succeed");
        let got = rx.recv().await.expect("message should arrive");
        assert_eq!(got.content, "hello");
    }

    #[tokio::test]
    async fn publish_and_consume_outbound() {
        let bus = MessageBus::new();
        let mut rx = bus.take_outbound_receiver().expect("receiver should exist");
        bus.publish_outbound(outbound())
            .await
            .expect("send should succeed");
        let got = rx.recv().await.expect("message should arrive");
        assert_eq!(got.content, "world");
    }

    #[test]
    fn taking_receiver_twice_fails() {
        let bus = MessageBus::new();
        let _ = bus
            .take_inbound_receiver()
            .expect("first take should succeed");
        let err = bus
            .take_inbound_receiver()
            .expect_err("second take should fail");
        assert!(matches!(err, BusError::InboundReceiverTaken));

        let _ = bus
            .take_outbound_receiver()
            .expect("first take should succeed");
        let err = bus
            .take_outbound_receiver()
            .expect_err("second take should fail");
        assert!(matches!(err, BusError::OutboundReceiverTaken));
    }
}

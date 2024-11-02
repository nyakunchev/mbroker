use {
    dashmap::DashMap,
    serde::{Deserialize, Serialize},
    std::{
        sync::Arc,
        time::Duration,
    },
    tokio::time::Instant,
    uuid::Uuid,
};


#[derive(Serialize, Clone, Debug)]
pub struct Message {
    pub message_id: Uuid,
    pub data: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MessageContent {
    pub key: Option<String>,
    pub data: String,
}

#[derive(Clone)]
pub struct Subscriber {
    pub id: Uuid,
    pub sender: tokio::sync::mpsc::UnboundedSender<Message>,
    pub pending_messages: Arc<DashMap<Uuid, PendingMessage>>,
}

#[derive(Clone)]
pub struct PendingMessage {
    pub sent_at: Instant,
}

#[derive(Deserialize)]
pub struct CreateTopicRequest {
    pub compaction_enabled: bool,
    pub retention_duration_secs: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CommitRequest {
    pub message_id: Uuid,
}

pub struct Topic {
    pub subscribers: Arc<DashMap<Uuid, Subscriber>>,
    pub messages: Arc<DashMap<Uuid, MessageEntry>>,
    pub retention_duration: Option<Duration>,
    pub compaction_enabled: bool,
}

#[derive(Clone)]
pub struct MessageEntry {
    pub message: Message,
    pub timestamp: Instant,
}

impl Topic {
    pub fn new(compaction_enabled: bool, retention_duration: Option<Duration>) -> Self {
        Self {
            subscribers: Arc::new(DashMap::new()),
            messages: Arc::new(DashMap::new()),
            retention_duration,
            compaction_enabled,
        }
    }
}

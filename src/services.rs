use {
    crate::models::{Message, MessageContent, PendingMessage, Subscriber, Topic},
    dashmap::DashMap,
    log::{error, info},
    std::{
        sync::Arc,
        time::Duration,
    },
    tokio::time::{interval, Instant},
    uuid::Uuid,
};


pub type Topics = Arc<DashMap<String, Topic>>;

pub fn create_topic(
    topics: &Topics,
    topic_name: String,
    compaction_enabled: bool,
    retention_duration_secs: Option<u64>,
) -> Result<(), String> {
    if topics.contains_key(&topic_name) {
        error!("Topic {} already exists", topic_name);
        return Err("Topic already exists".to_string());
    }

    let retention_duration = retention_duration_secs.map(Duration::from_secs);
    let new_topic = Topic::new(compaction_enabled, retention_duration);
    topics.insert(topic_name.clone(), new_topic);
    info!("Topic {} created", topic_name);

    Ok(())
}

pub fn publish_message(
    topics: &Topics,
    topic_name: String,
    message_content: MessageContent,
) -> Result<(), String> {
    if let Some(topic) = topics.get(&topic_name) {
        let topic = topic.value();
        let message_id = if let Some(key) = message_content.key {
            if topic.compaction_enabled {
                Uuid::new_v5(&Uuid::NAMESPACE_DNS, key.as_bytes())
            } else {
                Uuid::new_v4()
            }
        } else {
            Uuid::new_v4()
        };

        let new_message_entry = crate::models::MessageEntry {
            message: Message {
                message_id,
                data: message_content.data,
            },
            timestamp: Instant::now(),
        };

        topic.messages.insert(message_id, new_message_entry.clone());
        info!("Message {} published to topic {}", message_id, topic_name);

        let subscribers = topic
            .subscribers
            .iter()
            .map(|entry| entry.value().clone())
            .collect::<Vec<_>>();
        for subscriber in subscribers {
            subscriber.pending_messages.insert(
                message_id,
                PendingMessage {
                    sent_at: Instant::now(),
                },
            );

            start_resend_task(
                subscriber.clone(),
                message_id,
                new_message_entry.message.clone(),
            );
        }

        Ok(())
    } else {
        error!("Topic {} not found", topic_name);
        Err("Topic not found".to_string())
    }
}

pub fn clean_up_topics(topics: &Topics) {
    let now = Instant::now();
    topics.iter().for_each(|entry| {
        let topic = entry.value();
        if let Some(duration) = topic.retention_duration {
            topic
                .messages
                .retain(|_, entry| now.duration_since(entry.timestamp) < duration);
        }
        topic.subscribers.iter().for_each(|subscriber_entry| {
            let subscriber = subscriber_entry.value();
            subscriber.pending_messages.retain(|_, pending| {
                now.duration_since(pending.sent_at) < Duration::from_secs(300)
            });
        });
    });
}

pub fn start_resend_task(subscriber: Subscriber, message_id: Uuid, message: Message) {
    let sender_clone = subscriber.sender.clone();
    let pending_messages = subscriber.pending_messages.clone();
    let subscriber_id = subscriber.id;
    tokio::spawn(async move {
        let mut resend_interval = interval(Duration::from_secs(5));
        loop {
            resend_interval.tick().await;
            if pending_messages.contains_key(&message_id) {
                if sender_clone.send(message.clone()).is_err() {
                    error!(
                        "Failed to resend message {} to subscriber {}",
                        message_id, subscriber_id
                    );
                }
            } else {
                break;
            }
        }
    });
}

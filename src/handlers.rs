use {
    crate::{
        models::*,
        services::{create_topic, publish_message, start_resend_task}
    },
    actix_web::{
        cookie::Cookie,
        web::{self, Bytes, Data, Path},
        Error, HttpRequest, HttpResponse, Responder,
    },
    dashmap::DashMap,
    log::{error, info},
    std::sync::Arc,
    tokio::{
        sync::mpsc,
        time::Instant,
    },
    tokio_stream::StreamExt,
    uuid::Uuid,
};


type Topics = Arc<DashMap<String, Topic>>;

pub async fn create_topic_handler(
    topics: Data<Topics>,
    path: Path<String>,
    req: web::Json<CreateTopicRequest>,
) -> impl Responder {
    let topic_name = path.into_inner();
    let create_req = req.into_inner();

    match create_topic(
        &topics,
        topic_name,
        create_req.compaction_enabled,
        create_req.retention_duration_secs,
    ) {
        Ok(_) => HttpResponse::Created().body("Topic created"),
        Err(err) => HttpResponse::Conflict().body(err),
    }
}

pub async fn subscribe_handler(
    topics: Data<Topics>,
    path: Path<String>,
) -> Result<HttpResponse, Error> {
    let topic_name = path.into_inner();

    if let Some(topic) = topics.get(&topic_name) {
        let topic = topic.value();
        let (tx, rx) = mpsc::unbounded_channel();

        let subscriber = Subscriber {
            id: Uuid::new_v4(),
            sender: tx,
            pending_messages: Arc::new(DashMap::new()),
        };

        topic.subscribers.insert(subscriber.id, subscriber.clone());
        info!("Subscriber {} added to topic {}", subscriber.id, topic_name);

        for entry in topic.messages.iter() {
            let message_id = *entry.key();
            let message = entry.value().message.clone();

            if let Err(e) = subscriber.sender.send(message.clone()) {
                error!(
                    "Failed to send message to subscriber {}: {}",
                    subscriber.id, e
                );
            }
            subscriber.pending_messages.insert(
                message_id,
                PendingMessage {
                    sent_at: Instant::now(),
                },
            );

            start_resend_task(subscriber.clone(), message_id, message.clone());
        }

        let stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
        let sse_stream = stream.map(|message| {
            let json = serde_json::to_string(&message).map_err(|e| {
                error!("Failed to serialize message: {}", e);

                actix_web::error::ErrorInternalServerError("Failed to serialize message")
            })?;

            Ok::<Bytes, Error>(Bytes::from(format!("data: {}\n\n", json)))
        });

        let cookie = Cookie::build("subscriber_id", subscriber.id.to_string())
            .path("/")
            .http_only(true)
            .finish();

        Ok(HttpResponse::Ok()
            .cookie(cookie)
            .append_header(("Content-Type", "text/event-stream"))
            .streaming(sse_stream))
    } else {
        error!("Topic {} not found", topic_name);

        Ok(HttpResponse::NotFound().body("Topic not found"))
    }
}

pub async fn unsubscribe_handler(
    req: HttpRequest,
    topics: Data<Topics>,
    path: Path<String>,
) -> impl Responder {
    let topic_name = path.into_inner();

    let subscriber_id = match extract_subscriber_id(&req) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if let Some(topic) = topics.get(&topic_name) {
        let topic = topic.value();
        if topic.subscribers.remove(&subscriber_id).is_some() {
            info!(
                "Subscriber {} removed from topic {}",
                subscriber_id, topic_name
            );

            let mut cookie = Cookie::build("subscriber_id", "")
                .path("/")
                .http_only(true)
                .finish();
            cookie.make_removal();

            HttpResponse::Ok().cookie(cookie).body("Unsubscribed")
        } else {
            error!(
                "Subscriber {} not found in topic {}",
                subscriber_id, topic_name
            );

            HttpResponse::NotFound().body("Subscriber not found")
        }
    } else {
        error!("Topic {} not found", topic_name);

        HttpResponse::NotFound().body("Topic not found")
    }
}

pub async fn publish_handler(
    topics: Data<Topics>,
    path: Path<String>,
    msg: web::Json<MessageContent>,
) -> impl Responder {
    let topic_name = path.into_inner();
    let message_content = msg.into_inner();

    match publish_message(&topics, topic_name, message_content) {
        Ok(_) => HttpResponse::Ok().body("Message published"),
        Err(err) => HttpResponse::NotFound().body(err),
    }
}

pub async fn commit_handler(
    req: HttpRequest,
    topics: Data<Topics>,
    path: Path<String>,
    msg: web::Json<CommitRequest>,
) -> impl Responder {
    let topic_name = path.into_inner();
    let commit_request = msg.into_inner();

    let subscriber_id = match extract_subscriber_id(&req) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if let Some(topic) = topics.get(&topic_name) {
        let topic = topic.value();
        if let Some(subscriber) = topic.subscribers.get(&subscriber_id) {
            let subscriber = subscriber.value();
            if subscriber
                .pending_messages
                .remove(&commit_request.message_id)
                .is_some()
            {
                info!(
                    "Message {} committed by subscriber {}",
                    commit_request.message_id, subscriber_id
                );

                HttpResponse::Ok().body("Message committed")
            } else {
                error!(
                    "Message {} not found in pending messages for subscriber {}",
                    commit_request.message_id, subscriber_id
                );

                HttpResponse::NotFound().body("Message not found in pending messages")
            }
        } else {
            error!(
                "Subscriber {} not found in topic {}",
                subscriber_id, topic_name
            );

            HttpResponse::NotFound().body("Subscriber not found")
        }
    } else {
        error!("Topic {} not found", topic_name);

        HttpResponse::NotFound().body("Topic not found")
    }
}

fn extract_subscriber_id(req: &HttpRequest) -> Result<Uuid, HttpResponse> {
    match req.cookie("subscriber_id") {
        Some(cookie) => match Uuid::parse_str(cookie.value()) {
            Ok(id) => Ok(id),
            Err(_) => {
                error!("Invalid subscriber ID in cookie");

                Err(HttpResponse::BadRequest().body("Invalid subscriber ID in cookie"))
            }
        },
        None => {
            error!("No subscriber ID found in cookie");

            Err(HttpResponse::BadRequest().body("No subscriber ID found in cookie"))
        }
    }
}

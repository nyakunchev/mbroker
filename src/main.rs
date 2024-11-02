mod handlers;
mod models;
mod services;

use {
    crate::services::{clean_up_topics, Topics},
    actix_web::{
        web::{self, Data},
        App, HttpServer,
    },
    dashmap::DashMap,
    std::sync::Arc,
};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    let topics: Topics = Arc::new(DashMap::new());

    let topics_clone = topics.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            clean_up_topics(&topics_clone);
        }
    });

    HttpServer::new(move || {
        App::new()
            .app_data(Data::new(topics.clone()))
            .service(
                web::resource("/topics/{topic_name}")
                    .route(web::post().to(handlers::create_topic_handler)),
            )
            .service(
                web::resource("/topics/{topic_name}/subscribe")
                    .route(web::get().to(handlers::subscribe_handler)),
            )
            .service(
                web::resource("/topics/{topic_name}/unsubscribe")
                    .route(web::post().to(handlers::unsubscribe_handler)),
            )
            .service(
                web::resource("/topics/{topic_name}/publish")
                    .route(web::post().to(handlers::publish_handler)),
            )
            .service(
                web::resource("/topics/{topic_name}/commit")
                    .route(web::post().to(handlers::commit_handler)),
            )
    })
    .bind(("127.0.0.1", 1337))?
    .run()
    .await
}

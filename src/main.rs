mod config;
mod github;
mod handlers;
mod platform;

use actix_web::{web, App, HttpServer};
use dotenvy::dotenv;
use log::info;

use crate::config::AppState;
use crate::handlers::{download::download_asset, update::check_update};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();

    let address = std::env::var("ADDRESS").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let bind_address = format!("{}:{}", address, port);

    let app_state = AppState::load_config().await;

    info!(
        "Starting the multi-product update server on {}",
        &bind_address
    );

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .service(check_update)
            .service(download_asset)
    })
    .bind(&bind_address)?
    .run()
    .await
}

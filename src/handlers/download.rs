use actix_web::{get, web, Error, HttpResponse};
use log::error;

use crate::config::AppState;
use crate::github::client::GitHubClient;

#[get("/{product_name}/download/{asset_id}/{filename}")]
pub async fn download_asset(
    path: web::Path<(String, u64, String)>,
    data: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let (product_name, asset_id, filename) = path.into_inner();

    let products = data.products.read().await;
    let product_config = match products.get(&product_name.to_lowercase()) {
        Some(config) => config.clone(),
        None => {
            error!("Product {} not found in configuration", product_name);
            return Ok(HttpResponse::NotFound().finish());
        }
    };

    let github = GitHubClient::new(product_config.github_token)?;

    match github
        .download_asset(
            asset_id,
            &product_config.repo_owner,
            &product_config.repo_name,
        )
        .await
    {
        Ok(bytes) => Ok(HttpResponse::Ok()
            .append_header((
                "Content-Disposition",
                format!("attachment; filename={}", filename),
            ))
            .body(bytes)),
        Err(e) => {
            error!("Failed to download asset: {}", e);
            Err(actix_web::error::ErrorInternalServerError(
                "Failed to download asset",
            ))
        }
    }
}

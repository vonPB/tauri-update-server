use actix_web::{get, web, Error, HttpResponse};
use log::{debug, error};
use semver::Version;
use serde::Serialize;

use crate::config::AppState;
use crate::github::client::GitHubClient;
use crate::platform::matcher::{Platform, PlatformMatcher};

#[derive(Serialize)]
pub struct UpdateResponse {
    version: String,
    pub_date: String,
    url: String,
    signature: String,
    notes: String,
}

#[get("/{product_name}/{feature}/{target}/{arch}/{current_version}")]
pub async fn check_update(
    path: web::Path<(String, String, String, String, String)>,
    data: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let (product_name, feature, target, arch, current_version) = path.into_inner();

    debug!(
        "Checking for update for product {}, feature {}, target {}, arch {}, current version {}",
        product_name, feature, target, arch, current_version
    );

    // Get product configuration
    let products = data.products.read().await;
    let product_config = match products.get(&product_name.to_lowercase()) {
        Some(config) => config.clone(),
        None => {
            error!("Product {} not found in configuration", product_name);
            return Ok(HttpResponse::NotFound().finish());
        }
    };

    // Create GitHub client
    let github = GitHubClient::new(product_config.github_token)?;

    // Fetch latest release
    let release = github
        .get_latest_release(&product_config.repo_owner, &product_config.repo_name)
        .await?;

    // Parse versions and compare
    let latest_version = Version::parse(release.tag_name.trim_start_matches('v')).map_err(|e| {
        error!("Failed to parse latest version: {}", e);
        actix_web::error::ErrorInternalServerError("Invalid version format")
    })?;
    let current_version = Version::parse(&current_version).unwrap();

    if latest_version > current_version {
        let platform = Platform { target, arch };

        let matcher = PlatformMatcher::new();
        let assets: Vec<String> = release
            .assets
            .iter()
            .map(|asset| asset.name.clone())
            .collect();

        let asset_match = matcher.find_matching_asset(&platform, &assets, Some(&feature))?;

        let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string());

        // Find asset ID for the installer
        let asset_id = release
            .assets
            .iter()
            .find(|a| a.name == asset_match.filename)
            .map(|a| a.id.0)
            .ok_or_else(|| actix_web::error::ErrorInternalServerError("Asset not found"))?;

        let url = format!(
            "{}/{}/download/{}/{}",
            hostname, product_name, asset_id, asset_match.filename
        );

        let signature = if let Some(sig_filename) = asset_match.signature_filename.clone() {
            let sig_asset = release
                .assets
                .iter()
                .find(|a| a.name == sig_filename)
                .ok_or_else(|| actix_web::error::ErrorInternalServerError("Signature not found"))?;

            let sig_bytes = github
                .download_asset(
                    sig_asset.id.0,
                    &product_config.repo_owner,
                    &product_config.repo_name,
                )
                .await?;

            String::from_utf8(sig_bytes.to_vec())
                .unwrap_or_else(|_| "Failed to read signature".to_string())
        } else {
            return Err(actix_web::error::ErrorInternalServerError(
                "No signature file found",
            ));
        };

        debug!(
            "Found signature file: {}",
            asset_match.signature_filename.unwrap_or_default()
        );
        debug!("Signature length: {}", signature.len());

        let update_response = UpdateResponse {
            version: latest_version.to_string(),
            pub_date: release.published_at.unwrap().to_rfc3339(),
            url,
            signature,
            notes: release.body.unwrap_or_default(),
        };

        Ok(HttpResponse::Ok().json(update_response))
    } else {
        Ok(HttpResponse::NoContent().finish())
    }
}

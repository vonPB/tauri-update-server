use actix_web::{get, web, Error, HttpResponse};
use log::{debug, error, trace};
use semver::Version;
use serde::Serialize;

use std::sync::Arc;
use std::time::Instant;

use crate::config::{AppState, CachedValue, ProductConfig};
use crate::download_token::create_download_token;
use crate::github::client::GitHubClient;
use crate::platform::matcher::{Platform, PlatformMatcher};

const DOWNLOAD_TOKEN_TTL_MINUTES: u64 = 30;

#[derive(Serialize)]
pub struct UpdateResponse {
    version: String,
    pub_date: String,
    url: String,
    signature: String,
    notes: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_current_version() {
        assert!(parse_current_version("not-a-version").is_err());
    }

    #[test]
    fn rejects_invalid_signature_bytes() {
        assert!(read_signature(bytes::Bytes::from_static(&[0xff])).is_err());
    }

    #[test]
    fn rejects_missing_publication_date() {
        assert!(require_pub_date(None).is_err());
    }
}

fn parse_current_version(current_version: &str) -> Result<Version, Error> {
    Version::parse(current_version).map_err(|e| {
        error!("Failed to parse current version: {}", e);
        actix_web::error::ErrorBadRequest("Invalid current version format")
    })
}

fn read_signature(sig_bytes: bytes::Bytes) -> Result<String, Error> {
    String::from_utf8(sig_bytes.to_vec()).map_err(|e| {
        error!("Failed to read signature: {}", e);
        actix_web::error::ErrorInternalServerError("Invalid signature encoding")
    })
}

fn require_pub_date(pub_date: Option<String>) -> Result<String, Error> {
    pub_date.ok_or_else(|| {
        error!("Latest release is missing published_at");
        actix_web::error::ErrorInternalServerError("Release publication date missing")
    })
}

async fn get_cached_value<K, V>(
    cache: &tokio::sync::RwLock<std::collections::HashMap<K, CachedValue<V>>>,
    key: &K,
    cache_ttl: std::time::Duration,
) -> Option<Arc<V>>
where
    K: Eq + std::hash::Hash,
{
    if cache_ttl.is_zero() {
        return None;
    }

    cache.read().await.get(key).and_then(|cached| {
        if cached.fetched_at.elapsed() < cache_ttl {
            Some(cached.value.clone())
        } else {
            None
        }
    })
}

async fn set_cached_value<K, V>(
    cache: &tokio::sync::RwLock<std::collections::HashMap<K, CachedValue<V>>>,
    key: K,
    value: Arc<V>,
    cache_ttl: std::time::Duration,
) where
    K: Eq + std::hash::Hash,
{
    if cache_ttl.is_zero() {
        return;
    }

    cache.write().await.insert(
        key,
        CachedValue {
            value,
            fetched_at: Instant::now(),
        },
    );
}

async fn get_latest_release(
    data: &web::Data<AppState>,
    github: &GitHubClient,
    product_name: &str,
    product_config: &ProductConfig,
) -> Result<Arc<octocrab::models::repos::Release>, Error> {
    let cache_key = product_name.to_lowercase();
    let cache_ttl = data.github_release_cache_ttl;

    if let Some(release) = get_cached_value(&data.release_cache, &cache_key, cache_ttl).await {
        debug!("Using cached GitHub release for product {}", product_name);
        return Ok(release);
    }

    let release = Arc::new(
        github
            .get_latest_release(&product_config.repo_owner, &product_config.repo_name)
            .await?,
    );

    set_cached_value(&data.release_cache, cache_key, release.clone(), cache_ttl).await;

    Ok(release)
}

async fn get_signature(
    data: &web::Data<AppState>,
    github: &GitHubClient,
    signature_asset_id: u64,
    product_config: &ProductConfig,
) -> Result<Arc<String>, Error> {
    let cache_ttl = data.github_release_cache_ttl;

    if let Some(signature) =
        get_cached_value(&data.signature_cache, &signature_asset_id, cache_ttl).await
    {
        debug!("Using cached signature for asset {}", signature_asset_id);
        return Ok(signature);
    }

    let sig_bytes = github
        .download_asset(
            signature_asset_id,
            &product_config.repo_owner,
            &product_config.repo_name,
        )
        .await?;
    let signature = Arc::new(read_signature(sig_bytes)?);

    set_cached_value(
        &data.signature_cache,
        signature_asset_id,
        signature.clone(),
        cache_ttl,
    )
    .await;

    Ok(signature)
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

    let current_version = parse_current_version(&current_version)?;

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
    let github = GitHubClient::new(product_config.github_token.clone())?;

    // Fetch latest release
    let release = get_latest_release(&data, &github, &product_name, &product_config).await?;

    // Parse versions and compare
    let latest_version = Version::parse(release.tag_name.trim_start_matches('v')).map_err(|e| {
        error!("Failed to parse latest version: {}", e);
        actix_web::error::ErrorInternalServerError("Invalid version format")
    })?;

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

        let download_token = create_download_token(
            &data.download_token_secret,
            &product_name,
            asset_id,
            &asset_match.filename,
            DOWNLOAD_TOKEN_TTL_MINUTES,
        )
        .map_err(|e| {
            error!("Failed to create download token: {}", e);
            actix_web::error::ErrorInternalServerError("Failed to create download token")
        })?;

        let url = format!(
            "{}/{}/download/{}/{}",
            hostname, product_name, download_token, asset_match.filename
        );

        let signature = if let Some(sig_filename) = asset_match.signature_filename.clone() {
            let sig_asset = release
                .assets
                .iter()
                .find(|a| a.name == sig_filename)
                .ok_or_else(|| actix_web::error::ErrorInternalServerError("Signature not found"))?;

            get_signature(&data, &github, sig_asset.id.0, &product_config)
                .await?
                .as_ref()
                .clone()
        } else {
            return Err(actix_web::error::ErrorInternalServerError(
                "No signature file found",
            ));
        };

        trace!(
            "Found signature file: {}",
            asset_match.signature_filename.unwrap_or_default()
        );
        trace!("Signature length: {}", signature.len());

        let update_response = UpdateResponse {
            version: latest_version.to_string(),
            pub_date: require_pub_date(
                release.published_at.as_ref().map(|date| date.to_rfc3339()),
            )?,
            url,
            signature,
            notes: release.body.clone().unwrap_or_default(),
        };

        Ok(HttpResponse::Ok().json(update_response))
    } else {
        Ok(HttpResponse::NoContent().finish())
    }
}

use actix_web::{get, web, App, Error, HttpResponse, HttpServer};
use dotenvy::dotenv;
use log::{error, info};
use octocrab::Octocrab;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, sync::Arc};
use tokio::sync::RwLock;

#[derive(Clone, Deserialize)]
struct ProductConfig {
    github_token: String,
    repo_owner: String,
    repo_name: String,
}

#[derive(Clone)]
struct AppState {
    products: Arc<RwLock<HashMap<String, ProductConfig>>>,
}

#[derive(Serialize)]
struct UpdateResponse {
    version: String,
    pub_date: String,
    url: String,
    signature: String,
    notes: String,
}

impl AppState {
    async fn load_config() -> Self {
        let mut products = HashMap::new();

        // Load from environment variables with a pattern:
        // PRODUCT_NAME_TOKEN, PRODUCT_NAME_OWNER, PRODUCT_NAME_REPO
        let env_vars: HashMap<String, String> = env::vars().collect();

        for (key, value) in env_vars.iter() {
            if key.ends_with("_TOKEN") {
                let product_name = key.trim_end_matches("_TOKEN").to_lowercase();

                let owner_key = format!("{}_OWNER", product_name.to_uppercase());
                let repo_key = format!("{}_REPO", product_name.to_uppercase());

                if let (Some(owner), Some(repo)) =
                    (env_vars.get(&owner_key), env_vars.get(&repo_key))
                {
                    products.insert(
                        product_name,
                        ProductConfig {
                            github_token: value.clone(),
                            repo_owner: owner.clone(),
                            repo_name: repo.clone(),
                        },
                    );
                }
            }
        }

        AppState {
            products: Arc::new(RwLock::new(products)),
        }
    }
}

#[get("/")]
async fn home() -> Result<HttpResponse, Error> {
    Ok(HttpResponse::Ok().body("Multi-Product Update Server"))
}

#[get("/{product_name}/{feature}/{target}/{arch}/{current_version}")]
async fn check_update(
    path: web::Path<(String, String, String, String, String)>,
    data: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let (product_name, feature, _target, _arch, current_version) = path.into_inner();

    info!(
        "Checking for update for product {}, feature {}, current version {}",
        product_name, feature, current_version
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

    // Create octocrab instance for this product
    let octocrab = create_octocrab_instance(&product_config.github_token)?;

    // Fetch the latest release
    let release = fetch_latest_release(
        &octocrab,
        &product_config.repo_owner,
        &product_config.repo_name,
    )
    .await
    .map_err(|e| actix_web::error::ErrorInternalServerError(e))?;

    // Extract the latest version and compare with current version
    let latest_version = Version::parse(release.tag_name.trim_start_matches('v')).unwrap();
    let current_version = Version::parse(&current_version).unwrap();

    if latest_version > current_version {
        let signature = download_asset_content(
            &product_config.repo_owner,
            &product_config.repo_name,
            &release,
            &feature,
            ".sig",
            &product_config.github_token,
        )
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(e))?;

        let installer_filename = find_asset_filename(&release, &feature, ".msi")?;

        let hostname = env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string());

        let url = format!(
            "{}/{}/download/{}/{}",
            hostname,
            product_name,
            find_asset_id_by_filename(&release, &installer_filename)?,
            installer_filename
        );

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

#[get("/{product_name}/download/{asset_id}/{filename}")]
async fn download_asset(
    path: web::Path<(String, u64, String)>,
    data: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let (product_name, asset_id, filename) = path.into_inner();

    info!(
        "Downloading asset for product {}, id {} and filename {}",
        product_name, asset_id, filename
    );

    let products = data.products.read().await;
    let product_config = match products.get(&product_name.to_lowercase()) {
        Some(config) => config.clone(),
        None => {
            error!("Product {} not found in configuration", product_name);
            return Ok(HttpResponse::NotFound().finish());
        }
    };

    let client = reqwest::Client::new();
    let download_url = format!(
        "https://api.github.com/repos/{}/{}/releases/assets/{}",
        product_config.repo_owner, product_config.repo_name, asset_id
    );

    match download_from_github(&client, &download_url, &product_config.github_token).await {
        Ok(bytes) => Ok(HttpResponse::Ok()
            .append_header((
                "Content-Disposition",
                format!("attachment; filename={}", filename),
            ))
            .body(bytes)),
        Err(e) => {
            error!("Failed to download asset: {}", e);
            Err(actix_web::error::ErrorInternalServerError(
                "Internal Server Error",
            ))
        }
    }
}

// Helper functions modified to accept GitHub token as parameter
fn create_octocrab_instance(github_token: &str) -> Result<Octocrab, actix_web::Error> {
    Octocrab::builder()
        .personal_token(github_token.to_string())
        .build()
        .map_err(|e| {
            error!("Failed to build Octocrab instance: {}", e);
            actix_web::error::ErrorInternalServerError("Internal Server Error")
        })
}

async fn fetch_latest_release(
    octocrab: &Octocrab,
    repo_owner: &str,
    repo_name: &str,
) -> Result<octocrab::models::repos::Release, actix_web::Error> {
    octocrab
        .repos(repo_owner, repo_name)
        .releases()
        .get_latest()
        .await
        .map_err(|e| {
            error!("Failed to fetch latest release: {}", e);
            actix_web::error::ErrorInternalServerError("Internal Server Error")
        })
}

async fn download_asset_content(
    repo_owner: &str,
    repo_name: &str,
    release: &octocrab::models::repos::Release,
    feature: &str,
    extension: &str,
    github_token: &str,
) -> Result<String, actix_web::Error> {
    let asset_id = find_asset_id(release, feature, extension)?;
    let client = reqwest::Client::new();
    let download_url = format!(
        "https://api.github.com/repos/{}/{}/releases/assets/{}",
        repo_owner, repo_name, asset_id
    );

    download_from_github(&client, &download_url, github_token)
        .await
        .map(|bytes| {
            String::from_utf8(bytes.to_vec())
                .unwrap_or_else(|_| "Failed to convert bytes to string".to_string())
        })
        .map_err(|e| actix_web::error::ErrorInternalServerError(e))
}

// Reuse existing helper functions
fn find_asset_id(
    release: &octocrab::models::repos::Release,
    feature: &str,
    extension: &str,
) -> Result<u64, actix_web::Error> {
    release
        .assets
        .iter()
        .find(|asset| {
            asset.name.to_lowercase().contains(&feature.to_lowercase())
                && asset.name.ends_with(extension)
        })
        .map(|asset| asset.id.0)
        .ok_or_else(|| {
            error!(
                "Asset file not found for feature: {} and extension: {}",
                feature, extension
            );
            actix_web::error::ErrorInternalServerError("Asset file not found")
        })
}

fn find_asset_filename(
    release: &octocrab::models::repos::Release,
    feature: &str,
    extension: &str,
) -> Result<String, actix_web::Error> {
    release
        .assets
        .iter()
        .find(|asset| {
            asset.name.to_lowercase().contains(&feature.to_lowercase())
                && asset.name.ends_with(extension)
        })
        .map(|asset| asset.name.clone())
        .ok_or_else(|| {
            error!(
                "Asset file not found for feature: {} and extension: {}",
                feature, extension
            );
            actix_web::error::ErrorInternalServerError("Asset file not found")
        })
}

fn find_asset_id_by_filename(
    release: &octocrab::models::repos::Release,
    filename: &str,
) -> Result<u64, actix_web::Error> {
    release
        .assets
        .iter()
        .find(|asset| asset.name == filename)
        .map(|asset| asset.id.0)
        .ok_or_else(|| {
            error!("Asset file not found for filename: {}", filename);
            actix_web::error::ErrorInternalServerError("Asset file not found")
        })
}

async fn download_from_github(
    client: &reqwest::Client,
    url: &str,
    github_token: &str,
) -> Result<bytes::Bytes, actix_web::Error> {
    client
        .get(url)
        .header("Authorization", format!("Bearer {}", github_token))
        .header("Accept", "application/octet-stream")
        .header("User-Agent", "Multi-Product-Update-Server")
        .send()
        .await
        .map_err(|e| {
            error!("Failed to send request to GitHub: {}", e);
            actix_web::error::ErrorInternalServerError("Failed to send request to GitHub")
        })?
        .bytes()
        .await
        .map_err(|e| {
            error!("Failed to read response from GitHub: {}", e);
            actix_web::error::ErrorInternalServerError("Failed to read response from GitHub")
        })
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();

    let address = env::var("ADDRESS").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let bind_address = format!("{}:{}", address, port);

    let app_state = AppState::load_config().await;

    println!(
        "Starting the multi-product update server on {}",
        &bind_address
    );
    log::info!(
        "Starting the multi-product update server on {}",
        &bind_address
    );

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .service(check_update)
            .service(download_asset)
            .service(home)
    })
    .bind(&bind_address)?
    .run()
    .await
}

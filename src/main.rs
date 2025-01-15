use actix_web::{get, web, App, Error, HttpResponse, HttpServer};
use dotenvy::dotenv;
use lazy_static::lazy_static;
use log::{error, info};
use octocrab::Octocrab;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, env, sync::Arc};
use tokio::sync::RwLock;

// Platform-specific extension mappings
lazy_static! {
    static ref PLATFORM_EXTENSIONS: Vec<PlatformExtension> = vec![
        PlatformExtension {
            target: "windows",
            arch: "x86_64",
            extension: "x64_en-US.msi",
            sig_extension: "x64_en-US.msi.sig",
        },
        PlatformExtension {
            target: "windows",
            arch: "i686",
            extension: "x86_en-US.msi",
            sig_extension: "x86_en-US.msi.sig",
        },
        PlatformExtension {
            target: "darwin",
            arch: "x86_64",
            extension: "x64.app.tar.gz",
            sig_extension: "x64.app.tar.gz.sig",
        },
        PlatformExtension {
            target: "darwin",
            arch: "aarch64",
            extension: "aarch64.app.tar.gz",
            sig_extension: "aarch64.app.tar.gz.sig",
        },
        PlatformExtension {
            target: "linux",
            arch: "x86_64",
            extension: "amd64.AppImage",
            sig_extension: "amd64.AppImage.sig",
        },
    ];
}

struct PlatformExtension {
    target: &'static str,
    arch: &'static str,
    extension: &'static str,
    sig_extension: &'static str,
}

#[derive(Clone, Debug, Deserialize)]
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
    let (product_name, feature, target, arch, current_version) = path.into_inner();

    info!(
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

    // Create octocrab instance
    let octocrab = create_octocrab_instance(&product_config.github_token)?;

    // Fetch the latest release
    let release = fetch_latest_release(
        &octocrab,
        &product_config.repo_owner,
        &product_config.repo_name,
    )
    .await
    .map_err(actix_web::error::ErrorInternalServerError)?;

    // Parse versions and compare
    let latest_version = Version::parse(release.tag_name.trim_start_matches('v')).map_err(|e| {
        error!("Failed to parse latest version: {}", e);
        actix_web::error::ErrorInternalServerError("Failed to parse latest version")
    })?;
    let current_version = Version::parse(&current_version).unwrap();

    if latest_version > current_version {
        // Find the correct platform extension mapping
        let platform_ext = PLATFORM_EXTENSIONS
            .iter()
            .find(|p| p.target == target && p.arch == arch)
            .ok_or_else(|| {
                error!(
                    "No platform mapping found for target {} and arch {}",
                    target, arch
                );
                actix_web::error::ErrorInternalServerError("Unsupported platform/architecture")
            })?;

        // Find the installer and signature files
        let signature = download_asset_content(
            &product_config.repo_owner,
            &product_config.repo_name,
            &release,
            &feature,
            platform_ext.sig_extension,
            &product_config.github_token,
        )
        .await
        .map_err(actix_web::error::ErrorInternalServerError)?;

        let installer_filename =
            find_asset_filename(&release, &feature, platform_ext.extension, &target, &arch)?;

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

fn find_asset_filename(
    release: &octocrab::models::repos::Release,
    feature: &str,
    extension: &str,
    target: &str,
    arch: &str,
) -> Result<String, actix_web::Error> {
    let feature = if feature == "stable" {
        "".to_string()
    } else {
        feature.to_string()
    };

    release
        .assets
        .iter()
        .find(|asset| {
            let name_lower = asset.name.to_lowercase();
            let matches_feature =
                feature.is_empty() || name_lower.contains(&feature.to_lowercase());
            let matches_extension = asset.name.ends_with(extension);

            // Additional platform-specific checks
            let matches_platform = match target {
                "windows" => name_lower.contains("x64") || name_lower.contains("x86"),
                "darwin" => name_lower.contains("dmg") || name_lower.contains("app.tar.gz"),
                "linux" => {
                    name_lower.contains("appimage")
                        || name_lower.contains("deb")
                        || name_lower.contains("rpm")
                }
                _ => false,
            };

            matches_feature && matches_extension && matches_platform
        })
        .map(|asset| asset.name.clone())
        .ok_or_else(|| {
            error!(
                "Asset file not found for feature: {}, target: {}, arch: {}, extension: {}",
                feature, target, arch, extension
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
        .map_err(actix_web::error::ErrorInternalServerError)
}

fn find_asset_id(
    release: &octocrab::models::repos::Release,
    feature: &str,
    extension: &str,
) -> Result<u64, actix_web::Error> {
    let feature = if feature == "stable" {
        "".to_string()
    } else {
        feature.to_string()
    };

    release
        .assets
        .iter()
        .find(|asset| {
            let name_lower = asset.name.to_lowercase();
            (feature.is_empty() || name_lower.contains(&feature.to_lowercase()))
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

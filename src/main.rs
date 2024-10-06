use actix_web::{get, web, App, Error, HttpResponse, HttpServer, Responder};
use dotenvy::dotenv;
use log::{error, info};
use octocrab::Octocrab;
use semver::Version;
use serde::Serialize;
use std::env;

#[derive(Serialize)]
struct UpdateResponse {
    version: String,
    pub_date: String,
    url: String,
    signature: String,
    notes: String,
}

#[get("/")]
async fn home() -> Result<HttpResponse, actix_web::error::Error> {
    Ok(HttpResponse::Ok().body("FAS Update Server"))
}

#[get("/{feature}/{target}/{arch}/{current_version}")]
async fn check_update(
    path: web::Path<(String, String, String, String)>,
) -> Result<HttpResponse, Error> {
    let (feature, _target, _arch, current_version) = path.into_inner();

    info!(
        "Checking for update for feature {}, current version {}",
        feature, current_version
    );

    // Load environment variables securely from .env
    dotenv().ok();

    // Create octocrab instance
    let octocrab = create_octocrab_instance()?;

    // Fetch the latest release
    let repo_owner = env::var("GITHUB_REPO_OWNER").unwrap();
    let repo_name = env::var("GITHUB_REPO_NAME").unwrap();
    let release = fetch_latest_release(&octocrab, &repo_owner, &repo_name)
        .await
        .map_err(|e| actix_web::error::ErrorInternalServerError(e))?;

    // Extract the latest version and compare with current version
    let latest_version = Version::parse(release.tag_name.trim_start_matches('v')).unwrap();
    let current_version = Version::parse(&current_version).unwrap();

    if latest_version > current_version {
        let signature = download_asset_content(&repo_owner, &repo_name, &release, &feature, ".sig")
            .await
            .map_err(|e| actix_web::error::ErrorInternalServerError(e))?;
        let installer_filename = find_asset_filename(&release, &feature, ".msi")?;

        let hostname = env::var("HOSTNAME").unwrap_or("localhost".to_string());

        let url = format!(
            "{}/download/{}/{}",
            hostname,
            find_asset_id_by_filename(&release, &installer_filename)?,
            installer_filename
        );

        let update_response = UpdateResponse {
            version: latest_version.to_string(),
            pub_date: release.published_at.unwrap().to_rfc3339(),
            url,
            signature,
            notes: String::from(""), // Notes are empty for now
        };

        Ok(HttpResponse::Ok().json(update_response))
    } else {
        Ok(HttpResponse::NoContent().finish())
    }
}

#[get("/download/{asset_id}/{filename}")]
async fn download_asset(
    path: web::Path<(u64, String)>,
) -> Result<HttpResponse, actix_web::error::Error> {
    let (asset_id, filename) = path.into_inner();

    info!(
        "Downloading asset with id {} and filename {}",
        asset_id, filename
    );

    let client = reqwest::Client::new();

    // Load environment variables securely from .env
    dotenv().ok();

    let repo_owner = env::var("GITHUB_REPO_OWNER").unwrap();
    let repo_name = env::var("GITHUB_REPO_NAME").unwrap();
    let download_url = format!(
        "https://api.github.com/repos/{}/{}/releases/assets/{}",
        repo_owner, repo_name, asset_id
    );

    match download_from_github(&client, &download_url).await {
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

fn create_octocrab_instance() -> Result<Octocrab, actix_web::Error> {
    Octocrab::builder()
        .personal_token(env::var("GITHUB_TOKEN").unwrap())
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
) -> Result<octocrab::models::repos::Release, actix_web::error::Error> {
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
) -> Result<String, actix_web::error::Error> {
    let asset_id = find_asset_id(release, feature, extension)?;
    let client = reqwest::Client::new();
    let download_url = format!(
        "https://api.github.com/repos/{}/{}/releases/assets/{}",
        repo_owner, repo_name, asset_id
    );

    download_from_github(&client, &download_url)
        .await
        .map(|bytes| {
            String::from_utf8(bytes.to_vec())
                .unwrap_or_else(|_| "Failed to convert bytes to string".to_string())
        })
        .map_err(|e| actix_web::error::ErrorInternalServerError(e))
}

fn find_asset_id(
    release: &octocrab::models::repos::Release,
    feature: &str,
    extension: &str,
) -> Result<u64, actix_web::error::Error> {
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
) -> Result<String, actix_web::error::Error> {
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
) -> Result<u64, actix_web::error::Error> {
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
) -> Result<bytes::Bytes, actix_web::error::Error> {
    client
        .get(url)
        .header(
            "Authorization",
            format!("Bearer {}", env::var("GITHUB_TOKEN").unwrap()),
        )
        .header("Accept", "application/octet-stream")
        .header("User-Agent", "Tauri-Update-Server")
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
    info!("Starting the Tauri Update Server...");

    let ip = env::var("ADDRESS").unwrap();
    let port = env::var("PORT").unwrap();

    // Start HTTP server
    HttpServer::new(|| {
        App::new()
            // Serve the update check endpoint
            .service(check_update)
            .service(download_asset)
            .service(home)
    })
    .bind(format!("{}:{}", ip, port))?
    .workers(2)
    .run()
    .await
}

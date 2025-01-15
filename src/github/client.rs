use actix_web::Error;
use bytes::Bytes;
use log::{error, info};
use octocrab::Octocrab;
use reqwest;
use reqwest::Url;

pub struct GitHubClient {
    octocrab: Octocrab,
    github_token: String,
}

impl GitHubClient {
    pub fn new(github_token: String) -> Result<Self, Error> {
        let octocrab = Octocrab::builder()
            .personal_token(github_token.clone())
            .build()
            .map_err(|e| {
                error!("Failed to build Octocrab instance: {}", e);
                actix_web::error::ErrorInternalServerError("Failed to create GitHub client")
            })?;

        Ok(Self {
            octocrab,
            github_token,
        })
    }

    pub async fn get_latest_release(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<octocrab::models::repos::Release, Error> {
        self.octocrab
            .repos(owner, repo)
            .releases()
            .get_latest()
            .await
            .map_err(|e| {
                error!("Failed to fetch latest release: {}", e);
                actix_web::error::ErrorInternalServerError("Failed to fetch release")
            })
    }

    pub async fn download_asset(
        &self,
        asset_id: u64,
        owner: &str,
        repo: &str,
    ) -> Result<Bytes, Error> {
        let client = reqwest::Client::new();
        let url = format!(
            "https://api.github.com/repos/{}/{}/releases/assets/{}",
            owner, repo, asset_id
        );

        info!("Downloading asset from GitHub API URL: {}", url);

        let response = client
            .get(url)
            .header("Authorization", format!("Bearer {}", self.github_token))
            .header("Accept", "application/octet-stream")
            .header("User-Agent", "Multi-Product-Update-Server")
            .send()
            .await
            .map_err(|e| {
                error!("Failed to send request to GitHub: {}", e);
                actix_web::error::ErrorInternalServerError("Failed to download asset")
            })?;

        if !response.status().is_success() {
            error!(
                "GitHub API returned error status: {} for asset ID: {}",
                response.status(),
                asset_id
            );
            return Err(actix_web::error::ErrorInternalServerError(
                "GitHub API error",
            ));
        }

        response.bytes().await.map_err(|e| {
            error!("Failed to read response from GitHub: {}", e);
            actix_web::error::ErrorInternalServerError("Failed to read asset")
        })
    }
}

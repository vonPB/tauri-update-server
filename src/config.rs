use serde::Deserialize;
use std::{
    collections::HashMap,
    env,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;

const DEFAULT_GITHUB_RELEASE_CACHE_TTL_SECONDS: u64 = 60;

#[derive(Clone, Debug, Deserialize)]
pub struct ProductConfig {
    pub github_token: String,
    pub repo_owner: String,
    pub repo_name: String,
}

pub struct CachedValue<T> {
    pub value: Arc<T>,
    pub fetched_at: Instant,
}

#[derive(Clone)]
pub struct AppState {
    pub products: Arc<RwLock<HashMap<String, ProductConfig>>>,
    pub download_token_secret: String,
    pub release_cache: Arc<RwLock<HashMap<String, CachedValue<octocrab::models::repos::Release>>>>,
    pub signature_cache: Arc<RwLock<HashMap<u64, CachedValue<String>>>>,
    pub github_release_cache_ttl: Duration,
}

impl AppState {
    pub async fn load_config() -> Self {
        let env_vars: HashMap<String, String> = env::vars().collect();
        Self::load_config_from_env(env_vars)
    }

    fn load_config_from_env(env_vars: HashMap<String, String>) -> Self {
        let mut products = HashMap::new();
        let download_token_secret = env_vars
            .get("DOWNLOAD_TOKEN_SECRET")
            .expect("DOWNLOAD_TOKEN_SECRET must be set")
            .clone();
        let github_release_cache_ttl = env_vars
            .get("GITHUB_RELEASE_CACHE_TTL_SECONDS")
            .map(|value| {
                value
                    .parse::<u64>()
                    .expect("GITHUB_RELEASE_CACHE_TTL_SECONDS must be a number")
            })
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(DEFAULT_GITHUB_RELEASE_CACHE_TTL_SECONDS));

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
            download_token_secret,
            release_cache: Arc::new(RwLock::new(HashMap::new())),
            signature_cache: Arc::new(RwLock::new(HashMap::new())),
            github_release_cache_ttl,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_download_token_secret_from_env() {
        let mut env_vars = HashMap::new();
        env_vars.insert("MYAPP_TOKEN".to_string(), "github-token".to_string());
        env_vars.insert("MYAPP_OWNER".to_string(), "owner".to_string());
        env_vars.insert("MYAPP_REPO".to_string(), "repo".to_string());
        env_vars.insert(
            "DOWNLOAD_TOKEN_SECRET".to_string(),
            "download-secret".to_string(),
        );

        let state = AppState::load_config_from_env(env_vars);

        assert_eq!(state.download_token_secret, "download-secret");
    }
}

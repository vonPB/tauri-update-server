use serde::Deserialize;
use std::{collections::HashMap, env, sync::Arc};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Deserialize)]
pub struct ProductConfig {
    pub github_token: String,
    pub repo_owner: String,
    pub repo_name: String,
}

#[derive(Clone)]
pub struct AppState {
    pub products: Arc<RwLock<HashMap<String, ProductConfig>>>,
    pub download_token_secret: String,
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
        env_vars.insert("DOWNLOAD_TOKEN_SECRET".to_string(), "download-secret".to_string());

        let state = AppState::load_config_from_env(env_vars);

        assert_eq!(state.download_token_secret, "download-secret");
    }
}

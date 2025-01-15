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
}

impl AppState {
    pub async fn load_config() -> Self {
        let mut products = HashMap::new();
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

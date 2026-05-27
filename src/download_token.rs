use jsonwebtoken::{
    decode, encode, Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation,
};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadClaims {
    pub product_name: String,
    pub asset_id: u64,
    pub filename: String,
    pub exp: usize,
}

pub fn create_download_token(
    secret: &str,
    product_name: &str,
    asset_id: u64,
    filename: &str,
    ttl_minutes: u64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let claims = DownloadClaims {
        product_name: product_name.to_string(),
        asset_id,
        filename: filename.to_string(),
        exp: expires_at(ttl_minutes),
    };

    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

pub fn verify_download_token(
    secret: &str,
    token: &str,
    product_name: &str,
    filename: &str,
) -> Result<DownloadClaims, jsonwebtoken::errors::Error> {
    let token_data: TokenData<DownloadClaims> = decode(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )?;

    let claims = token_data.claims;
    if claims.product_name != product_name || claims.filename != filename {
        return Err(jsonwebtoken::errors::Error::from(
            jsonwebtoken::errors::ErrorKind::InvalidToken,
        ));
    }

    Ok(claims)
}

fn expires_at(ttl_minutes: u64) -> usize {
    let expiry = SystemTime::now() + Duration::from_secs(ttl_minutes * 60);
    expiry
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_matching_download_token() {
        let token = create_download_token(
            "secret",
            "myapp",
            42,
            "myapp_1.0.0_x64.msi",
            30,
        )
        .unwrap();

        let claims = verify_download_token("secret", &token, "myapp", "myapp_1.0.0_x64.msi")
            .unwrap();

        assert_eq!(claims.asset_id, 42);
    }

    #[test]
    fn rejects_token_for_different_filename() {
        let token = create_download_token(
            "secret",
            "myapp",
            42,
            "myapp_1.0.0_x64.msi",
            30,
        )
        .unwrap();

        assert!(verify_download_token("secret", &token, "myapp", "other.msi").is_err());
    }
}

use actix_web::{get, http::header, web, Error, HttpResponse};
use futures_util::TryStreamExt;
use log::error;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

use crate::config::AppState;
use crate::download_token::verify_download_token;
use crate::github::client::GitHubClient;

// RFC 5987 filename* values allow a limited attr-char set.
// Encode separators and delimiters so asset names cannot alter the header.
const RFC5987_FILENAME_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'%')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b',')
    .add(b'/')
    .add(b':')
    .add(b';')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'{')
    .add(b'}');

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_download_filename_for_content_disposition() {
        let header = content_disposition("release \"x64\".msi");

        assert_eq!(
            header,
            "attachment; filename*=UTF-8''release%20%22x64%22.msi"
        );
    }
}

#[get("/{product_name}/download/{download_token}/{filename}")]
pub async fn download_asset(
    path: web::Path<(String, String, String)>,
    data: web::Data<AppState>,
) -> Result<HttpResponse, Error> {
    let (product_name, download_token, filename) = path.into_inner();

    let products = data.products.read().await;
    let product_config = match products.get(&product_name.to_lowercase()) {
        Some(config) => config.clone(),
        None => {
            error!("Product {} not found in configuration", product_name);
            return Ok(HttpResponse::NotFound().finish());
        }
    };

    let claims = verify_download_token(
        &data.download_token_secret,
        &download_token,
        &product_name,
        &filename,
    )
    .map_err(|e| {
        error!("Invalid download token: {}", e);
        actix_web::error::ErrorUnauthorized("Invalid download token")
    })?;

    let github = GitHubClient::new(product_config.github_token)?;

    match github
        .download_asset_response(
            claims.asset_id,
            &product_config.repo_owner,
            &product_config.repo_name,
        )
        .await
    {
        Ok(response) => Ok(HttpResponse::Ok()
            .append_header((header::CONTENT_DISPOSITION, content_disposition(&filename)))
            .streaming(response.bytes_stream().map_err(|e| {
                error!("Failed to stream asset from GitHub: {}", e);
                actix_web::error::ErrorInternalServerError("Failed to stream asset")
            }))),
        Err(e) => {
            error!("Failed to download asset: {}", e);
            Err(actix_web::error::ErrorInternalServerError(
                "Failed to download asset",
            ))
        }
    }
}

fn content_disposition(filename: &str) -> String {
    format!(
        "attachment; filename*=UTF-8''{}",
        utf8_percent_encode(filename, RFC5987_FILENAME_ENCODE_SET)
    )
}

use std::fmt;

use actix_web::http::StatusCode;
use actix_web::ResponseError;
use log::{error, info};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Platform {
    pub target: String,
    pub arch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetMatch {
    pub filename: String,
    pub signature_filename: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum MatchError {
    #[error("No matching asset found for {target} {arch}")]
    NoMatch { target: String, arch: String },
    #[error("No matching signature found for {0}")]
    NoSignature(String),
}

impl ResponseError for MatchError {
    fn status_code(&self) -> StatusCode {
        match self {
            MatchError::NoMatch { .. } => StatusCode::NOT_FOUND,
            MatchError::NoSignature(_) => StatusCode::NOT_FOUND,
        }
    }

    fn error_response(&self) -> actix_web::HttpResponse {
        actix_web::HttpResponse::build(self.status_code())
            .content_type("text/plain")
            .body(self.to_string())
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.target, self.arch)
    }
}

pub struct PlatformMatcher {
    rules: Vec<Box<dyn MatchRule>>,
}

pub trait MatchRule: Send + Sync {
    fn matches(&self, platform: &Platform, filename: &str) -> bool;
    fn get_signature_extension(&self) -> &str;
}

// Windows MSI Rule
pub struct WindowsMsiRule;
impl MatchRule for WindowsMsiRule {
    fn matches(&self, platform: &Platform, filename: &str) -> bool {
        if platform.target != "windows" {
            return false;
        }

        let filename_lower = filename.to_lowercase();
        match platform.arch.as_str() {
            "x86_64" => filename_lower.contains("_x64") && filename_lower.ends_with(".msi"),
            "i686" => filename_lower.contains("_x86") && filename_lower.ends_with(".msi"),
            _ => false,
        }
    }

    fn get_signature_extension(&self) -> &str {
        ".msi.sig"
    }
}

// macOS Rule
pub struct MacOSRule;
impl MatchRule for MacOSRule {
    fn matches(&self, platform: &Platform, filename: &str) -> bool {
        if platform.target != "darwin" {
            return false;
        }

        let filename_lower = filename.to_lowercase();
        let arch_match = match platform.arch.as_str() {
            "x86_64" => filename_lower.contains("_x64"),
            "aarch64" => filename_lower.contains("aarch64"),
            _ => false,
        };

        arch_match && (filename_lower.ends_with(".app.tar.gz") || filename_lower.ends_with(".dmg"))
    }

    fn get_signature_extension(&self) -> &str {
        ".sig"
    }
}

// Linux Rule
pub struct LinuxRule;
impl MatchRule for LinuxRule {
    fn matches(&self, platform: &Platform, filename: &str) -> bool {
        if platform.target != "linux" {
            return false;
        }

        let filename_lower = filename.to_lowercase();
        platform.arch == "x86_64"
            && filename_lower.contains("amd64")
            && filename_lower.ends_with(".appimage")
    }

    fn get_signature_extension(&self) -> &str {
        ".sig"
    }
}

impl PlatformMatcher {
    pub fn new() -> Self {
        let rules: Vec<Box<dyn MatchRule>> = vec![
            Box::new(WindowsMsiRule),
            Box::new(MacOSRule),
            Box::new(LinuxRule),
        ];
        PlatformMatcher { rules }
    }

    pub fn find_matching_asset(
        &self,
        platform: &Platform,
        assets: &[String],
        feature: Option<&str>,
    ) -> Result<AssetMatch, MatchError> {
        let feature_prefix = feature.map(|f| {
            if f.eq_ignore_ascii_case("stable") {
                String::new()
            } else {
                format!("{}.", f.to_uppercase())
            }
        });

        // Debug log all assets
        info!("Available assets: {:?}", assets);
        if let Some(prefix) = &feature_prefix {
            info!("Looking for feature prefix: {}", prefix);
        }

        // Find matching installer
        let matching_asset = assets
            .iter()
            .find(|asset| {
                let passes_feature = match &feature_prefix {
                    Some(prefix) if !prefix.is_empty() => asset.starts_with(prefix),
                    _ => true,
                };

                passes_feature && self.rules.iter().any(|rule| rule.matches(platform, asset))
            })
            .ok_or_else(|| MatchError::NoMatch {
                target: platform.target.clone(),
                arch: platform.arch.clone(),
            })?;

        // Look for exact signature match
        let signature_filename = format!("{}.sig", matching_asset);
        let signature = if assets.contains(&signature_filename) {
            Some(signature_filename)
        } else {
            error!("No signature file found for {}", matching_asset);
            info!("Expected signature file: {}", signature_filename);
            None
        };

        Ok(AssetMatch {
            filename: matching_asset.clone(),
            signature_filename: signature,
        })
    }
}

#[test]
fn test_windows_msi_matching() {
    let matcher = PlatformMatcher::new();
    let platform = Platform {
        target: "windows".to_string(),
        arch: "x86_64".to_string(),
    };

    let assets = vec![
        "FAS2.Lumina_2.0.11_x64_de-DE.msi".to_string(),
        "FAS2.Lumina_2.0.11_x64_de-DE.msi.sig".to_string(),
        "FAS2.Lumina_2.0.11_x64-setup.exe".to_string(),
        "FAS2.Lumina_2.0.11_x64-setup.exe.sig".to_string(),
    ];

    let result = matcher
        .find_matching_asset(&platform, &assets, Some("fas2"))
        .unwrap();
    assert_eq!(result.filename, "FAS2.Lumina_2.0.11_x64_de-DE.msi");
    assert_eq!(
        result.signature_filename,
        Some("FAS2.Lumina_2.0.11_x64_de-DE.msi.sig".to_string())
    );
}

#[test]
fn test_stable_feature_matching() {
    let matcher = PlatformMatcher::new();
    let platform = Platform {
        target: "windows".to_string(),
        arch: "x86_64".to_string(),
    };

    let assets = vec![
        "KWALIS.-.Naturland_1.2.0_x64_en-US.msi".to_string(),
        "KWALIS.-.Naturland_1.2.0_x64_en-US.msi.sig".to_string(),
    ];

    let result = matcher
        .find_matching_asset(&platform, &assets, Some("stable"))
        .unwrap();
    assert_eq!(result.filename, "KWALIS.-.Naturland_1.2.0_x64_en-US.msi");
}

#[test]
fn test_macos_matching() {
    let matcher = PlatformMatcher::new();
    let platform = Platform {
        target: "darwin".to_string(),
        arch: "aarch64".to_string(),
    };

    let assets = vec![
        "KWALIS.-.Naturland_1.2.0_aarch64.app.tar.gz".to_string(),
        "KWALIS.-.Naturland_1.2.0_aarch64.app.tar.gz.sig".to_string(),
        "KWALIS.-.Naturland_1.2.0_x64.app.tar.gz".to_string(),
    ];

    let result = matcher
        .find_matching_asset(&platform, &assets, None)
        .unwrap();
    assert_eq!(
        result.filename,
        "KWALIS.-.Naturland_1.2.0_aarch64.app.tar.gz"
    );
}

#[test]
fn test_linux_matching() {
    let matcher = PlatformMatcher::new();
    let platform = Platform {
        target: "linux".to_string(),
        arch: "x86_64".to_string(),
    };

    let assets = vec![
        "KWALIS.-.Naturland_1.2.0_amd64.AppImage".to_string(),
        "KWALIS.-.Naturland_1.2.0_amd64.AppImage.sig".to_string(),
    ];

    let result = matcher
        .find_matching_asset(&platform, &assets, None)
        .unwrap();
    assert_eq!(result.filename, "KWALIS.-.Naturland_1.2.0_amd64.AppImage");
}

#[test]
fn test_no_matching_asset() {
    let matcher = PlatformMatcher::new();
    let platform = Platform {
        target: "windows".to_string(),
        arch: "x86_64".to_string(),
    };

    let assets = vec!["KWALIS.-.Naturland_1.2.0_aarch64.app.tar.gz".to_string()];

    assert!(matcher
        .find_matching_asset(&platform, &assets, None)
        .is_err());
}

#[test]
fn test_feature_mismatch() {
    let matcher = PlatformMatcher::new();
    let platform = Platform {
        target: "windows".to_string(),
        arch: "x86_64".to_string(),
    };

    let assets = vec![
        "FAS1.Lumina_2.0.11_x64_de-DE.msi".to_string(),
        "FAS1.Lumina_2.0.11_x64_de-DE.msi.sig".to_string(),
    ];

    assert!(matcher
        .find_matching_asset(&platform, &assets, Some("fas2"))
        .is_err());
}

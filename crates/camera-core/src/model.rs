use crate::CameraError;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

const ID_HEX_LENGTH: usize = 64;
pub const DEFAULT_MEDIA_PAGE_SIZE: u32 = 60;
pub const ABSOLUTE_MAX_MEDIA_PAGE_SIZE: u32 = 200;
pub const ABSOLUTE_MAX_THUMBNAIL_BYTES: u64 = 8 * 1024 * 1024;

macro_rules! opaque_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn parse(value: impl Into<String>) -> Result<Self, CameraError> {
                let value = value.into();
                let Some(digest) = value.strip_prefix($prefix) else {
                    return Err(CameraError::invalid_input(concat!(
                        "invalid ",
                        stringify!($name),
                        " prefix"
                    )));
                };
                if digest.len() != ID_HEX_LENGTH
                    || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(CameraError::invalid_input(concat!(
                        "invalid ",
                        stringify!($name),
                        " digest"
                    )));
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            fn from_digest(digest: impl AsRef<[u8]>) -> Self {
                Self(format!(concat!($prefix, "{}"), hex::encode(digest)))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

opaque_id!(CameraId, "camera_");
opaque_id!(AssetId, "asset_");
opaque_id!(CaptureGroupId, "capture_");

fn update_length_prefixed(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
}

impl CameraId {
    /// 在不暴露相机序列号的情况下构建稳定身份。
    pub fn derive(manufacturer: &str, model: &str, serial_number: Option<&str>) -> Self {
        let mut hasher = Sha256::new();
        update_length_prefixed(&mut hasher, manufacturer.as_bytes());
        update_length_prefixed(&mut hasher, model.as_bytes());
        update_length_prefixed(
            &mut hasher,
            serial_number.unwrap_or("serial-unavailable").as_bytes(),
        );
        Self::from_digest(hasher.finalize())
    }
}

impl AssetId {
    /// 根据完整远端身份派生不可逆的公开 ID。
    ///
    /// `remote_object_key` 仅保留在驱动内部，绝不能复制到 DTO 中。
    /// 添加长度前缀可避免拼接歧义。
    pub fn derive(
        camera_id: &CameraId,
        storage_id: u32,
        remote_object_key: &[u8],
        size_bytes: u64,
        captured_at: Option<NaiveDateTime>,
    ) -> Self {
        let mut hasher = Sha256::new();
        update_length_prefixed(&mut hasher, camera_id.as_str().as_bytes());
        hasher.update(storage_id.to_le_bytes());
        update_length_prefixed(&mut hasher, remote_object_key);
        hasher.update(size_bytes.to_le_bytes());
        update_length_prefixed(
            &mut hasher,
            captured_at
                .map(|date| date.format("%Y%m%dT%H%M%S").to_string())
                .unwrap_or_default()
                .as_bytes(),
        );
        Self::from_digest(hasher.finalize())
    }
}

impl CaptureGroupId {
    pub fn derive(
        camera_id: &CameraId,
        storage_id: u32,
        filename_stem: &str,
        captured_at: Option<NaiveDateTime>,
    ) -> Self {
        let mut hasher = Sha256::new();
        update_length_prefixed(&mut hasher, camera_id.as_str().as_bytes());
        hasher.update(storage_id.to_le_bytes());
        update_length_prefixed(&mut hasher, filename_stem.as_bytes());
        update_length_prefixed(
            &mut hasher,
            captured_at
                .map(|date| date.format("%Y%m%dT%H%M%S").to_string())
                .unwrap_or_default()
                .as_bytes(),
        );
        Self::from_digest(hasher.finalize())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub client_name: String,
    pub client_id: Uuid,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
}

impl ConnectionConfig {
    pub fn validate(&self) -> Result<(), CameraError> {
        if self.host.trim().is_empty() {
            return Err(CameraError::invalid_input("camera host is empty"));
        }
        if self.port == 0 {
            return Err(CameraError::invalid_input("camera port must be non-zero"));
        }
        if self.client_name.trim().is_empty() || self.client_name.chars().count() > 64 {
            return Err(CameraError::invalid_input(
                "client name must contain 1 to 64 characters",
            ));
        }
        if self.connect_timeout.is_zero() || self.request_timeout.is_zero() {
            return Err(CameraError::invalid_input("timeouts must be non-zero"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CameraInfo {
    pub id: CameraId,
    pub manufacturer: String,
    pub model: String,
    pub display_name: String,
    pub serial_number: Option<String>,
    pub firmware_version: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaFormat {
    Jpeg,
    Nef,
}

impl MediaFormat {
    pub fn from_filename(filename: &str) -> Option<Self> {
        let extension = std::path::Path::new(filename)
            .extension()?
            .to_string_lossy();
        if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
            Some(Self::Jpeg)
        } else if extension.eq_ignore_ascii_case("nef") {
            Some(Self::Nef)
        } else {
            None
        }
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Nef => "nef",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CameraCapabilityProfile {
    pub profile_id: String,
    pub display_name: String,
    pub supported_formats: BTreeSet<MediaFormat>,
    pub supports_thumbnails: bool,
    pub supports_partial_downloads: bool,
    pub supports_events: bool,
    pub max_page_size: u32,
    pub max_thumbnail_bytes: u64,
}

impl CameraCapabilityProfile {
    pub fn validate(&self) -> Result<(), CameraError> {
        if self.profile_id.trim().is_empty() || self.display_name.trim().is_empty() {
            return Err(CameraError::invalid_input(
                "capability profile names must not be empty",
            ));
        }
        if self.max_page_size == 0 || self.max_page_size > ABSOLUTE_MAX_MEDIA_PAGE_SIZE {
            return Err(CameraError::invalid_input("invalid maximum page size"));
        }
        if self.max_thumbnail_bytes == 0 || self.max_thumbnail_bytes > ABSOLUTE_MAX_THUMBNAIL_BYTES
        {
            return Err(CameraError::invalid_input("invalid thumbnail byte limit"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CameraAsset {
    pub id: AssetId,
    pub capture_group_id: CaptureGroupId,
    pub filename: String,
    pub format: MediaFormat,
    pub size_bytes: u64,
    pub captured_at: Option<NaiveDateTime>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub thumbnail_available: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PageCursor(String);

impl PageCursor {
    pub fn parse(value: impl Into<String>) -> Result<Self, CameraError> {
        let value = value.into();
        if value.is_empty() || value.len() > 256 || !value.is_ascii() {
            return Err(CameraError::invalid_input("invalid media cursor"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MediaQuery {
    pub formats: BTreeSet<MediaFormat>,
    pub page_size: u32,
    pub cursor: Option<PageCursor>,
}

impl Default for MediaQuery {
    fn default() -> Self {
        Self {
            formats: BTreeSet::new(),
            page_size: DEFAULT_MEDIA_PAGE_SIZE,
            cursor: None,
        }
    }
}

impl MediaQuery {
    pub fn validate(&self, capabilities: &CameraCapabilityProfile) -> Result<(), CameraError> {
        if self.page_size == 0 || self.page_size > capabilities.max_page_size {
            return Err(CameraError::invalid_input(format!(
                "page size must be between 1 and {}",
                capabilities.max_page_size
            )));
        }
        if !self
            .formats
            .iter()
            .all(|format| capabilities.supported_formats.contains(format))
        {
            return Err(CameraError::invalid_input(
                "query contains an unsupported media format",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MediaPage {
    pub items: Vec<CameraAsset>,
    pub next_cursor: Option<PageCursor>,
    pub total_items: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ThumbnailRequest {
    pub asset_id: AssetId,
    pub max_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Thumbnail {
    pub mime_type: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DownloadRequest {
    pub asset_id: AssetId,
    pub destination_path: PathBuf,
    pub expected_size: u64,
    pub resume_partial: bool,
    pub overwrite: bool,
}

impl DownloadRequest {
    pub fn validate(&self) -> Result<(), CameraError> {
        if self.expected_size == 0 {
            return Err(CameraError::invalid_input(
                "expected download size must be non-zero",
            ));
        }
        if self.destination_path.file_name().is_none() {
            return Err(CameraError::invalid_input(
                "download destination must include a filename",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_identity_is_stable_and_does_not_expose_handle() {
        let camera = CameraId::derive("Nikon", "Z6 II", Some("serial"));
        let id = AssetId::derive(&camera, 7, &42_u32.to_le_bytes(), 1234, None);
        assert_eq!(
            id,
            AssetId::derive(&camera, 7, &42_u32.to_le_bytes(), 1234, None)
        );
        assert!(!id.as_str().contains("42"));
        assert_eq!(id.as_str().len(), "asset_".len() + ID_HEX_LENGTH);
    }

    #[test]
    fn identity_fields_are_unambiguous() {
        let camera = CameraId::derive("Nikon", "Z6 II", None);
        let first = AssetId::derive(&camera, 1, &[1, 2], 3, None);
        let second = AssetId::derive(&camera, 1, &[1], 0x0302, None);
        assert_ne!(first, second);
    }

    #[test]
    fn media_format_detection_is_case_insensitive() {
        assert_eq!(
            MediaFormat::from_filename("DSC_0001.NEF"),
            Some(MediaFormat::Nef)
        );
        assert_eq!(
            MediaFormat::from_filename("DSC_0001.JpEg"),
            Some(MediaFormat::Jpeg)
        );
        assert_eq!(MediaFormat::from_filename("clip.mov"), None);
    }
}

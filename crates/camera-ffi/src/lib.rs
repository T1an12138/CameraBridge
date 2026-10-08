//! `CameraBridge` 的精简 `UniFFI` 门面。
//!
//! 自动生成的绑定由 Swift actor 或 Kotlin 客户端包装。本 crate 仅导出稳定的
//! DTO 以及不透明的会话/任务对象；绝不导出 Nikon 对象句柄，也不接受任意目标路径。

use camera_core::{
    CameraDriver, CameraError, CameraSession, ConnectionConfig, DownloadRequest, MediaFormat,
    MediaQuery, PageCursor, ThumbnailRequest, TransferJob, TransferPhase,
};
use nikon_driver::NikonZ6IiDriver;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

uniffi::setup_scaffolding!();

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiMediaFormat {
    Jpeg,
    Nef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiTransferPhase {
    Queued,
    Downloading,
    Finalizing,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiErrorCode {
    InvalidInput,
    ConnectionFailed,
    Timeout,
    CameraBusy,
    CameraAsleep,
    UnsupportedFirmware,
    UnsupportedOperation,
    Protocol,
    ObjectNotFound,
    PermissionDenied,
    InsufficientSpace,
    Io,
    SessionInterrupted,
    Cancelled,
    Conflict,
    Internal,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiConnectionConfig {
    pub host: String,
    pub port: u16,
    pub client_name: String,
    pub client_id: String,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub staging_root: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiCameraInfo {
    pub id: String,
    pub manufacturer: String,
    pub model: String,
    pub display_name: String,
    pub firmware_version: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiCapabilityProfile {
    pub profile_id: String,
    pub display_name: String,
    pub supported_formats: Vec<FfiMediaFormat>,
    pub supports_thumbnails: bool,
    pub supports_partial_downloads: bool,
    pub supports_events: bool,
    pub max_page_size: u32,
    pub max_thumbnail_bytes: u64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiCameraAsset {
    pub id: String,
    pub capture_group_id: String,
    pub filename: String,
    pub format: FfiMediaFormat,
    pub size_bytes: u64,
    pub captured_at: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub thumbnail_available: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiMediaPage {
    pub items: Vec<FfiCameraAsset>,
    pub next_cursor: Option<String>,
    pub total_items: Option<u64>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiThumbnail {
    pub mime_type: String,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiTransferSnapshot {
    pub phase: FfiTransferPhase,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub destination_path: Option<String>,
    pub error_code: Option<FfiErrorCode>,
    pub error_message: Option<String>,
}

#[derive(Debug, Error, uniffi::Error)]
pub enum CameraFfiError {
    // `message` 与 Kotlin Throwable.message 冲突，使用跨平台安全的字段名。
    #[error("{reason}")]
    Failure { code: FfiErrorCode, reason: String },
}

#[derive(Debug, uniffi::Object)]
pub struct CameraCore {
    driver: NikonZ6IiDriver,
}

#[uniffi::export(async_runtime = "tokio")]
impl CameraCore {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            driver: NikonZ6IiDriver,
        })
    }

    pub async fn connect(
        &self,
        config: FfiConnectionConfig,
    ) -> Result<Arc<FfiCameraSession>, CameraFfiError> {
        let staging_root = validate_staging_root(&config.staging_root).await?;
        let client_id = Uuid::parse_str(&config.client_id)
            .map_err(|_| ffi_invalid("client_id must be a UUID"))?;
        let session = self
            .driver
            .connect(ConnectionConfig {
                host: config.host,
                port: config.port,
                client_name: config.client_name,
                client_id,
                connect_timeout: Duration::from_millis(config.connect_timeout_ms),
                request_timeout: Duration::from_millis(config.request_timeout_ms),
            })
            .await
            .map_err(CameraFfiError::from)?;
        Ok(Arc::new(FfiCameraSession {
            session,
            staging_root,
        }))
    }
}

#[derive(uniffi::Object)]
pub struct FfiCameraSession {
    session: Arc<dyn CameraSession>,
    staging_root: PathBuf,
}
impl std::fmt::Debug for FfiCameraSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FfiCameraSession")
            .field("staging_root", &self.staging_root)
            .finish_non_exhaustive()
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl FfiCameraSession {
    pub async fn info(&self) -> Result<FfiCameraInfo, CameraFfiError> {
        let value = self.session.info().await.map_err(CameraFfiError::from)?;
        Ok(FfiCameraInfo {
            id: value.id.to_string(),
            manufacturer: value.manufacturer,
            model: value.model,
            display_name: value.display_name,
            firmware_version: value.firmware_version,
        })
    }

    pub async fn capabilities(&self) -> Result<FfiCapabilityProfile, CameraFfiError> {
        let value = self
            .session
            .capabilities()
            .await
            .map_err(CameraFfiError::from)?;
        Ok(FfiCapabilityProfile {
            profile_id: value.profile_id,
            display_name: value.display_name,
            supported_formats: value
                .supported_formats
                .into_iter()
                .map(Into::into)
                .collect(),
            supports_thumbnails: value.supports_thumbnails,
            supports_partial_downloads: value.supports_partial_downloads,
            supports_events: value.supports_events,
            max_page_size: value.max_page_size,
            max_thumbnail_bytes: value.max_thumbnail_bytes,
        })
    }

    pub async fn list_media(
        &self,
        formats: Vec<FfiMediaFormat>,
        page_size: u32,
        cursor: Option<String>,
    ) -> Result<FfiMediaPage, CameraFfiError> {
        let cursor = cursor
            .map(PageCursor::parse)
            .transpose()
            .map_err(CameraFfiError::from)?;
        let page = self
            .session
            .list_media(MediaQuery {
                formats: formats.into_iter().map(Into::into).collect::<BTreeSet<_>>(),
                page_size,
                cursor,
            })
            .await
            .map_err(CameraFfiError::from)?;
        Ok(FfiMediaPage {
            items: page
                .items
                .into_iter()
                .map(|asset| FfiCameraAsset {
                    id: asset.id.to_string(),
                    capture_group_id: asset.capture_group_id.to_string(),
                    filename: asset.filename,
                    format: asset.format.into(),
                    size_bytes: asset.size_bytes,
                    captured_at: asset
                        .captured_at
                        .map(|date| date.format("%Y-%m-%dT%H:%M:%S").to_string()),
                    width: asset.width,
                    height: asset.height,
                    thumbnail_available: asset.thumbnail_available,
                })
                .collect(),
            next_cursor: page.next_cursor.map(|value| value.as_str().to_owned()),
            total_items: page.total_items,
        })
    }

    pub async fn load_thumbnail(
        &self,
        asset_id: String,
        max_bytes: u64,
    ) -> Result<FfiThumbnail, CameraFfiError> {
        let thumbnail = self
            .session
            .load_thumbnail(ThumbnailRequest {
                asset_id: camera_core::AssetId::parse(asset_id).map_err(CameraFfiError::from)?,
                max_bytes,
            })
            .await
            .map_err(CameraFfiError::from)?;
        Ok(FfiThumbnail {
            mime_type: thumbnail.mime_type,
            data: thumbnail.data,
        })
    }

    pub async fn start_download(
        &self,
        asset_id: String,
        filename: String,
        expected_size: u64,
        overwrite: bool,
    ) -> Result<Arc<FfiTransferJob>, CameraFfiError> {
        validate_filename(&filename)?;
        let job = self
            .session
            .start_download(DownloadRequest {
                asset_id: camera_core::AssetId::parse(asset_id).map_err(CameraFfiError::from)?,
                destination_path: self.staging_root.join(filename),
                expected_size,
                resume_partial: false,
                overwrite,
            })
            .await
            .map_err(CameraFfiError::from)?;
        Ok(Arc::new(FfiTransferJob { job }))
    }

    pub async fn disconnect(&self) -> Result<(), CameraFfiError> {
        self.session
            .disconnect()
            .await
            .map_err(CameraFfiError::from)
    }
}

#[derive(uniffi::Object)]
pub struct FfiTransferJob {
    job: Arc<dyn TransferJob>,
}

impl std::fmt::Debug for FfiTransferJob {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FfiTransferJob")
            .finish_non_exhaustive()
    }
}

#[uniffi::export(async_runtime = "tokio")]
impl FfiTransferJob {
    pub fn snapshot(&self) -> FfiTransferSnapshot {
        self.job.snapshot().into()
    }
    pub fn cancel(&self) {
        self.job.cancel();
    }
    pub async fn wait(&self) -> FfiTransferSnapshot {
        self.job.wait().await.into()
    }
}

impl From<MediaFormat> for FfiMediaFormat {
    fn from(value: MediaFormat) -> Self {
        match value {
            MediaFormat::Jpeg => Self::Jpeg,
            MediaFormat::Nef => Self::Nef,
        }
    }
}

impl From<FfiMediaFormat> for MediaFormat {
    fn from(value: FfiMediaFormat) -> Self {
        match value {
            FfiMediaFormat::Jpeg => Self::Jpeg,
            FfiMediaFormat::Nef => Self::Nef,
        }
    }
}

impl From<TransferPhase> for FfiTransferPhase {
    fn from(value: TransferPhase) -> Self {
        match value {
            TransferPhase::Queued => Self::Queued,
            TransferPhase::Downloading => Self::Downloading,
            TransferPhase::Finalizing => Self::Finalizing,
            TransferPhase::Completed => Self::Completed,
            TransferPhase::Cancelled => Self::Cancelled,
            TransferPhase::Failed => Self::Failed,
        }
    }
}

impl From<camera_core::TransferSnapshot> for FfiTransferSnapshot {
    fn from(value: camera_core::TransferSnapshot) -> Self {
        let (error_code, error_message) = value.error.map_or((None, None), |error| {
            (Some(error.code().into()), Some(error.to_string()))
        });
        Self {
            phase: value.phase.into(),
            bytes_transferred: value.bytes_transferred,
            total_bytes: value.total_bytes,
            destination_path: value
                .destination_path
                .map(|path| path.to_string_lossy().into_owned()),
            error_code,
            error_message,
        }
    }
}

impl From<camera_core::CameraErrorCode> for FfiErrorCode {
    fn from(value: camera_core::CameraErrorCode) -> Self {
        use camera_core::CameraErrorCode as Source;
        match value {
            Source::InvalidInput => Self::InvalidInput,
            Source::ConnectionFailed => Self::ConnectionFailed,
            Source::Timeout => Self::Timeout,
            Source::CameraBusy => Self::CameraBusy,
            Source::CameraAsleep => Self::CameraAsleep,
            Source::UnsupportedFirmware => Self::UnsupportedFirmware,
            Source::UnsupportedOperation => Self::UnsupportedOperation,
            Source::Protocol => Self::Protocol,
            Source::ObjectNotFound => Self::ObjectNotFound,
            Source::PermissionDenied => Self::PermissionDenied,
            Source::InsufficientSpace => Self::InsufficientSpace,
            Source::Io => Self::Io,
            Source::SessionInterrupted => Self::SessionInterrupted,
            Source::Cancelled => Self::Cancelled,
            Source::Conflict => Self::Conflict,
            Source::Internal => Self::Internal,
        }
    }
}

impl From<CameraError> for CameraFfiError {
    fn from(value: CameraError) -> Self {
        Self::Failure {
            code: value.code().into(),
            reason: value.to_string(),
        }
    }
}

fn ffi_invalid(message: impl Into<String>) -> CameraFfiError {
    CameraFfiError::Failure {
        code: FfiErrorCode::InvalidInput,
        reason: message.into(),
    }
}

async fn validate_staging_root(value: &str) -> Result<PathBuf, CameraFfiError> {
    if value.trim().is_empty() {
        return Err(ffi_invalid("staging_root is empty"));
    }
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(ffi_invalid("staging_root must be absolute"));
    }
    tokio::fs::create_dir_all(&path)
        .await
        .map_err(CameraError::from)
        .map_err(CameraFfiError::from)?;
    Ok(path)
}

fn validate_filename(value: &str) -> Result<(), CameraFfiError> {
    let path = Path::new(value);
    if value.is_empty()
        || value.contains('/')
        || value.contains('\\')
        || path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err(ffi_invalid("filename must be one safe path component"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames_are_confined_to_staging_root() {
        assert!(validate_filename("DSC_0001.NEF").is_ok());
        assert!(validate_filename("../photo.nef").is_err());
        assert!(validate_filename("folder/photo.nef").is_err());
    }
}

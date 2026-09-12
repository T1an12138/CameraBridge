//! 与品牌无关的相机领域类型及下载编排。
//!
//! 协议实现在驱动 crate 中。本 crate 的公开会话 API 特意不包含
//! Nikon 或 PTP 术语。

mod error;
mod model;
mod session;
mod transfer;

pub use error::{CameraError, CameraErrorCode, PermissionKind};
pub use model::{
    AssetId, CameraAsset, CameraCapabilityProfile, CameraId, CameraInfo, CaptureGroupId,
    ConnectionConfig, DownloadRequest, MediaFormat, MediaPage, MediaQuery, PageCursor, Thumbnail,
    ThumbnailRequest,
};
pub use session::{AssetSource, CameraDriver, CameraSession, StreamOutcome, StreamRequest};
pub use transfer::{
    CancellationToken, TransferJob, TransferPhase, TransferSnapshot, start_file_download,
};

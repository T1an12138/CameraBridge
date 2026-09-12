use crate::{
    AssetId, CameraCapabilityProfile, CameraError, CameraInfo, CancellationToken, ConnectionConfig,
    DownloadRequest, MediaPage, MediaQuery, Thumbnail, ThumbnailRequest, TransferJob,
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::io::AsyncWrite;

#[async_trait]
pub trait CameraDriver: Send + Sync {
    async fn connect(
        &self,
        config: ConnectionConfig,
    ) -> Result<Arc<dyn CameraSession>, CameraError>;
}

/// 已连接的相机。实现必须串行执行协议事务。
#[async_trait]
pub trait CameraSession: Send + Sync {
    async fn info(&self) -> Result<CameraInfo, CameraError>;

    async fn capabilities(&self) -> Result<CameraCapabilityProfile, CameraError>;

    async fn list_media(&self, query: MediaQuery) -> Result<MediaPage, CameraError>;

    async fn load_thumbnail(&self, request: ThumbnailRequest) -> Result<Thumbnail, CameraError>;

    async fn start_download(
        &self,
        request: DownloadRequest,
    ) -> Result<Arc<dyn TransferJob>, CameraError>;

    /// 对已经断开的会话再次执行断开操作也必须成功。
    async fn disconnect(&self) -> Result<(), CameraError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamRequest {
    pub asset_id: AssetId,
    pub offset: u64,
    pub expected_remaining_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamOutcome {
    pub bytes_written: u64,
    pub source_total_bytes: Option<u64>,
}

/// 协议驱动实现的精简流式传输契约。
///
/// 下载字节始终留在 Rust 内部：数据源直接写入提供的异步 writer，
/// 绝不跨 FFI 边界返回完整对象缓冲区。
#[async_trait]
pub trait AssetSource: Send + Sync {
    async fn stream_asset(
        &self,
        request: StreamRequest,
        writer: &mut (dyn AsyncWrite + Unpin + Send),
        cancellation: CancellationToken,
    ) -> Result<StreamOutcome, CameraError>;
}

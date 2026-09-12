use crate::{AssetSource, CameraError, DownloadRequest, StreamRequest};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::{Notify, watch};

const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: AtomicBool,
    notify: Notify,
}

/// 可克隆的取消令牌，`cancel` 操作具备幂等性。
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    state: Arc<CancellationState>,
}

impl CancellationToken {
    /// 仅当本次调用让令牌转为已取消状态时才返回 true。
    pub fn cancel(&self) -> bool {
        let was_cancelled = self.state.cancelled.swap(true, Ordering::AcqRel);
        if !was_cancelled {
            self.state.notify.notify_waiters();
        }
        !was_cancelled
    }

    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    pub async fn cancelled(&self) {
        loop {
            let notified = self.state.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferPhase {
    Queued,
    Downloading,
    Finalizing,
    Completed,
    Cancelled,
    Failed,
}

impl TransferPhase {
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TransferSnapshot {
    pub phase: TransferPhase,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub destination_path: Option<PathBuf>,
    pub error: Option<CameraError>,
}

#[async_trait]
pub trait TransferJob: Send + Sync {
    fn snapshot(&self) -> TransferSnapshot;
    fn cancel(&self);
    async fn wait(&self) -> TransferSnapshot;
}

#[derive(Debug)]
struct FileTransferJob {
    snapshots: watch::Receiver<TransferSnapshot>,
    cancellation: CancellationToken,
}

#[async_trait]
impl TransferJob for FileTransferJob {
    fn snapshot(&self) -> TransferSnapshot {
        self.snapshots.borrow().clone()
    }

    fn cancel(&self) {
        self.cancellation.cancel();
    }

    async fn wait(&self) -> TransferSnapshot {
        let mut receiver = self.snapshots.clone();
        loop {
            let current = receiver.borrow().clone();
            if current.phase.is_terminal() {
                return current;
            }
            if receiver.changed().await.is_err() {
                return receiver.borrow().clone();
            }
        }
    }
}

#[derive(Debug)]
struct ProgressPublisher {
    sender: watch::Sender<TransferSnapshot>,
    last_publish: Mutex<Instant>,
    initial_offset: u64,
    total_bytes: u64,
}

impl ProgressPublisher {
    fn publish_bytes(&self, newly_written: u64, force: bool) {
        let mut last_publish = self
            .last_publish
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !force && last_publish.elapsed() < PROGRESS_INTERVAL {
            return;
        }
        *last_publish = Instant::now();
        let mut snapshot = self.sender.borrow().clone();
        snapshot.bytes_transferred = self.initial_offset.saturating_add(newly_written);
        snapshot.total_bytes = self.total_bytes;
        self.sender.send_replace(snapshot);
    }

    fn transition(
        &self,
        phase: TransferPhase,
        bytes_transferred: u64,
        destination_path: Option<PathBuf>,
        error: Option<CameraError>,
    ) {
        self.sender.send_replace(TransferSnapshot {
            phase,
            bytes_transferred,
            total_bytes: self.total_bytes,
            destination_path,
            error,
        });
    }
}

struct TrackingWriter {
    file: File,
    cancellation: CancellationToken,
    publisher: Arc<ProgressPublisher>,
    written: u64,
}

impl std::fmt::Debug for TrackingWriter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TrackingWriter")
            .field("written", &self.written)
            .field("cancelled", &self.cancellation.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl AsyncWrite for TrackingWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let this = self.get_mut();
        if this.cancellation.is_cancelled() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "transfer cancelled",
            )));
        }
        match Pin::new(&mut this.file).poll_write(context, buffer) {
            Poll::Ready(Ok(count)) => {
                this.written = this.written.saturating_add(count as u64);
                this.publisher.publish_bytes(this.written, false);
                Poll::Ready(Ok(count))
            }
            other => other,
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().file).poll_flush(context)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Pin::new(&mut self.get_mut().file).poll_shutdown(context)
    }
}

fn partial_path(destination: &Path) -> Result<PathBuf, CameraError> {
    let Some(filename) = destination.file_name() else {
        return Err(CameraError::invalid_input(
            "download destination must include a filename",
        ));
    };
    let mut partial_name = OsString::from(filename);
    partial_name.push(".partial");
    Ok(destination.with_file_name(partial_name))
}

async fn prepare_partial(
    request: &DownloadRequest,
    partial: &Path,
) -> Result<(File, u64), CameraError> {
    if let Some(parent) = request.destination_path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(CameraError::from_io)?;
    }

    if fs::try_exists(&request.destination_path)
        .await
        .map_err(CameraError::from_io)?
        && !request.overwrite
    {
        return Err(CameraError::Conflict);
    }

    let existing_length = if request.resume_partial {
        match fs::metadata(partial).await {
            Ok(metadata) if metadata.len() <= request.expected_size => metadata.len(),
            Ok(_) => 0,
            Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
            Err(error) => return Err(CameraError::from_io(error)),
        }
    } else {
        0
    };

    let mut options = OpenOptions::new();
    options.create(true).write(true);
    if existing_length == 0 {
        options.truncate(true);
    } else {
        options.append(true);
    }
    let file = options.open(partial).await.map_err(CameraError::from_io)?;
    Ok((file, existing_length))
}

async fn complete_atomic(
    partial: &Path,
    destination: &Path,
    overwrite: bool,
) -> Result<(), CameraError> {
    if overwrite {
        // 直接原子替换；不能先删除旧文件，否则重命名失败会丢失原文件。
        return fs::rename(partial, destination)
            .await
            .map_err(CameraError::from_io);
    }

    // 创建硬链接是不可覆盖的原子操作，即使目标在下载过程中出现也会失败。
    // 不支持硬链接的文件系统安全报错并保留 partial，绝不降级为覆盖操作。
    fs::hard_link(partial, destination).await.map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            CameraError::Conflict
        } else {
            CameraError::from_io(error)
        }
    })?;
    if let Err(error) = fs::remove_file(partial).await {
        tracing::warn!(%error, "已完成下载，但未能清理临时硬链接");
    }
    Ok(())
}

async fn run_download(
    source: Arc<dyn AssetSource>,
    request: DownloadRequest,
    partial: PathBuf,
    cancellation: CancellationToken,
    sender: watch::Sender<TransferSnapshot>,
) {
    let result = async {
        let (file, offset) = prepare_partial(&request, &partial).await?;
        let publisher = Arc::new(ProgressPublisher {
            sender: sender.clone(),
            last_publish: Mutex::new(Instant::now()),
            initial_offset: offset,
            total_bytes: request.expected_size,
        });
        publisher.transition(TransferPhase::Downloading, offset, None, None);

        if cancellation.is_cancelled() {
            return Err(CameraError::Cancelled);
        }

        let remaining = request.expected_size.saturating_sub(offset);
        let mut writer = TrackingWriter {
            file,
            cancellation: cancellation.clone(),
            publisher: Arc::clone(&publisher),
            written: 0,
        };

        if remaining > 0 {
            let outcome = source
                .stream_asset(
                    StreamRequest {
                        asset_id: request.asset_id.clone(),
                        offset,
                        expected_remaining_bytes: remaining,
                    },
                    &mut writer,
                    cancellation.clone(),
                )
                .await?;
            if outcome.bytes_written != writer.written {
                return Err(CameraError::protocol(format!(
                    "source reported {} bytes but wrote {}",
                    outcome.bytes_written, writer.written
                )));
            }
            if let Some(source_total) = outcome.source_total_bytes
                && source_total != request.expected_size
            {
                return Err(CameraError::protocol(format!(
                    "source length {source_total} does not match expected length {}",
                    request.expected_size
                )));
            }
        }

        if cancellation.is_cancelled() {
            return Err(CameraError::Cancelled);
        }
        writer.flush().await.map_err(CameraError::from_io)?;
        writer
            .file
            .sync_data()
            .await
            .map_err(CameraError::from_io)?;
        writer.publisher.publish_bytes(writer.written, true);
        let completed_bytes = offset.saturating_add(writer.written);
        if completed_bytes != request.expected_size {
            return Err(CameraError::protocol(format!(
                "download length {completed_bytes} does not match expected length {}",
                request.expected_size
            )));
        }

        writer
            .publisher
            .transition(TransferPhase::Finalizing, completed_bytes, None, None);
        drop(writer);
        complete_atomic(&partial, &request.destination_path, request.overwrite).await?;
        publisher.transition(
            TransferPhase::Completed,
            completed_bytes,
            Some(request.destination_path.clone()),
            None,
        );
        Ok::<(), CameraError>(())
    }
    .await;

    if let Err(error) = result {
        let current = sender.borrow().clone();
        let error = if cancellation.is_cancelled() {
            CameraError::Cancelled
        } else {
            error
        };
        sender.send_replace(TransferSnapshot {
            phase: if error == CameraError::Cancelled {
                TransferPhase::Cancelled
            } else {
                TransferPhase::Failed
            },
            bytes_transferred: current.bytes_transferred,
            total_bytes: request.expected_size,
            destination_path: None,
            error: Some(error),
        });
    }
}

/// 启动基于文件的传输，并在校验后立即返回。
///
/// 只有在完成长度校验、`sync_data` 和同目录原子发布后，才会报告完成。
/// 中断时保留 `.partial` 文件。
pub async fn start_file_download(
    source: Arc<dyn AssetSource>,
    request: DownloadRequest,
) -> Result<Arc<dyn TransferJob>, CameraError> {
    request.validate()?;
    let partial = partial_path(&request.destination_path)?;
    if fs::try_exists(&request.destination_path)
        .await
        .map_err(CameraError::from_io)?
        && !request.overwrite
    {
        return Err(CameraError::Conflict);
    }

    let initial = TransferSnapshot {
        phase: TransferPhase::Queued,
        bytes_transferred: 0,
        total_bytes: request.expected_size,
        destination_path: None,
        error: None,
    };
    let (sender, receiver) = watch::channel(initial);
    let cancellation = CancellationToken::default();
    tokio::spawn(run_download(
        source,
        request,
        partial,
        cancellation.clone(),
        sender,
    ));
    Ok(Arc::new(FileTransferJob {
        snapshots: receiver,
        cancellation,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetId, CameraId, StreamOutcome};
    use async_trait::async_trait;
    use tokio::io::AsyncWriteExt;

    #[derive(Debug)]
    struct BytesSource {
        bytes: Vec<u8>,
        fail_after: Option<usize>,
    }

    #[async_trait]
    impl AssetSource for BytesSource {
        async fn stream_asset(
            &self,
            request: StreamRequest,
            writer: &mut (dyn AsyncWrite + Unpin + Send),
            _cancellation: CancellationToken,
        ) -> Result<crate::StreamOutcome, CameraError> {
            let start = usize::try_from(request.offset).unwrap();
            let data = &self.bytes[start..];
            let count = self.fail_after.unwrap_or(data.len()).min(data.len());
            writer
                .write_all(&data[..count])
                .await
                .map_err(CameraError::from_io)?;
            if self.fail_after.is_some() {
                return Err(CameraError::SessionInterrupted {
                    message: "test interruption".to_owned(),
                });
            }
            Ok(StreamOutcome {
                bytes_written: count as u64,
                source_total_bytes: Some(self.bytes.len() as u64),
            })
        }
    }

    fn request(path: PathBuf, size: u64) -> DownloadRequest {
        let camera = CameraId::derive("test", "camera", None);
        DownloadRequest {
            asset_id: AssetId::derive(&camera, 1, b"asset", size, None),
            destination_path: path,
            expected_size: size,
            resume_partial: true,
            overwrite: false,
        }
    }

    #[test]
    fn cancellation_is_idempotent() {
        let token = CancellationToken::default();
        assert!(token.cancel());
        assert!(!token.cancel());
        assert!(token.is_cancelled());
    }

    #[tokio::test]
    async fn successful_transfer_is_atomically_completed() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("photo.nef");
        let bytes = b"camera bytes".to_vec();
        let source = Arc::new(BytesSource {
            bytes: bytes.clone(),
            fail_after: None,
        });
        let job = start_file_download(source, request(destination.clone(), bytes.len() as u64))
            .await
            .unwrap();
        let snapshot = job.wait().await;
        assert_eq!(snapshot.phase, TransferPhase::Completed);
        assert_eq!(snapshot.bytes_transferred, bytes.len() as u64);
        assert_eq!(fs::read(destination).await.unwrap(), bytes);
        assert!(!directory.path().join("photo.nef.partial").exists());
    }

    #[tokio::test]
    async fn interrupted_transfer_preserves_only_partial_file() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("photo.nef");
        let bytes = b"camera bytes".to_vec();
        let source = Arc::new(BytesSource {
            bytes: bytes.clone(),
            fail_after: Some(4),
        });
        let job = start_file_download(source, request(destination.clone(), bytes.len() as u64))
            .await
            .unwrap();
        let snapshot = job.wait().await;
        assert_eq!(snapshot.phase, TransferPhase::Failed);
        assert!(!destination.exists());
        assert_eq!(
            fs::read(directory.path().join("photo.nef.partial"))
                .await
                .unwrap(),
            &bytes[..4]
        );
    }

    #[tokio::test]
    async fn existing_destination_is_not_overwritten_by_default() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("photo.jpg");
        fs::write(&destination, b"existing").await.unwrap();
        let result = start_file_download(
            Arc::new(BytesSource {
                bytes: b"new".to_vec(),
                fail_after: None,
            }),
            request(destination, 3),
        )
        .await;
        assert!(matches!(result, Err(CameraError::Conflict)));
    }

    #[tokio::test]
    async fn destination_created_during_download_is_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("photo.nef");
        let partial = directory.path().join("photo.nef.partial");
        fs::write(&partial, b"new camera bytes").await.unwrap();
        fs::write(&destination, b"keep original").await.unwrap();
        let result = complete_atomic(&partial, &destination, false).await;
        assert!(matches!(result, Err(CameraError::Conflict)));
        assert_eq!(fs::read(destination).await.unwrap(), b"keep original");
        assert_eq!(fs::read(partial).await.unwrap(), b"new camera bytes");
    }

    #[tokio::test]
    async fn failed_replacement_does_not_delete_original() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("photo.nef");
        let missing_partial = directory.path().join("missing.partial");
        fs::write(&destination, b"keep original").await.unwrap();
        assert!(
            complete_atomic(&missing_partial, &destination, true)
                .await
                .is_err()
        );
        assert_eq!(fs::read(destination).await.unwrap(), b"keep original");
    }
}

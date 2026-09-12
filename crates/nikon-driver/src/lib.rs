//! Nikon Z6 II C 1.70 PTP/IP 驱动。

mod dataset;

use async_trait::async_trait;
use camera_core::{
    AssetId, AssetSource, CameraAsset, CameraCapabilityProfile, CameraDriver, CameraError,
    CameraId, CameraInfo, CameraSession, CancellationToken, CaptureGroupId, ConnectionConfig,
    DownloadRequest, MediaFormat, MediaPage, MediaQuery, PageCursor, StreamOutcome, StreamRequest,
    Thumbnail, ThumbnailRequest, TransferJob, start_file_download,
};
use dataset::{
    ObjectInfoDataset, parse_device_info, parse_object_handles, parse_object_info,
    parse_storage_ids,
};
use ptpip::{DataPhase, PtpIpConnectionConfig, PtpIpError, PtpIpSession};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncWrite;
use tokio::sync::RwLock;
use tracing::{debug, instrument, warn};

pub const Z6II_PROFILE_ID: &str = "nikon-z6ii-c-1.70-wifi";
pub const SUPPORTED_FIRMWARE: &str = "1.70";

pub mod operation {
    pub const GET_DEVICE_INFO: u16 = 0x1001;
    pub const OPEN_SESSION: u16 = 0x1002;
    pub const CLOSE_SESSION: u16 = 0x1003;
    pub const GET_STORAGE_IDS: u16 = 0x1004;
    pub const GET_OBJECT_HANDLES: u16 = 0x1007;
    pub const GET_OBJECT_INFO: u16 = 0x1008;
    pub const GET_OBJECT: u16 = 0x1009;
    pub const GET_THUMB: u16 = 0x100a;
    pub const GET_PARTIAL_OBJECT: u16 = 0x101b;
}

pub mod response {
    pub const OK: u16 = 0x2001;
    pub const OPERATION_NOT_SUPPORTED: u16 = 0x2005;
    pub const INVALID_OBJECT_HANDLE: u16 = 0x2009;
    pub const DEVICE_BUSY: u16 = 0x2019;
    pub const SESSION_ALREADY_OPEN: u16 = 0x201e;
}

#[derive(Clone, Debug, Default)]
pub struct NikonZ6IiDriver;

#[derive(Clone, Debug)]
struct RemoteObject {
    handle: u32,
    info: ObjectInfoDataset,
    asset: CameraAsset,
}

#[derive(Debug)]
struct NikonAssetSource {
    protocol: Arc<PtpIpSession>,
    objects: Arc<RwLock<HashMap<AssetId, RemoteObject>>>,
    partial_downloads: bool,
}

#[derive(Debug)]
struct NikonZ6IiSession {
    protocol: Arc<PtpIpSession>,
    info: CameraInfo,
    capabilities: CameraCapabilityProfile,
    objects: Arc<RwLock<HashMap<AssetId, RemoteObject>>>,
    ordered_assets: RwLock<Vec<CameraAsset>>,
    source: Arc<NikonAssetSource>,
}

impl NikonZ6IiDriver {
    pub fn capability_profile() -> CameraCapabilityProfile {
        CameraCapabilityProfile {
            profile_id: Z6II_PROFILE_ID.to_owned(),
            display_name: "Nikon Z6 II C 1.70 (Wi-Fi)".to_owned(),
            supported_formats: BTreeSet::from([MediaFormat::Jpeg, MediaFormat::Nef]),
            supports_thumbnails: true,
            supports_partial_downloads: false,
            supports_events: true,
            max_page_size: 60,
            max_thumbnail_bytes: 8 * 1024 * 1024,
        }
    }
}

#[async_trait]
impl CameraDriver for NikonZ6IiDriver {
    #[instrument(skip(self, config), fields(host = %config.host, port = config.port))]
    async fn connect(
        &self,
        config: ConnectionConfig,
    ) -> Result<Arc<dyn CameraSession>, CameraError> {
        config.validate()?;
        let protocol = Arc::new(
            PtpIpSession::connect(PtpIpConnectionConfig {
                host: config.host,
                port: config.port,
                initiator_guid: config.client_id,
                friendly_name: config.client_name,
                connect_timeout: config.connect_timeout,
                request_timeout: config.request_timeout,
                maximum_packet_length: ptpip::DEFAULT_MAX_PACKET_LENGTH,
            })
            .await
            .map_err(map_ptpip_error)?,
        );

        let (result, device_bytes) = protocol
            .execute_data_in(operation::GET_DEVICE_INFO, &[], 1024 * 1024)
            .await
            .map_err(map_ptpip_error)?;
        ensure_response(result.response.response_code)?;
        let device = parse_device_info(&device_bytes)?;
        if !device.manufacturer.to_ascii_lowercase().contains("nikon") {
            let _ = protocol.disconnect().await;
            return Err(CameraError::invalid_input(format!(
                "expected Nikon camera, found {}",
                device.manufacturer
            )));
        }
        if !is_z6ii_model(&device.model) {
            let _ = protocol.disconnect().await;
            return Err(CameraError::invalid_input(format!(
                "expected Nikon Z6 II, found {}",
                device.model
            )));
        }
        if !device.device_version.contains(SUPPORTED_FIRMWARE) {
            let _ = protocol.disconnect().await;
            return Err(CameraError::UnsupportedFirmware {
                found: device.device_version,
                supported: SUPPORTED_FIRMWARE.to_owned(),
            });
        }
        ensure_response(
            protocol
                .execute_no_data(operation::OPEN_SESSION, &[1])
                .await
                .map_err(map_ptpip_error)?
                .response
                .response_code,
        )?;

        let serial = (!device.serial_number.is_empty()).then_some(device.serial_number);
        let camera_id = CameraId::derive(&device.manufacturer, &device.model, serial.as_deref());
        let info = CameraInfo {
            id: camera_id,
            manufacturer: device.manufacturer,
            model: "Z6 II".to_owned(),
            display_name: "Nikon Z6 II".to_owned(),
            serial_number: serial,
            firmware_version: Some(SUPPORTED_FIRMWARE.to_owned()),
        };
        let capabilities = Self::capability_profile();
        capabilities.validate()?;
        let objects = Arc::new(RwLock::new(HashMap::new()));
        let source = Arc::new(NikonAssetSource {
            protocol: Arc::clone(&protocol),
            objects: Arc::clone(&objects),
            partial_downloads: capabilities.supports_partial_downloads,
        });
        Ok(Arc::new(NikonZ6IiSession {
            protocol,
            info,
            capabilities,
            objects,
            ordered_assets: RwLock::new(Vec::new()),
            source,
        }))
    }
}

#[async_trait]
impl CameraSession for NikonZ6IiSession {
    async fn info(&self) -> Result<CameraInfo, CameraError> {
        Ok(self.info.clone())
    }
    async fn capabilities(&self) -> Result<CameraCapabilityProfile, CameraError> {
        Ok(self.capabilities.clone())
    }

    async fn list_media(&self, query: MediaQuery) -> Result<MediaPage, CameraError> {
        query.validate(&self.capabilities)?;
        if self.ordered_assets.read().await.is_empty() {
            self.refresh_assets().await?;
        }
        let assets = self.ordered_assets.read().await;
        let filtered: Vec<_> = assets
            .iter()
            .filter(|asset| query.formats.is_empty() || query.formats.contains(&asset.format))
            .cloned()
            .collect();
        let offset = query
            .cursor
            .as_ref()
            .map(parse_cursor)
            .transpose()?
            .unwrap_or(0);
        if offset > filtered.len() {
            return Err(CameraError::invalid_input("media cursor is past the end"));
        }
        let end = offset
            .saturating_add(query.page_size as usize)
            .min(filtered.len());
        let next_cursor = (end < filtered.len())
            .then(|| PageCursor::parse(format!("offset_{end}")))
            .transpose()?;
        Ok(MediaPage {
            items: filtered[offset..end].to_vec(),
            next_cursor,
            total_items: Some(filtered.len() as u64),
        })
    }

    async fn load_thumbnail(&self, request: ThumbnailRequest) -> Result<Thumbnail, CameraError> {
        if request.max_bytes == 0 || request.max_bytes > self.capabilities.max_thumbnail_bytes {
            return Err(CameraError::invalid_input("invalid thumbnail byte limit"));
        }
        let object = self.object(&request.asset_id).await?;
        if object.info.thumb_compressed_size == 0 {
            return Err(CameraError::UnsupportedOperation {
                operation: "thumbnail unavailable".into(),
            });
        }
        let (result, data) = self
            .protocol
            .execute_data_in(operation::GET_THUMB, &[object.handle], request.max_bytes)
            .await
            .map_err(map_ptpip_error)?;
        ensure_response(result.response.response_code)?;
        if data.len() as u64 > request.max_bytes {
            return Err(CameraError::protocol("thumbnail exceeds requested maximum"));
        }
        Ok(Thumbnail {
            mime_type: "image/jpeg".to_owned(),
            data,
        })
    }

    async fn start_download(
        &self,
        mut request: DownloadRequest,
    ) -> Result<Arc<dyn TransferJob>, CameraError> {
        let object = self.object(&request.asset_id).await?;
        if request.expected_size != object.asset.size_bytes {
            return Err(CameraError::invalid_input(
                "download size does not match camera metadata",
            ));
        }
        request.resume_partial &= self.capabilities.supports_partial_downloads;
        start_file_download(self.source.clone(), request).await
    }

    async fn disconnect(&self) -> Result<(), CameraError> {
        if !self.protocol.is_closed() {
            match self
                .protocol
                .execute_no_data(operation::CLOSE_SESSION, &[])
                .await
            {
                Ok(value) => {
                    ensure_response(value.response.response_code)?;
                }
                Err(PtpIpError::Closed) => {}
                Err(error) => warn!(%error, "close-session command failed; closing transport"),
            }
        }
        self.protocol.disconnect().await.map_err(map_ptpip_error)
    }
}

impl NikonZ6IiSession {
    async fn object(&self, id: &AssetId) -> Result<RemoteObject, CameraError> {
        if let Some(value) = self.objects.read().await.get(id).cloned() {
            return Ok(value);
        }
        self.refresh_assets().await?;
        self.objects
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or(CameraError::ObjectNotFound)
    }

    async fn refresh_assets(&self) -> Result<(), CameraError> {
        let (result, storage_bytes) = self
            .protocol
            .execute_data_in(operation::GET_STORAGE_IDS, &[], 1024 * 1024)
            .await
            .map_err(map_ptpip_error)?;
        ensure_response(result.response.response_code)?;
        let mut remotes = HashMap::new();
        let mut assets = Vec::new();
        for storage_id in parse_storage_ids(&storage_bytes)? {
            let (result, handle_bytes) = self
                .protocol
                .execute_data_in(
                    operation::GET_OBJECT_HANDLES,
                    &[storage_id, 0, 0],
                    16 * 1024 * 1024,
                )
                .await
                .map_err(map_ptpip_error)?;
            ensure_response(result.response.response_code)?;
            for handle in parse_object_handles(&handle_bytes)? {
                let (result, info_bytes) = self
                    .protocol
                    .execute_data_in(operation::GET_OBJECT_INFO, &[handle], 1024 * 1024)
                    .await
                    .map_err(map_ptpip_error)?;
                ensure_response(result.response.response_code)?;
                let info = parse_object_info(&info_bytes)?;
                let Some(format) = info.media_format() else {
                    continue;
                };
                let stem = Path::new(&info.filename)
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or(&info.filename);
                let id = AssetId::derive(
                    &self.info.id,
                    info.storage_id,
                    &handle.to_le_bytes(),
                    info.compressed_size,
                    info.captured_at,
                );
                let asset = CameraAsset {
                    id: id.clone(),
                    capture_group_id: CaptureGroupId::derive(
                        &self.info.id,
                        info.storage_id,
                        stem,
                        info.captured_at,
                    ),
                    filename: sanitize_filename(&info.filename)?,
                    format,
                    size_bytes: info.compressed_size,
                    captured_at: info.captured_at,
                    width: (info.image_width != 0).then_some(info.image_width),
                    height: (info.image_height != 0).then_some(info.image_height),
                    thumbnail_available: info.thumb_compressed_size > 0,
                };
                remotes.insert(
                    id,
                    RemoteObject {
                        handle,
                        info,
                        asset: asset.clone(),
                    },
                );
                assets.push(asset);
            }
        }
        assets.sort_by(|left, right| {
            right
                .captured_at
                .cmp(&left.captured_at)
                .then_with(|| right.filename.cmp(&left.filename))
        });
        debug!(count = assets.len(), "refreshed Nikon media cache");
        *self.objects.write().await = remotes;
        *self.ordered_assets.write().await = assets;
        Ok(())
    }
}

#[async_trait]
impl AssetSource for NikonAssetSource {
    async fn stream_asset(
        &self,
        request: StreamRequest,
        writer: &mut (dyn AsyncWrite + Unpin + Send),
        cancellation: CancellationToken,
    ) -> Result<StreamOutcome, CameraError> {
        let object = self
            .objects
            .read()
            .await
            .get(&request.asset_id)
            .cloned()
            .ok_or(CameraError::ObjectNotFound)?;
        let (operation, parameters) = if request.offset == 0 {
            (operation::GET_OBJECT, vec![object.handle])
        } else if self.partial_downloads {
            let offset =
                u32::try_from(request.offset).map_err(|_| CameraError::UnsupportedOperation {
                    operation: "partial offset above 4 GiB".into(),
                })?;
            let length = u32::try_from(request.expected_remaining_bytes).unwrap_or(u32::MAX);
            (
                operation::GET_PARTIAL_OBJECT,
                vec![object.handle, offset, length],
            )
        } else {
            return Err(CameraError::UnsupportedOperation {
                operation: "partial download".into(),
            });
        };
        let result = self
            .protocol
            .execute_data_in_to(
                operation,
                &parameters,
                DataPhase::DataIn,
                request.expected_remaining_bytes,
                writer,
                || cancellation.is_cancelled(),
            )
            .await
            .map_err(map_ptpip_error)?;
        ensure_response(result.response.response_code)?;
        Ok(StreamOutcome {
            bytes_written: result.data_length,
            source_total_bytes: Some(object.info.compressed_size),
        })
    }
}

fn ensure_response(code: u16) -> Result<(), CameraError> {
    match code {
        response::OK | response::SESSION_ALREADY_OPEN => Ok(()),
        response::DEVICE_BUSY => Err(CameraError::CameraBusy),
        response::INVALID_OBJECT_HANDLE => Err(CameraError::ObjectNotFound),
        response::OPERATION_NOT_SUPPORTED => Err(CameraError::UnsupportedOperation {
            operation: "camera operation".into(),
        }),
        other => Err(CameraError::protocol(format!(
            "camera returned PTP response 0x{other:04x}"
        ))),
    }
}

fn map_ptpip_error(error: PtpIpError) -> CameraError {
    match error {
        PtpIpError::Timeout(operation) => CameraError::Timeout { operation },
        PtpIpError::Cancelled => CameraError::Cancelled,
        PtpIpError::Closed => CameraError::SessionInterrupted {
            message: "transport is closed".into(),
        },
        PtpIpError::Io(message) => CameraError::ConnectionFailed { message },
        other => CameraError::protocol(other.to_string()),
    }
}

fn is_z6ii_model(model: &str) -> bool {
    let normalized: String = model
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect();
    matches!(normalized.as_str(), "z6ii" | "z62")
}

fn parse_cursor(cursor: &PageCursor) -> Result<usize, CameraError> {
    cursor
        .as_str()
        .strip_prefix("offset_")
        .ok_or_else(|| CameraError::invalid_input("unknown media cursor"))?
        .parse()
        .map_err(|_| CameraError::invalid_input("invalid media cursor offset"))
}

fn sanitize_filename(value: &str) -> Result<String, CameraError> {
    let file = Path::new(value)
        .file_name()
        .and_then(|part| part.to_str())
        .ok_or_else(|| CameraError::protocol("camera returned an invalid filename"))?;
    if file.is_empty() || file == "." || file == ".." {
        return Err(CameraError::protocol("camera returned an unsafe filename"));
    }
    Ok(file.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn z6ii_model_variants_are_recognized() {
        assert!(is_z6ii_model("Z 6II"));
        assert!(is_z6ii_model("Z 6_2"));
        assert!(!is_z6ii_model("Z 6III"));
    }

    #[test]
    fn filenames_cannot_escape_staging() {
        assert_eq!(
            sanitize_filename("DCIM/DSC_0001.NEF").unwrap(),
            "DSC_0001.NEF"
        );
        assert!(sanitize_filename("..").is_err());
    }
}

//! 用于集成测试和故障测试的确定性类 Nikon PTP/IP 对端。

use ptp_codec::OperationResponse;
use ptpip::{Packet, PacketType, PtpIpError, read_packet, write_packet};
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, watch};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use uuid::Uuid;

const STORAGE_ID: u32 = 0x0001_0001;
const RESPONSE_OK: u16 = 0x2001;
const RESPONSE_BUSY: u16 = 0x2019;

#[derive(Clone, Debug)]
pub struct MockCameraConfig {
    pub asset_count: usize,
    pub chunk_size: usize,
    pub response_delay: Duration,
    pub disconnect_first_download_after: Option<u64>,
    pub malformed_first_response: bool,
    pub busy_first_operation: bool,
}

impl Default for MockCameraConfig {
    fn default() -> Self {
        Self {
            asset_count: 500,
            chunk_size: 16 * 1024,
            response_delay: Duration::ZERO,
            disconnect_first_download_after: None,
            malformed_first_response: false,
            busy_first_operation: false,
        }
    }
}

#[derive(Clone, Debug)]
struct MockAsset {
    handle: u32,
    filename: String,
    object_format: u16,
    bytes: Vec<u8>,
    captured_at: String,
}

#[derive(Debug)]
struct State {
    assets: HashMap<u32, MockAsset>,
    config: MockCameraConfig,
    first_download_fault_used: bool,
}

#[derive(Debug)]
pub struct MockCameraServer {
    address: SocketAddr,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<Result<(), MockCameraError>>,
}

impl MockCameraServer {
    pub async fn spawn(config: MockCameraConfig) -> Result<Self, MockCameraError> {
        Self::bind("127.0.0.1:0", config).await
    }

    pub async fn bind(address: &str, config: MockCameraConfig) -> Result<Self, MockCameraError> {
        if config.asset_count > u32::MAX as usize - 1 || config.chunk_size == 0 {
            return Err(MockCameraError::InvalidConfiguration);
        }
        let listener = TcpListener::bind(address).await?;
        let address = listener.local_addr()?;
        let state = Arc::new(Mutex::new(State {
            assets: build_assets(config.asset_count),
            config,
            first_download_fault_used: false,
        }));
        let (shutdown, receiver) = watch::channel(false);
        let task = tokio::spawn(serve(listener, state, receiver));
        Ok(Self {
            address,
            shutdown,
            task,
        })
    }

    pub const fn address(&self) -> SocketAddr {
        self.address
    }

    pub async fn stop(self) -> Result<(), MockCameraError> {
        let _ = self.shutdown.send(true);
        self.task
            .await
            .map_err(|error| MockCameraError::Task(error.to_string()))?
    }
}

async fn serve(
    listener: TcpListener,
    state: Arc<Mutex<State>>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), MockCameraError> {
    info!(address = %listener.local_addr()?, "mock camera listening");
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() { return Ok(()); }
            }
            accepted = listener.accept() => {
                let (mut command, peer) = accepted?;
                debug!(%peer, "command channel accepted");
                initialize_command(&mut command).await?;
                let (mut event, event_peer) = listener.accept().await?;
                debug!(%event_peer, "event channel accepted");
                initialize_event(&mut event).await?;
                let connection_state = Arc::clone(&state);
                tokio::spawn(async move {
                    let event_task = tokio::spawn(async move {
                        let mut byte = [0_u8; 1];
                        let _ = tokio::io::AsyncReadExt::read(&mut event, &mut byte).await;
                    });
                    if let Err(error) = command_loop(command, connection_state).await {
                        warn!(%error, "mock command connection ended");
                    }
                    event_task.abort();
                });
            }
        }
    }
}

async fn initialize_command(stream: &mut TcpStream) -> Result<(), MockCameraError> {
    let packet = read_packet(
        stream,
        Duration::from_secs(5),
        ptpip::DEFAULT_MAX_PACKET_LENGTH,
    )
    .await?;
    match packet {
        Packet::InitCommandRequest {
            protocol_version, ..
        } if protocol_version == ptpip::PROTOCOL_VERSION_1_0 => {
            write_packet(
                stream,
                &Packet::InitCommandAck {
                    connection_number: 1,
                    responder_guid: *Uuid::new_v4().as_bytes(),
                    friendly_name: "CameraBridge Mock Nikon Z6 II".to_owned(),
                    protocol_version: ptpip::PROTOCOL_VERSION_1_0,
                },
                Duration::from_secs(5),
            )
            .await?;
            Ok(())
        }
        other => Err(MockCameraError::UnexpectedPacket(other.packet_type())),
    }
}

async fn initialize_event(stream: &mut TcpStream) -> Result<(), MockCameraError> {
    match read_packet(stream, Duration::from_secs(5), 1024).await? {
        Packet::InitEventRequest {
            connection_number: 1,
        } => {
            write_packet(stream, &Packet::InitEventAck, Duration::from_secs(5)).await?;
            Ok(())
        }
        other => Err(MockCameraError::UnexpectedPacket(other.packet_type())),
    }
}

async fn command_loop(
    mut stream: TcpStream,
    state: Arc<Mutex<State>>,
) -> Result<(), MockCameraError> {
    let mut first_operation = true;
    loop {
        let packet = match read_packet(
            &mut stream,
            Duration::from_secs(30),
            ptpip::DEFAULT_MAX_PACKET_LENGTH,
        )
        .await
        {
            Ok(packet) => packet,
            Err(PtpIpError::Io(_)) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let Packet::OperationRequest {
            operation_code,
            transaction_id,
            parameters,
            ..
        } = packet
        else {
            if matches!(packet, Packet::Cancel { .. }) {
                continue;
            }
            return Err(MockCameraError::UnexpectedPacket(packet.packet_type()));
        };
        let (delay, malformed, busy) = {
            let guard = state.lock().await;
            (
                guard.config.response_delay,
                guard.config.malformed_first_response && first_operation,
                guard.config.busy_first_operation && first_operation,
            )
        };
        first_operation = false;
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        if malformed {
            stream.write_all(&[4, 0, 0, 0, 7, 0, 0, 0]).await?;
            stream.shutdown().await?;
            return Ok(());
        }
        if busy {
            send_response(&mut stream, transaction_id, RESPONSE_BUSY).await?;
            continue;
        }
        match operation_code {
            0x1001 => {
                send_data_response(
                    &mut stream,
                    transaction_id,
                    encode_device_info(),
                    state.lock().await.config.chunk_size,
                    None,
                )
                .await?;
            }
            0x1002 | 0x1003 => send_response(&mut stream, transaction_id, RESPONSE_OK).await?,
            0x1004 => {
                send_data_response(
                    &mut stream,
                    transaction_id,
                    encode_u32_array(&[STORAGE_ID]),
                    state.lock().await.config.chunk_size,
                    None,
                )
                .await?;
            }
            0x1007 => {
                let guard = state.lock().await;
                let mut handles: Vec<_> = guard.assets.keys().copied().collect();
                handles.sort_unstable();
                let data = encode_u32_array(&handles);
                let chunk = guard.config.chunk_size;
                drop(guard);
                send_data_response(&mut stream, transaction_id, data, chunk, None).await?;
            }
            0x1008 => {
                let handle = parameters
                    .first()
                    .copied()
                    .ok_or(MockCameraError::MissingParameter)?;
                let guard = state.lock().await;
                let asset = guard
                    .assets
                    .get(&handle)
                    .ok_or(MockCameraError::UnknownObject(handle))?;
                let data = encode_object_info(asset);
                let chunk = guard.config.chunk_size;
                drop(guard);
                send_data_response(&mut stream, transaction_id, data, chunk, None).await?;
            }
            0x100a => {
                send_data_response(
                    &mut stream,
                    transaction_id,
                    vec![0xff, 0xd8, 0xff, 0xd9],
                    1024,
                    None,
                )
                .await?;
            }
            0x1009 | 0x101b => {
                let handle = parameters
                    .first()
                    .copied()
                    .ok_or(MockCameraError::MissingParameter)?;
                let (bytes, chunk_size, disconnect) = {
                    let mut guard = state.lock().await;
                    let asset = guard
                        .assets
                        .get(&handle)
                        .ok_or(MockCameraError::UnknownObject(handle))?;
                    let offset = if operation_code == 0x101b {
                        parameters.get(1).copied().unwrap_or(0) as usize
                    } else {
                        0
                    };
                    let maximum = if operation_code == 0x101b {
                        parameters.get(2).copied().unwrap_or(u32::MAX) as usize
                    } else {
                        usize::MAX
                    };
                    let end = offset.saturating_add(maximum).min(asset.bytes.len());
                    let bytes = asset
                        .bytes
                        .get(offset..end)
                        .ok_or(MockCameraError::UnknownObject(handle))?
                        .to_vec();
                    let disconnect = if guard.first_download_fault_used {
                        None
                    } else {
                        guard.first_download_fault_used =
                            guard.config.disconnect_first_download_after.is_some();
                        guard.config.disconnect_first_download_after
                    };
                    (bytes, guard.config.chunk_size, disconnect)
                };
                send_data_response(&mut stream, transaction_id, bytes, chunk_size, disconnect)
                    .await?;
                if disconnect.is_some() {
                    return Ok(());
                }
            }
            _ => send_response(&mut stream, transaction_id, 0x2005).await?,
        }
    }
}

async fn send_response(
    stream: &mut TcpStream,
    transaction_id: u32,
    response_code: u16,
) -> Result<(), MockCameraError> {
    write_packet(
        stream,
        &Packet::OperationResponse(
            OperationResponse::new(response_code, transaction_id, [])
                .map_err(|error| MockCameraError::Protocol(error.to_string()))?,
        ),
        Duration::from_secs(5),
    )
    .await?;
    Ok(())
}

async fn send_data_response(
    stream: &mut TcpStream,
    transaction_id: u32,
    data: Vec<u8>,
    chunk_size: usize,
    disconnect_after: Option<u64>,
) -> Result<(), MockCameraError> {
    write_packet(
        stream,
        &Packet::StartData {
            transaction_id,
            total_length: data.len() as u64,
        },
        Duration::from_secs(5),
    )
    .await?;
    let mut sent = 0_u64;
    let mut chunks = data.chunks(chunk_size).peekable();
    while let Some(chunk) = chunks.next() {
        let packet = if chunks.peek().is_none() {
            Packet::EndData {
                transaction_id,
                bytes: chunk.to_vec(),
            }
        } else {
            Packet::Data {
                transaction_id,
                bytes: chunk.to_vec(),
            }
        };
        write_packet(stream, &packet, Duration::from_secs(5)).await?;
        sent += chunk.len() as u64;
        if disconnect_after.is_some_and(|limit| sent >= limit) {
            stream.shutdown().await?;
            return Ok(());
        }
    }
    if data.is_empty() {
        write_packet(
            stream,
            &Packet::EndData {
                transaction_id,
                bytes: Vec::new(),
            },
            Duration::from_secs(5),
        )
        .await?;
    }
    send_response(stream, transaction_id, RESPONSE_OK).await
}

fn build_assets(count: usize) -> HashMap<u32, MockAsset> {
    (0..count)
        .map(|index| {
            let handle = u32::try_from(index + 1).expect("validated asset count");
            let is_nef = index % 2 == 1;
            let filename = format!(
                "DSC_{:04}.{}",
                index + 1,
                if is_nef { "NEF" } else { "JPG" }
            );
            let size = if is_nef {
                4096 + index % 127
            } else {
                1024 + index % 61
            };
            let bytes = (0..size)
                .map(|offset| (offset as u8).wrapping_add(handle as u8))
                .collect();
            (
                handle,
                MockAsset {
                    handle,
                    filename,
                    object_format: if is_nef { 0xb101 } else { 0x3801 },
                    bytes,
                    captured_at: format!("202608{:02}T120000", 1 + index % 28),
                },
            )
        })
        .collect()
}

fn encode_device_info() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&100_u16.to_le_bytes());
    out.extend_from_slice(&6_u32.to_le_bytes());
    out.extend_from_slice(&100_u16.to_le_bytes());
    push_string(&mut out, "Nikon mock");
    out.extend_from_slice(&0_u16.to_le_bytes());
    push_u16_array(
        &mut out,
        &[
            0x1001, 0x1002, 0x1003, 0x1004, 0x1007, 0x1008, 0x1009, 0x100a,
        ],
    );
    for _ in 0..4 {
        out.extend_from_slice(&0_u32.to_le_bytes());
    }
    push_string(&mut out, "Nikon");
    push_string(&mut out, "Z 6_2");
    push_string(&mut out, "C 1.70");
    push_string(&mut out, "MOCK-0001");
    out
}

fn encode_object_info(asset: &MockAsset) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&STORAGE_ID.to_le_bytes());
    out.extend_from_slice(&asset.object_format.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&(asset.bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x3801_u16.to_le_bytes());
    out.extend_from_slice(&4_u32.to_le_bytes());
    out.extend_from_slice(&160_u32.to_le_bytes());
    out.extend_from_slice(&120_u32.to_le_bytes());
    out.extend_from_slice(&6048_u32.to_le_bytes());
    out.extend_from_slice(&4024_u32.to_le_bytes());
    out.extend_from_slice(&24_u32.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&asset.handle.to_le_bytes());
    push_string(&mut out, &asset.filename);
    push_string(&mut out, &asset.captured_at);
    push_string(&mut out, &asset.captured_at);
    push_string(&mut out, "");
    out
}

fn encode_u32_array(values: &[u32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + values.len() * 4);
    out.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

fn push_u16_array(out: &mut Vec<u8>, values: &[u16]) {
    out.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn push_string(out: &mut Vec<u8>, value: &str) {
    let units: Vec<_> = value.encode_utf16().collect();
    out.push(u8::try_from(units.len() + 1).expect("mock string length"));
    for unit in &units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&0_u16.to_le_bytes());
}

#[derive(Debug, Error)]
pub enum MockCameraError {
    #[error("invalid mock-camera configuration")]
    InvalidConfiguration,
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("PTP/IP error: {0}")]
    PtpIp(#[from] PtpIpError),
    #[error("unexpected packet {0:?}")]
    UnexpectedPacket(PacketType),
    #[error("operation parameter is missing")]
    MissingParameter,
    #[error("unknown mock object {0}")]
    UnknownObject(u32),
    #[error("protocol construction failed: {0}")]
    Protocol(String),
    #[error("server task failed: {0}")]
    Task(String),
}

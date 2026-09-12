//! PTP/IP 帧处理，以及异步、事务串行化的发起端会话。
//!
//! 本实现遵循 CIPA DC-005。它有意公开数值形式的操作码和响应码，
//! 使相机专属行为保留在驱动 crate 中。数据包解析在分配内存前受限，
//! 大型 data-in 操作则直接以流式方式写入异步 writer。

use bytes::BytesMut;
use ptp_codec::{Event, OperationResponse};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::time::timeout;
use tracing::{debug, instrument};
use uuid::Uuid;

pub const DEFAULT_PORT: u16 = 15_740;
pub const PROTOCOL_VERSION_1_0: u32 = 0x0001_0000;
pub const DEFAULT_MAX_PACKET_LENGTH: u32 = 128 * 1024 * 1024;
pub const PACKET_HEADER_LENGTH: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PacketType {
    InitCommandRequest = 1,
    InitCommandAck = 2,
    InitEventRequest = 3,
    InitEventAck = 4,
    InitFail = 5,
    OperationRequest = 6,
    OperationResponse = 7,
    Event = 8,
    StartData = 9,
    Data = 10,
    EndData = 11,
    Cancel = 12,
    ProbeRequest = 13,
    ProbeResponse = 14,
}

impl TryFrom<u32> for PacketType {
    type Error = PtpIpError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::InitCommandRequest),
            2 => Ok(Self::InitCommandAck),
            3 => Ok(Self::InitEventRequest),
            4 => Ok(Self::InitEventAck),
            5 => Ok(Self::InitFail),
            6 => Ok(Self::OperationRequest),
            7 => Ok(Self::OperationResponse),
            8 => Ok(Self::Event),
            9 => Ok(Self::StartData),
            10 => Ok(Self::Data),
            11 => Ok(Self::EndData),
            12 => Ok(Self::Cancel),
            13 => Ok(Self::ProbeRequest),
            14 => Ok(Self::ProbeResponse),
            other => Err(PtpIpError::UnknownPacketType(other)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DataPhase {
    None = 1,
    DataOut = 2,
    DataIn = 3,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Packet {
    InitCommandRequest {
        guid: [u8; 16],
        friendly_name: String,
        protocol_version: u32,
    },
    InitCommandAck {
        connection_number: u32,
        responder_guid: [u8; 16],
        friendly_name: String,
        protocol_version: u32,
    },
    InitEventRequest {
        connection_number: u32,
    },
    InitEventAck,
    InitFail {
        reason: u32,
    },
    OperationRequest {
        data_phase: DataPhase,
        operation_code: u16,
        transaction_id: u32,
        parameters: Vec<u32>,
    },
    OperationResponse(OperationResponse),
    Event(Event),
    StartData {
        transaction_id: u32,
        total_length: u64,
    },
    Data {
        transaction_id: u32,
        bytes: Vec<u8>,
    },
    EndData {
        transaction_id: u32,
        bytes: Vec<u8>,
    },
    Cancel {
        transaction_id: u32,
    },
    ProbeRequest,
    ProbeResponse,
}

impl Packet {
    pub const fn packet_type(&self) -> PacketType {
        match self {
            Self::InitCommandRequest { .. } => PacketType::InitCommandRequest,
            Self::InitCommandAck { .. } => PacketType::InitCommandAck,
            Self::InitEventRequest { .. } => PacketType::InitEventRequest,
            Self::InitEventAck => PacketType::InitEventAck,
            Self::InitFail { .. } => PacketType::InitFail,
            Self::OperationRequest { .. } => PacketType::OperationRequest,
            Self::OperationResponse(_) => PacketType::OperationResponse,
            Self::Event(_) => PacketType::Event,
            Self::StartData { .. } => PacketType::StartData,
            Self::Data { .. } => PacketType::Data,
            Self::EndData { .. } => PacketType::EndData,
            Self::Cancel { .. } => PacketType::Cancel,
            Self::ProbeRequest => PacketType::ProbeRequest,
            Self::ProbeResponse => PacketType::ProbeResponse,
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, PtpIpError> {
        let mut payload = Vec::new();
        match self {
            Self::InitCommandRequest {
                guid,
                friendly_name,
                protocol_version,
            } => {
                payload.extend_from_slice(guid);
                encode_utf16z(&mut payload, friendly_name)?;
                payload.extend_from_slice(&protocol_version.to_le_bytes());
            }
            Self::InitCommandAck {
                connection_number,
                responder_guid,
                friendly_name,
                protocol_version,
            } => {
                payload.extend_from_slice(&connection_number.to_le_bytes());
                payload.extend_from_slice(responder_guid);
                encode_utf16z(&mut payload, friendly_name)?;
                payload.extend_from_slice(&protocol_version.to_le_bytes());
            }
            Self::InitEventRequest { connection_number } => {
                payload.extend_from_slice(&connection_number.to_le_bytes());
            }
            Self::InitEventAck | Self::ProbeRequest | Self::ProbeResponse => {}
            Self::InitFail { reason } => payload.extend_from_slice(&reason.to_le_bytes()),
            Self::OperationRequest {
                data_phase,
                operation_code,
                transaction_id,
                parameters,
            } => {
                validate_parameters(parameters, 5)?;
                payload.extend_from_slice(&(*data_phase as u32).to_le_bytes());
                payload.extend_from_slice(&operation_code.to_le_bytes());
                payload.extend_from_slice(&transaction_id.to_le_bytes());
                encode_parameters(&mut payload, parameters);
            }
            Self::OperationResponse(response) => {
                validate_parameters(&response.parameters, 5)?;
                payload.extend_from_slice(&response.response_code.to_le_bytes());
                payload.extend_from_slice(&response.transaction_id.to_le_bytes());
                encode_parameters(&mut payload, &response.parameters);
            }
            Self::Event(event) => {
                validate_parameters(&event.parameters, 3)?;
                payload.extend_from_slice(&event.event_code.to_le_bytes());
                payload.extend_from_slice(&event.transaction_id.to_le_bytes());
                encode_parameters(&mut payload, &event.parameters);
            }
            Self::StartData {
                transaction_id,
                total_length,
            } => {
                payload.extend_from_slice(&transaction_id.to_le_bytes());
                payload.extend_from_slice(&total_length.to_le_bytes());
            }
            Self::Data {
                transaction_id,
                bytes,
            }
            | Self::EndData {
                transaction_id,
                bytes,
            } => {
                payload.extend_from_slice(&transaction_id.to_le_bytes());
                payload.extend_from_slice(bytes);
            }
            Self::Cancel { transaction_id } => {
                payload.extend_from_slice(&transaction_id.to_le_bytes());
            }
        }
        let length = PACKET_HEADER_LENGTH
            .checked_add(payload.len())
            .ok_or(PtpIpError::LengthOverflow)?;
        let length = u32::try_from(length).map_err(|_| PtpIpError::LengthOverflow)?;
        if length > DEFAULT_MAX_PACKET_LENGTH {
            return Err(PtpIpError::PacketTooLarge {
                declared: length,
                maximum: DEFAULT_MAX_PACKET_LENGTH,
            });
        }
        let mut output = Vec::with_capacity(length as usize);
        output.extend_from_slice(&length.to_le_bytes());
        output.extend_from_slice(&(self.packet_type() as u32).to_le_bytes());
        output.extend_from_slice(&payload);
        Ok(output)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, PtpIpError> {
        if bytes.len() < PACKET_HEADER_LENGTH {
            return Err(PtpIpError::UnexpectedEof {
                expected: PACKET_HEADER_LENGTH,
                actual: bytes.len(),
            });
        }
        let declared = read_u32(bytes, 0);
        validate_packet_length(declared, DEFAULT_MAX_PACKET_LENGTH)?;
        if bytes.len() != declared as usize {
            return Err(PtpIpError::LengthMismatch {
                declared: declared as usize,
                actual: bytes.len(),
            });
        }
        let kind = PacketType::try_from(read_u32(bytes, 4))?;
        decode_payload(kind, &bytes[PACKET_HEADER_LENGTH..])
    }
}

#[derive(Clone, Debug)]
pub struct PacketDecoder {
    buffer: BytesMut,
    maximum: u32,
}

impl Default for PacketDecoder {
    fn default() -> Self {
        Self {
            buffer: BytesMut::new(),
            maximum: DEFAULT_MAX_PACKET_LENGTH,
        }
    }
}

impl PacketDecoder {
    pub fn with_maximum(maximum: u32) -> Result<Self, PtpIpError> {
        validate_packet_length(PACKET_HEADER_LENGTH as u32, maximum)?;
        Ok(Self {
            buffer: BytesMut::new(),
            maximum,
        })
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    pub fn next_packet(&mut self) -> Result<Option<Packet>, PtpIpError> {
        if self.buffer.len() < 4 {
            return Ok(None);
        }
        let declared = read_u32(&self.buffer, 0);
        validate_packet_length(declared, self.maximum)?;
        if self.buffer.len() < declared as usize {
            return Ok(None);
        }
        let frame = self.buffer.split_to(declared as usize);
        Packet::decode(&frame).map(Some)
    }
}

#[derive(Clone, Debug)]
pub struct PtpIpConnectionConfig {
    pub host: String,
    pub port: u16,
    pub initiator_guid: Uuid,
    pub friendly_name: String,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub maximum_packet_length: u32,
}

impl PtpIpConnectionConfig {
    pub fn validate(&self) -> Result<(), PtpIpError> {
        if self.host.trim().is_empty() {
            return Err(PtpIpError::InvalidInput("host is empty".into()));
        }
        if self.port == 0 {
            return Err(PtpIpError::InvalidInput("port must be non-zero".into()));
        }
        if self.friendly_name.is_empty() || self.friendly_name.encode_utf16().count() > 64 {
            return Err(PtpIpError::InvalidInput(
                "friendly name must contain 1 to 64 UTF-16 code units".into(),
            ));
        }
        if self.connect_timeout.is_zero() || self.request_timeout.is_zero() {
            return Err(PtpIpError::InvalidInput("timeouts must be non-zero".into()));
        }
        validate_packet_length(PACKET_HEADER_LENGTH as u32, self.maximum_packet_length)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponderInfo {
    pub connection_number: u32,
    pub guid: [u8; 16],
    pub friendly_name: String,
    pub protocol_version: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationResult {
    pub transaction_id: u32,
    pub response: OperationResponse,
    pub data_length: u64,
}

#[derive(Debug)]
pub struct PtpIpSession {
    command: Mutex<TcpStream>,
    event: Mutex<TcpStream>,
    responder: ResponderInfo,
    next_transaction: AtomicU32,
    request_timeout: Duration,
    closed: AtomicBool,
    maximum_packet_length: u32,
}

impl PtpIpSession {
    #[instrument(skip(config), fields(host = %config.host, port = config.port))]
    pub async fn connect(config: PtpIpConnectionConfig) -> Result<Self, PtpIpError> {
        config.validate()?;
        let address = format!("{}:{}", config.host, config.port);
        let mut command = timeout(config.connect_timeout, TcpStream::connect(&address))
            .await
            .map_err(|_| PtpIpError::Timeout("connect command channel".into()))??;
        command.set_nodelay(true)?;
        let request = Packet::InitCommandRequest {
            guid: config.initiator_guid.as_bytes().to_owned(),
            friendly_name: config.friendly_name,
            protocol_version: PROTOCOL_VERSION_1_0,
        };
        write_packet(&mut command, &request, config.request_timeout).await?;
        let ack = read_packet(
            &mut command,
            config.request_timeout,
            config.maximum_packet_length,
        )
        .await?;
        let responder = match ack {
            Packet::InitCommandAck {
                connection_number,
                responder_guid,
                friendly_name,
                protocol_version,
            } => ResponderInfo {
                connection_number,
                guid: responder_guid,
                friendly_name,
                protocol_version,
            },
            Packet::InitFail { reason } => return Err(PtpIpError::InitializationFailed(reason)),
            other => {
                return Err(PtpIpError::UnexpectedPacket {
                    expected: PacketType::InitCommandAck,
                    actual: other.packet_type(),
                });
            }
        };

        let mut event = timeout(config.connect_timeout, TcpStream::connect(&address))
            .await
            .map_err(|_| PtpIpError::Timeout("connect event channel".into()))??;
        event.set_nodelay(true)?;
        write_packet(
            &mut event,
            &Packet::InitEventRequest {
                connection_number: responder.connection_number,
            },
            config.request_timeout,
        )
        .await?;
        let event_ack = read_packet(
            &mut event,
            config.request_timeout,
            config.maximum_packet_length,
        )
        .await?;
        match event_ack {
            Packet::InitEventAck => {}
            Packet::InitFail { reason } => return Err(PtpIpError::InitializationFailed(reason)),
            other => {
                return Err(PtpIpError::UnexpectedPacket {
                    expected: PacketType::InitEventAck,
                    actual: other.packet_type(),
                });
            }
        }
        debug!(
            connection_number = responder.connection_number,
            "PTP/IP session initialized"
        );
        Ok(Self {
            command: Mutex::new(command),
            event: Mutex::new(event),
            responder,
            next_transaction: AtomicU32::new(1),
            request_timeout: config.request_timeout,
            closed: AtomicBool::new(false),
            maximum_packet_length: config.maximum_packet_length,
        })
    }

    pub const fn responder(&self) -> &ResponderInfo {
        &self.responder
    }
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    pub async fn execute_no_data(
        &self,
        operation_code: u16,
        parameters: &[u32],
    ) -> Result<OperationResult, PtpIpError> {
        let mut sink = tokio::io::sink();
        self.execute_data_in_to(
            operation_code,
            parameters,
            DataPhase::None,
            0,
            &mut sink,
            || false,
        )
        .await
    }

    pub async fn execute_data_in(
        &self,
        operation_code: u16,
        parameters: &[u32],
        maximum_bytes: u64,
    ) -> Result<(OperationResult, Vec<u8>), PtpIpError> {
        let mut data = Vec::new();
        let result = self
            .execute_data_in_to(
                operation_code,
                parameters,
                DataPhase::DataIn,
                maximum_bytes,
                &mut data,
                || false,
            )
            .await?;
        if result.data_length > maximum_bytes {
            return Err(PtpIpError::DataLimitExceeded {
                actual: result.data_length,
                maximum: maximum_bytes,
            });
        }
        Ok((result, data))
    }

    pub async fn execute_data_in_to<W, F>(
        &self,
        operation_code: u16,
        parameters: &[u32],
        phase: DataPhase,
        maximum_bytes: u64,
        writer: &mut W,
        is_cancelled: F,
    ) -> Result<OperationResult, PtpIpError>
    where
        W: AsyncWrite + Unpin + Send + ?Sized,
        F: Fn() -> bool + Send + Sync,
    {
        if self.is_closed() {
            return Err(PtpIpError::Closed);
        }
        validate_parameters(parameters, 5)?;
        let transaction_id = self.next_transaction.fetch_add(1, Ordering::AcqRel);
        let mut stream = self.command.lock().await;
        write_packet(
            &mut *stream,
            &Packet::OperationRequest {
                data_phase: phase,
                operation_code,
                transaction_id,
                parameters: parameters.to_vec(),
            },
            self.request_timeout,
        )
        .await?;

        let mut declared_total = None;
        let mut written = 0_u64;
        let mut cancelling = false;
        loop {
            if is_cancelled() && !cancelling {
                let _ = write_packet(
                    &mut *stream,
                    &Packet::Cancel { transaction_id },
                    self.request_timeout,
                )
                .await;
                cancelling = true;
            }
            let read_timeout = if cancelling {
                self.request_timeout.min(Duration::from_secs(1))
            } else {
                self.request_timeout
            };
            let read_result = {
                let read_future =
                    read_packet(&mut *stream, read_timeout, self.maximum_packet_length);
                tokio::pin!(read_future);
                loop {
                    tokio::select! {
                        result = &mut read_future => break Some(result),
                        () = tokio::time::sleep(Duration::from_millis(25)) => {
                            if is_cancelled() && !cancelling { break None; }
                        }
                    }
                }
            };
            let Some(read_result) = read_result else {
                let _ = write_packet(
                    &mut *stream,
                    &Packet::Cancel { transaction_id },
                    self.request_timeout,
                )
                .await;
                cancelling = true;
                continue;
            };
            let packet = match read_result {
                Ok(packet) => packet,
                Err(_) if cancelling => {
                    self.closed.store(true, Ordering::Release);
                    let _ = stream.shutdown().await;
                    return Err(PtpIpError::Cancelled);
                }
                Err(error) => return Err(error),
            };
            match packet {
                Packet::StartData {
                    transaction_id: id,
                    total_length,
                } if id == transaction_id => {
                    if phase != DataPhase::DataIn {
                        return Err(PtpIpError::UnexpectedData);
                    }
                    if total_length > maximum_bytes {
                        return Err(PtpIpError::DataLimitExceeded {
                            actual: total_length,
                            maximum: maximum_bytes,
                        });
                    }
                    declared_total = Some(total_length);
                }
                Packet::Data {
                    transaction_id: id,
                    bytes,
                }
                | Packet::EndData {
                    transaction_id: id,
                    bytes,
                } if id == transaction_id => {
                    if phase != DataPhase::DataIn {
                        return Err(PtpIpError::UnexpectedData);
                    }
                    let next_written = written
                        .checked_add(bytes.len() as u64)
                        .ok_or(PtpIpError::LengthOverflow)?;
                    if next_written > maximum_bytes {
                        return Err(PtpIpError::DataLimitExceeded {
                            actual: next_written,
                            maximum: maximum_bytes,
                        });
                    }
                    if !cancelling {
                        writer.write_all(&bytes).await?;
                    }
                    written = next_written;
                }
                Packet::OperationResponse(response)
                    if response.transaction_id == transaction_id =>
                {
                    if cancelling {
                        return Err(PtpIpError::Cancelled);
                    }
                    if let Some(total) = declared_total
                        && total != written
                    {
                        return Err(PtpIpError::DataLengthMismatch {
                            declared: total,
                            actual: written,
                        });
                    }
                    return Ok(OperationResult {
                        transaction_id,
                        response,
                        data_length: written,
                    });
                }
                other => {
                    return Err(PtpIpError::TransactionMismatch {
                        expected: transaction_id,
                        actual: packet_transaction_id(&other),
                    });
                }
            }
        }
    }

    pub async fn next_event(&self) -> Result<Event, PtpIpError> {
        if self.is_closed() {
            return Err(PtpIpError::Closed);
        }
        let mut stream = self.event.lock().await;
        match read_packet(
            &mut *stream,
            self.request_timeout,
            self.maximum_packet_length,
        )
        .await?
        {
            Packet::Event(event) => Ok(event),
            other => Err(PtpIpError::UnexpectedPacket {
                expected: PacketType::Event,
                actual: other.packet_type(),
            }),
        }
    }

    pub async fn disconnect(&self) -> Result<(), PtpIpError> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let mut command = self.command.lock().await;
        let mut event = self.event.lock().await;
        let command_result = command.shutdown().await;
        let event_result = event.shutdown().await;
        command_result.and(event_result).map_err(PtpIpError::from)
    }
}

pub async fn read_packet<R: AsyncRead + Unpin>(
    reader: &mut R,
    duration: Duration,
    maximum: u32,
) -> Result<Packet, PtpIpError> {
    let mut header = [0_u8; PACKET_HEADER_LENGTH];
    timeout(duration, reader.read_exact(&mut header))
        .await
        .map_err(|_| PtpIpError::Timeout("read packet header".into()))??;
    let declared = read_u32(&header, 0);
    validate_packet_length(declared, maximum)?;
    let mut frame = Vec::with_capacity(declared as usize);
    frame.extend_from_slice(&header);
    frame.resize(declared as usize, 0);
    timeout(
        duration,
        reader.read_exact(&mut frame[PACKET_HEADER_LENGTH..]),
    )
    .await
    .map_err(|_| PtpIpError::Timeout("read packet body".into()))??;
    Packet::decode(&frame)
}

pub async fn write_packet<W: AsyncWrite + Unpin>(
    writer: &mut W,
    packet: &Packet,
    duration: Duration,
) -> Result<(), PtpIpError> {
    let bytes = packet.encode()?;
    timeout(duration, writer.write_all(&bytes))
        .await
        .map_err(|_| PtpIpError::Timeout("write packet".into()))??;
    timeout(duration, writer.flush())
        .await
        .map_err(|_| PtpIpError::Timeout("flush packet".into()))??;
    Ok(())
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PtpIpError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("I/O error: {0}")]
    Io(String),
    #[error("operation timed out: {0}")]
    Timeout(String),
    #[error("PTP/IP session is closed")]
    Closed,
    #[error("operation cancelled")]
    Cancelled,
    #[error("length overflow")]
    LengthOverflow,
    #[error("invalid packet length {0}")]
    InvalidPacketLength(u32),
    #[error("packet length {declared} exceeds maximum {maximum}")]
    PacketTooLarge { declared: u32, maximum: u32 },
    #[error("packet declared {declared} bytes but received {actual}")]
    LengthMismatch { declared: usize, actual: usize },
    #[error("incomplete packet: expected {expected} bytes, got {actual}")]
    UnexpectedEof { expected: usize, actual: usize },
    #[error("unknown packet type {0}")]
    UnknownPacketType(u32),
    #[error("invalid UTF-16 friendly name")]
    InvalidUtf16,
    #[error("unterminated UTF-16 friendly name")]
    UnterminatedUtf16,
    #[error("too many parameters: {actual}, maximum {maximum}")]
    TooManyParameters { actual: usize, maximum: usize },
    #[error("malformed parameter payload")]
    MalformedParameters,
    #[error("unexpected packet: expected {expected:?}, got {actual:?}")]
    UnexpectedPacket {
        expected: PacketType,
        actual: PacketType,
    },
    #[error("initialization failed with reason 0x{0:08x}")]
    InitializationFailed(u32),
    #[error("transaction mismatch: expected {expected}, got {actual:?}")]
    TransactionMismatch { expected: u32, actual: Option<u32> },
    #[error("unexpected operation data")]
    UnexpectedData,
    #[error("operation data length mismatch: declared {declared}, got {actual}")]
    DataLengthMismatch { declared: u64, actual: u64 },
    #[error("operation data length {actual} exceeds maximum {maximum}")]
    DataLimitExceeded { actual: u64, maximum: u64 },
}

impl From<io::Error> for PtpIpError {
    fn from(value: io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

fn validate_packet_length(declared: u32, maximum: u32) -> Result<(), PtpIpError> {
    if maximum < PACKET_HEADER_LENGTH as u32 {
        return Err(PtpIpError::InvalidPacketLength(maximum));
    }
    if declared < PACKET_HEADER_LENGTH as u32 {
        return Err(PtpIpError::InvalidPacketLength(declared));
    }
    if declared > maximum {
        return Err(PtpIpError::PacketTooLarge { declared, maximum });
    }
    Ok(())
}

fn validate_parameters(parameters: &[u32], maximum: usize) -> Result<(), PtpIpError> {
    if parameters.len() > maximum {
        Err(PtpIpError::TooManyParameters {
            actual: parameters.len(),
            maximum,
        })
    } else {
        Ok(())
    }
}

fn encode_parameters(output: &mut Vec<u8>, parameters: &[u32]) {
    for value in parameters {
        output.extend_from_slice(&value.to_le_bytes());
    }
}

fn decode_parameters(bytes: &[u8], maximum: usize) -> Result<Vec<u32>, PtpIpError> {
    if !bytes.len().is_multiple_of(4) {
        return Err(PtpIpError::MalformedParameters);
    }
    let values: Vec<_> = bytes
        .chunks_exact(4)
        .map(|chunk| read_u32(chunk, 0))
        .collect();
    validate_parameters(&values, maximum)?;
    Ok(values)
}

fn encode_utf16z(output: &mut Vec<u8>, value: &str) -> Result<(), PtpIpError> {
    if value.contains('\0') {
        return Err(PtpIpError::InvalidInput(
            "friendly name contains NUL".into(),
        ));
    }
    for unit in value.encode_utf16() {
        output.extend_from_slice(&unit.to_le_bytes());
    }
    output.extend_from_slice(&0_u16.to_le_bytes());
    Ok(())
}

fn decode_utf16z(bytes: &[u8], offset: &mut usize) -> Result<String, PtpIpError> {
    let mut units = Vec::new();
    loop {
        if bytes.len().saturating_sub(*offset) < 2 {
            return Err(PtpIpError::UnterminatedUtf16);
        }
        let unit = read_u16(bytes, *offset);
        *offset += 2;
        if unit == 0 {
            break;
        }
        units.push(unit);
        if units.len() > 255 {
            return Err(PtpIpError::InvalidInput("friendly name is too long".into()));
        }
    }
    String::from_utf16(&units).map_err(|_| PtpIpError::InvalidUtf16)
}

fn decode_payload(kind: PacketType, payload: &[u8]) -> Result<Packet, PtpIpError> {
    match kind {
        PacketType::InitCommandRequest => {
            require_len(payload, 16 + 2 + 4)?;
            let guid = payload[..16].try_into().expect("fixed GUID slice");
            let mut offset = 16;
            let friendly_name = decode_utf16z(payload, &mut offset)?;
            require_exact_remaining(payload, offset, 4)?;
            Ok(Packet::InitCommandRequest {
                guid,
                friendly_name,
                protocol_version: read_u32(payload, offset),
            })
        }
        PacketType::InitCommandAck => {
            require_len(payload, 4 + 16 + 2 + 4)?;
            let connection_number = read_u32(payload, 0);
            let responder_guid = payload[4..20].try_into().expect("fixed GUID slice");
            let mut offset = 20;
            let friendly_name = decode_utf16z(payload, &mut offset)?;
            require_exact_remaining(payload, offset, 4)?;
            Ok(Packet::InitCommandAck {
                connection_number,
                responder_guid,
                friendly_name,
                protocol_version: read_u32(payload, offset),
            })
        }
        PacketType::InitEventRequest => {
            require_exact(payload, 4)?;
            Ok(Packet::InitEventRequest {
                connection_number: read_u32(payload, 0),
            })
        }
        PacketType::InitEventAck => {
            require_exact(payload, 0)?;
            Ok(Packet::InitEventAck)
        }
        PacketType::InitFail => {
            require_exact(payload, 4)?;
            Ok(Packet::InitFail {
                reason: read_u32(payload, 0),
            })
        }
        PacketType::OperationRequest => {
            require_len(payload, 10)?;
            let data_phase = match read_u32(payload, 0) {
                1 => DataPhase::None,
                2 => DataPhase::DataOut,
                3 => DataPhase::DataIn,
                value => {
                    return Err(PtpIpError::InvalidInput(format!(
                        "invalid data phase {value}"
                    )));
                }
            };
            Ok(Packet::OperationRequest {
                data_phase,
                operation_code: read_u16(payload, 4),
                transaction_id: read_u32(payload, 6),
                parameters: decode_parameters(&payload[10..], 5)?,
            })
        }
        PacketType::OperationResponse => {
            require_len(payload, 6)?;
            Ok(Packet::OperationResponse(
                OperationResponse::new(
                    read_u16(payload, 0),
                    read_u32(payload, 2),
                    decode_parameters(&payload[6..], 5)?,
                )
                .map_err(|error| PtpIpError::InvalidInput(error.to_string()))?,
            ))
        }
        PacketType::Event => {
            require_len(payload, 6)?;
            Ok(Packet::Event(
                Event::new(
                    read_u16(payload, 0),
                    read_u32(payload, 2),
                    decode_parameters(&payload[6..], 3)?,
                )
                .map_err(|error| PtpIpError::InvalidInput(error.to_string()))?,
            ))
        }
        PacketType::StartData => {
            require_exact(payload, 12)?;
            Ok(Packet::StartData {
                transaction_id: read_u32(payload, 0),
                total_length: read_u64(payload, 4),
            })
        }
        PacketType::Data => {
            require_len(payload, 4)?;
            Ok(Packet::Data {
                transaction_id: read_u32(payload, 0),
                bytes: payload[4..].to_vec(),
            })
        }
        PacketType::EndData => {
            require_len(payload, 4)?;
            Ok(Packet::EndData {
                transaction_id: read_u32(payload, 0),
                bytes: payload[4..].to_vec(),
            })
        }
        PacketType::Cancel => {
            require_exact(payload, 4)?;
            Ok(Packet::Cancel {
                transaction_id: read_u32(payload, 0),
            })
        }
        PacketType::ProbeRequest => {
            require_exact(payload, 0)?;
            Ok(Packet::ProbeRequest)
        }
        PacketType::ProbeResponse => {
            require_exact(payload, 0)?;
            Ok(Packet::ProbeResponse)
        }
    }
}

fn packet_transaction_id(packet: &Packet) -> Option<u32> {
    match packet {
        Packet::OperationRequest { transaction_id, .. }
        | Packet::StartData { transaction_id, .. }
        | Packet::Data { transaction_id, .. }
        | Packet::EndData { transaction_id, .. }
        | Packet::Cancel { transaction_id } => Some(*transaction_id),
        Packet::OperationResponse(value) => Some(value.transaction_id),
        Packet::Event(value) => Some(value.transaction_id),
        _ => None,
    }
}

fn require_len(bytes: &[u8], minimum: usize) -> Result<(), PtpIpError> {
    if bytes.len() < minimum {
        Err(PtpIpError::UnexpectedEof {
            expected: minimum,
            actual: bytes.len(),
        })
    } else {
        Ok(())
    }
}
fn require_exact(bytes: &[u8], expected: usize) -> Result<(), PtpIpError> {
    if bytes.len() == expected {
        Ok(())
    } else {
        Err(PtpIpError::LengthMismatch {
            declared: expected,
            actual: bytes.len(),
        })
    }
}
fn require_exact_remaining(bytes: &[u8], offset: usize, expected: usize) -> Result<(), PtpIpError> {
    require_exact(&bytes[offset..], expected)
}
fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}
fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("u32 slice"))
}
fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("u64 slice"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use tokio::io::{AsyncWriteExt, duplex};

    #[test]
    fn init_command_request_matches_golden_bytes() {
        let packet = Packet::InitCommandRequest {
            guid: [1; 16],
            friendly_name: "A".into(),
            protocol_version: PROTOCOL_VERSION_1_0,
        };
        let encoded = packet.encode().unwrap();
        assert_eq!(&encoded[..8], &[32, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(Packet::decode(&encoded), Ok(packet));
    }

    #[test]
    fn fragmented_and_coalesced_packets_decode() {
        let first = Packet::ProbeRequest.encode().unwrap();
        let second = Packet::Cancel { transaction_id: 7 }.encode().unwrap();
        let mut decoder = PacketDecoder::default();
        decoder.push(&first[..3]);
        assert_eq!(decoder.next_packet().unwrap(), None);
        decoder.push(&first[3..]);
        decoder.push(&second);
        assert_eq!(decoder.next_packet().unwrap(), Some(Packet::ProbeRequest));
        assert_eq!(
            decoder.next_packet().unwrap(),
            Some(Packet::Cancel { transaction_id: 7 })
        );
    }

    #[tokio::test]
    async fn read_packet_accepts_fragmented_io() {
        let (mut writer, mut reader) = duplex(64);
        let expected = Packet::ProbeResponse;
        let bytes = expected.encode().unwrap();
        tokio::spawn(async move {
            for chunk in bytes.chunks(2) {
                writer.write_all(chunk).await.unwrap();
            }
        });
        assert_eq!(
            read_packet(&mut reader, Duration::from_secs(1), 1024)
                .await
                .unwrap(),
            expected
        );
    }

    proptest! {
        #[test]
        fn operation_request_round_trips(code in any::<u16>(), transaction in any::<u32>(), params in proptest::collection::vec(any::<u32>(), 0..=5)) {
            let packet = Packet::OperationRequest { data_phase: DataPhase::DataIn, operation_code: code, transaction_id: transaction, parameters: params };
            let bytes = packet.encode().unwrap();
            prop_assert_eq!(Packet::decode(&bytes), Ok(packet));
        }
    }
}

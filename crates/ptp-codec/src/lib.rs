//! 标准图片传输协议（PTP）容器的严格编解码器。
//!
//! PTP 容器使用 12 字节小端序头，后接 32 位参数或不透明数据。
//! 本 crate 有意保持数值形式的操作码、响应码和事件码开放，
//! 从而无需让编解码器了解每种相机型号，也能表示
//! 厂商扩展。

use bytes::BytesMut;
use std::fmt;
use thiserror::Error;

/// PTP 容器头的字节数。
pub const CONTAINER_HEADER_LEN: usize = 12;

/// PTP 规定命令和响应最多允许的参数数量。
pub const MAX_OPERATION_PARAMETERS: usize = 5;

/// PTP 规定事件最多允许的参数数量。
pub const MAX_EVENT_PARAMETERS: usize = 3;

/// 默认接受的最大容器大小（64 MiB）。
pub const DEFAULT_MAX_CONTAINER_LENGTH: u32 = 64 * 1024 * 1024;

/// 解码不受信任的相机输入时采用的限制。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodecLimits {
    /// 最大完整容器大小，包含其 12 字节头。
    pub max_container_length: u32,
}

impl Default for CodecLimits {
    fn default() -> Self {
        Self {
            max_container_length: DEFAULT_MAX_CONTAINER_LENGTH,
        }
    }
}

/// PTP 容器头中存储的类型字段。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ContainerType {
    /// 操作请求。
    Command = 1,
    /// 操作数据。
    Data = 2,
    /// 操作响应。
    Response = 3,
    /// 异步事件。
    Event = 4,
}

impl TryFrom<u16> for ContainerType {
    type Error = CodecError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::Command),
            2 => Ok(Self::Data),
            3 => Ok(Self::Response),
            4 => Ok(Self::Event),
            other => Err(CodecError::UnknownContainerType(other)),
        }
    }
}

/// 操作请求容器。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationRequest {
    /// PTP 操作码，包括厂商自定义代码。
    pub operation_code: u16,
    /// 由发起端分配的事务标识符。
    pub transaction_id: u32,
    /// 零到五个 32 位操作参数。
    pub parameters: Vec<u32>,
}

impl OperationRequest {
    /// 构造已校验的请求。
    pub fn new(
        operation_code: u16,
        transaction_id: u32,
        parameters: impl Into<Vec<u32>>,
    ) -> Result<Self, CodecError> {
        let request = Self {
            operation_code,
            transaction_id,
            parameters: parameters.into(),
        };
        validate_parameter_count(ContainerType::Command, request.parameters.len())?;
        Ok(request)
    }
}

/// 与操作关联的数据容器。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataContainer {
    /// 此数据所属的操作码。
    pub operation_code: u16,
    /// 与请求共用的事务标识符。
    pub transaction_id: u32,
    /// 未解释的操作数据。
    pub data: Vec<u8>,
}

/// 操作响应容器。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationResponse {
    /// PTP 响应码，包括厂商自定义代码。
    pub response_code: u16,
    /// 与请求共用的事务标识符。
    pub transaction_id: u32,
    /// 零到五个响应参数。
    pub parameters: Vec<u32>,
}

impl OperationResponse {
    /// 构造已校验的响应。
    pub fn new(
        response_code: u16,
        transaction_id: u32,
        parameters: impl Into<Vec<u32>>,
    ) -> Result<Self, CodecError> {
        let response = Self {
            response_code,
            transaction_id,
            parameters: parameters.into(),
        };
        validate_parameter_count(ContainerType::Response, response.parameters.len())?;
        Ok(response)
    }
}

/// 异步事件容器。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Event {
    /// PTP 事件码，包括厂商自定义代码。
    pub event_code: u16,
    /// 关联的事务；无关联事务时使用相机的哨兵值。
    pub transaction_id: u32,
    /// 零到三个事件参数。
    pub parameters: Vec<u32>,
}

impl Event {
    /// 构造已校验的事件。
    pub fn new(
        event_code: u16,
        transaction_id: u32,
        parameters: impl Into<Vec<u32>>,
    ) -> Result<Self, CodecError> {
        let event = Self {
            event_code,
            transaction_id,
            parameters: parameters.into(),
        };
        validate_parameter_count(ContainerType::Event, event.parameters.len())?;
        Ok(event)
    }
}

/// 任意标准 PTP 容器。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Container {
    /// 操作请求。
    OperationRequest(OperationRequest),
    /// 操作数据。
    Data(DataContainer),
    /// 操作响应。
    OperationResponse(OperationResponse),
    /// 异步事件。
    Event(Event),
}

impl Container {
    /// 返回线上的容器类型。
    pub const fn container_type(&self) -> ContainerType {
        match self {
            Self::OperationRequest(_) => ContainerType::Command,
            Self::Data(_) => ContainerType::Data,
            Self::OperationResponse(_) => ContainerType::Response,
            Self::Event(_) => ContainerType::Event,
        }
    }

    /// 返回容器头中存储的操作码、响应码或事件码。
    pub const fn code(&self) -> u16 {
        match self {
            Self::OperationRequest(value) => value.operation_code,
            Self::Data(value) => value.operation_code,
            Self::OperationResponse(value) => value.response_code,
            Self::Event(value) => value.event_code,
        }
    }

    /// 返回容器头中存储的事务标识符。
    pub const fn transaction_id(&self) -> u32 {
        match self {
            Self::OperationRequest(value) => value.transaction_id,
            Self::Data(value) => value.transaction_id,
            Self::OperationResponse(value) => value.transaction_id,
            Self::Event(value) => value.transaction_id,
        }
    }

    /// 使用默认限制编码此容器。
    pub fn encode(&self) -> Result<Vec<u8>, CodecError> {
        self.encode_with_limits(CodecLimits::default())
    }

    /// 强制执行 `limits` 后编码此容器。
    pub fn encode_with_limits(&self, limits: CodecLimits) -> Result<Vec<u8>, CodecError> {
        validate_limits(limits)?;

        let payload_len = match self {
            Self::OperationRequest(value) => {
                validate_parameter_count(ContainerType::Command, value.parameters.len())?;
                parameter_bytes(value.parameters.len())?
            }
            Self::Data(value) => value.data.len(),
            Self::OperationResponse(value) => {
                validate_parameter_count(ContainerType::Response, value.parameters.len())?;
                parameter_bytes(value.parameters.len())?
            }
            Self::Event(value) => {
                validate_parameter_count(ContainerType::Event, value.parameters.len())?;
                parameter_bytes(value.parameters.len())?
            }
        };
        let total_len = CONTAINER_HEADER_LEN
            .checked_add(payload_len)
            .ok_or(CodecError::LengthOverflow)?;
        let total_len_u32 = u32::try_from(total_len).map_err(|_| CodecError::LengthOverflow)?;
        validate_declared_length(total_len_u32, limits)?;

        let mut output = Vec::with_capacity(total_len);
        output.extend_from_slice(&total_len_u32.to_le_bytes());
        output.extend_from_slice(&(self.container_type() as u16).to_le_bytes());
        output.extend_from_slice(&self.code().to_le_bytes());
        output.extend_from_slice(&self.transaction_id().to_le_bytes());

        match self {
            Self::OperationRequest(value) => write_parameters(&mut output, &value.parameters),
            Self::Data(value) => output.extend_from_slice(&value.data),
            Self::OperationResponse(value) => write_parameters(&mut output, &value.parameters),
            Self::Event(value) => write_parameters(&mut output, &value.parameters),
        }
        Ok(output)
    }

    /// 使用默认限制精确解码一个完整容器。
    ///
    /// 拒绝额外字节；处理字节流时请使用 [`ContainerDecoder`]。
    pub fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode_with_limits(bytes, CodecLimits::default())
    }

    /// 强制执行 `limits` 后精确解码一个完整容器。
    pub fn decode_with_limits(bytes: &[u8], limits: CodecLimits) -> Result<Self, CodecError> {
        validate_limits(limits)?;
        if bytes.len() < 4 {
            return Err(CodecError::UnexpectedEof {
                expected: 4,
                actual: bytes.len(),
            });
        }
        let declared = read_u32(bytes, 0);
        validate_declared_length(declared, limits)?;
        let declared_usize = declared as usize;
        match bytes.len().cmp(&declared_usize) {
            std::cmp::Ordering::Less => Err(CodecError::UnexpectedEof {
                expected: declared_usize,
                actual: bytes.len(),
            }),
            std::cmp::Ordering::Greater => Err(CodecError::TrailingBytes {
                declared: declared_usize,
                actual: bytes.len(),
            }),
            std::cmp::Ordering::Equal => decode_complete(bytes),
        }
    }
}

/// 用于任意拆分或合并字节流的增量解码器。
#[derive(Clone)]
pub struct ContainerDecoder {
    buffer: BytesMut,
    limits: CodecLimits,
}

impl fmt::Debug for ContainerDecoder {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContainerDecoder")
            .field("buffered_bytes", &self.buffer.len())
            .field("limits", &self.limits)
            .finish()
    }
}

impl Default for ContainerDecoder {
    fn default() -> Self {
        Self::new(CodecLimits::default())
    }
}

impl ContainerDecoder {
    /// 使用 `limits` 创建解码器。
    pub fn new(limits: CodecLimits) -> Self {
        Self {
            buffer: BytesMut::new(),
            limits,
        }
    }

    /// 追加从传输层接收的字节。
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// 当前等待组成完整帧的字节数。
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    /// 移除所有已缓冲字节。
    pub fn clear(&mut self) {
        self.buffer.clear();
    }

    /// 若已有完整容器，则解码下一个容器。
    pub fn next_container(&mut self) -> Result<Option<Container>, CodecError> {
        validate_limits(self.limits)?;
        if self.buffer.len() < 4 {
            return Ok(None);
        }
        let declared = read_u32(&self.buffer, 0);
        validate_declared_length(declared, self.limits)?;
        let frame_len = declared as usize;
        if self.buffer.len() < frame_len {
            return Ok(None);
        }
        let frame = self.buffer.split_to(frame_len);
        Container::decode_with_limits(&frame, self.limits).map(Some)
    }
}

/// 校验、编码或解码容器时产生的错误。
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CodecError {
    /// 配置的最大值小于 PTP 头。
    #[error("invalid maximum container length {0}; it must be at least {CONTAINER_HEADER_LEN}")]
    InvalidLimit(u32),
    /// 长度字段或分配大小溢出其线上表示范围。
    #[error("container length cannot be represented on the wire")]
    LengthOverflow,
    /// 声明的容器长度小于容器头。
    #[error("invalid PTP container length {declared}; minimum is {CONTAINER_HEADER_LEN}")]
    InvalidLength { declared: u32 },
    /// 容器超过配置的内存分配上限。
    #[error("PTP container length {declared} exceeds configured maximum {maximum}")]
    LengthLimitExceeded { declared: u32, maximum: u32 },
    /// 输入在声明帧结束前已结束。
    #[error("incomplete PTP container: expected {expected} bytes, got {actual}")]
    UnexpectedEof { expected: usize, actual: usize },
    /// 精确解码在声明帧后又收到额外字节。
    #[error("PTP container declared {declared} bytes but input contains {actual}")]
    TrailingBytes { declared: usize, actual: usize },
    /// 线上的容器类型不属于标准 PTP。
    #[error("unknown PTP container type 0x{0:04x}")]
    UnknownContainerType(u16),
    /// 含参数的载荷长度不是 32 位值大小的整数倍。
    #[error("{container_type:?} payload has invalid byte length {payload_length}")]
    MisalignedParameters {
        container_type: ContainerType,
        payload_length: usize,
    },
    /// 容器携带的参数数量超过 PTP 允许的上限。
    #[error("{container_type:?} has {actual} parameters; maximum is {maximum}")]
    TooManyParameters {
        container_type: ContainerType,
        actual: usize,
        maximum: usize,
    },
}

fn validate_limits(limits: CodecLimits) -> Result<(), CodecError> {
    if limits.max_container_length < CONTAINER_HEADER_LEN as u32 {
        Err(CodecError::InvalidLimit(limits.max_container_length))
    } else {
        Ok(())
    }
}

fn validate_declared_length(declared: u32, limits: CodecLimits) -> Result<(), CodecError> {
    if declared < CONTAINER_HEADER_LEN as u32 {
        return Err(CodecError::InvalidLength { declared });
    }
    if declared > limits.max_container_length {
        return Err(CodecError::LengthLimitExceeded {
            declared,
            maximum: limits.max_container_length,
        });
    }
    Ok(())
}

fn parameter_bytes(count: usize) -> Result<usize, CodecError> {
    count.checked_mul(4).ok_or(CodecError::LengthOverflow)
}

fn parameter_limit(container_type: ContainerType) -> usize {
    match container_type {
        ContainerType::Event => MAX_EVENT_PARAMETERS,
        ContainerType::Command | ContainerType::Response => MAX_OPERATION_PARAMETERS,
        ContainerType::Data => 0,
    }
}

fn validate_parameter_count(
    container_type: ContainerType,
    actual: usize,
) -> Result<(), CodecError> {
    let maximum = parameter_limit(container_type);
    if actual > maximum {
        Err(CodecError::TooManyParameters {
            container_type,
            actual,
            maximum,
        })
    } else {
        Ok(())
    }
}

fn decode_complete(bytes: &[u8]) -> Result<Container, CodecError> {
    debug_assert!(bytes.len() >= CONTAINER_HEADER_LEN);
    let container_type = ContainerType::try_from(read_u16(bytes, 4))?;
    let code = read_u16(bytes, 6);
    let transaction_id = read_u32(bytes, 8);
    let payload = &bytes[CONTAINER_HEADER_LEN..];

    match container_type {
        ContainerType::Command => Ok(Container::OperationRequest(OperationRequest {
            operation_code: code,
            transaction_id,
            parameters: decode_parameters(container_type, payload)?,
        })),
        ContainerType::Data => Ok(Container::Data(DataContainer {
            operation_code: code,
            transaction_id,
            data: payload.to_vec(),
        })),
        ContainerType::Response => Ok(Container::OperationResponse(OperationResponse {
            response_code: code,
            transaction_id,
            parameters: decode_parameters(container_type, payload)?,
        })),
        ContainerType::Event => Ok(Container::Event(Event {
            event_code: code,
            transaction_id,
            parameters: decode_parameters(container_type, payload)?,
        })),
    }
}

fn decode_parameters(container_type: ContainerType, bytes: &[u8]) -> Result<Vec<u32>, CodecError> {
    if !bytes.len().is_multiple_of(4) {
        return Err(CodecError::MisalignedParameters {
            container_type,
            payload_length: bytes.len(),
        });
    }
    let count = bytes.len() / 4;
    validate_parameter_count(container_type, count)?;
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| read_u32(chunk, 0))
        .collect())
}

fn write_parameters(output: &mut Vec<u8>, parameters: &[u32]) {
    for parameter in parameters {
        output.extend_from_slice(&parameter.to_le_bytes());
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::collection::vec;
    use proptest::prelude::*;

    #[test]
    fn operation_request_matches_golden_little_endian_bytes() {
        let container = Container::OperationRequest(
            OperationRequest::new(0x1009, 0x1234_5678, [0x0102_0304, 0xaabb_ccdd])
                .expect("valid request"),
        );
        assert_eq!(
            container.encode().expect("encode"),
            vec![
                0x14, 0x00, 0x00, 0x00, // 长度
                0x01, 0x00, // 命令
                0x09, 0x10, // 操作码
                0x78, 0x56, 0x34, 0x12, // 事务
                0x04, 0x03, 0x02, 0x01, // 参数 1
                0xdd, 0xcc, 0xbb, 0xaa, // 参数 2
            ]
        );
    }

    #[test]
    fn fragmented_frame_waits_for_completion() {
        let expected = Container::OperationResponse(
            OperationResponse::new(0x2001, 7, [42]).expect("valid response"),
        );
        let encoded = expected.encode().expect("encode");
        let mut decoder = ContainerDecoder::default();
        for byte in &encoded[..encoded.len() - 1] {
            decoder.push(std::slice::from_ref(byte));
            assert_eq!(decoder.next_container().expect("decode"), None);
        }
        decoder.push(&encoded[encoded.len() - 1..]);
        assert_eq!(decoder.next_container().expect("decode"), Some(expected));
    }

    #[test]
    fn coalesced_frames_are_returned_individually() {
        let first = Container::Event(Event::new(0x4002, u32::MAX, [3]).expect("event"));
        let second = Container::Data(DataContainer {
            operation_code: 0x1009,
            transaction_id: 4,
            data: vec![1, 2, 3],
        });
        let mut bytes = first.encode().expect("first");
        bytes.extend(second.encode().expect("second"));

        let mut decoder = ContainerDecoder::default();
        decoder.push(&bytes);
        assert_eq!(decoder.next_container().expect("first decode"), Some(first));
        assert_eq!(
            decoder.next_container().expect("second decode"),
            Some(second)
        );
        assert_eq!(decoder.next_container().expect("empty"), None);
    }

    #[test]
    fn malformed_lengths_are_rejected_before_allocation() {
        let mut too_short = vec![0; CONTAINER_HEADER_LEN];
        too_short[..4].copy_from_slice(&11_u32.to_le_bytes());
        assert_eq!(
            Container::decode(&too_short),
            Err(CodecError::InvalidLength { declared: 11 })
        );

        let mut excessive = Vec::from((DEFAULT_MAX_CONTAINER_LENGTH + 1).to_le_bytes());
        excessive.resize(CONTAINER_HEADER_LEN, 0);
        assert!(matches!(
            Container::decode(&excessive),
            Err(CodecError::LengthLimitExceeded { .. })
        ));
    }

    #[test]
    fn strict_decode_rejects_trailing_bytes_and_misaligned_parameters() {
        let request =
            Container::OperationRequest(OperationRequest::new(0x1001, 1, []).expect("request"));
        let mut encoded = request.encode().expect("encode");
        encoded.push(0);
        assert!(matches!(
            Container::decode(&encoded),
            Err(CodecError::TrailingBytes { .. })
        ));

        let malformed = [
            13, 0, 0, 0, // 长度
            1, 0, // 命令
            1, 0x10, // 代码
            1, 0, 0, 0, // 事务
            0xff,
        ];
        assert!(matches!(
            Container::decode(&malformed),
            Err(CodecError::MisalignedParameters { .. })
        ));
    }

    proptest! {
        #[test]
        fn request_round_trip_is_lossless(
            code in any::<u16>(),
            transaction_id in any::<u32>(),
            parameters in vec(any::<u32>(), 0..=MAX_OPERATION_PARAMETERS),
        ) {
            let expected = Container::OperationRequest(OperationRequest {
                operation_code: code,
                transaction_id,
                parameters,
            });
            let encoded = expected.encode().expect("valid generated request");
            prop_assert_eq!(Container::decode(&encoded), Ok(expected));
        }

        #[test]
        fn arbitrary_fragment_boundaries_preserve_frames(
            payload in vec(any::<u8>(), 0..4096),
            split in 0_usize..5000,
        ) {
            let expected = Container::Data(DataContainer {
                operation_code: 0x1009,
                transaction_id: 99,
                data: payload,
            });
            let encoded = expected.encode().expect("encode");
            let split = split.min(encoded.len());
            let mut decoder = ContainerDecoder::default();
            decoder.push(&encoded[..split]);
            let first = decoder.next_container().expect("partial decode");
            if split == encoded.len() {
                prop_assert_eq!(first, Some(expected));
            } else {
                prop_assert_eq!(first, None);
                decoder.push(&encoded[split..]);
                prop_assert_eq!(decoder.next_container().expect("complete decode"), Some(expected));
            }
        }
    }
}

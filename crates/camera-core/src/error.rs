use serde::{Deserialize, Serialize};
use std::io;
use thiserror::Error;

/// 供 UI 层使用的稳定、与语言无关的错误类别。
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraErrorCode {
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionKind {
    LocalNetwork,
    Photos,
    Files,
}

/// 错误包含可操作的上下文，但绝不包含连接凭据。
#[derive(Clone, Debug, Deserialize, Eq, Error, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CameraError {
    #[error("invalid input: {message}")]
    InvalidInput { message: String },
    #[error("connection failed: {message}")]
    ConnectionFailed { message: String },
    #[error("{operation} timed out")]
    Timeout { operation: String },
    #[error("camera is busy")]
    CameraBusy,
    #[error("camera is asleep or unavailable")]
    CameraAsleep,
    #[error("unsupported firmware {found}; expected {supported}")]
    UnsupportedFirmware { found: String, supported: String },
    #[error("unsupported operation: {operation}")]
    UnsupportedOperation { operation: String },
    #[error("protocol error: {message}")]
    Protocol { message: String },
    #[error("media object not found")]
    ObjectNotFound,
    #[error("permission denied: {permission:?}")]
    PermissionDenied { permission: PermissionKind },
    #[error(
        "not enough space (required {required_bytes} bytes, available {available_bytes} bytes)"
    )]
    InsufficientSpace {
        required_bytes: u64,
        available_bytes: u64,
    },
    #[error("I/O error ({kind}): {message}")]
    Io { kind: String, message: String },
    #[error("camera session was interrupted: {message}")]
    SessionInterrupted { message: String },
    #[error("operation cancelled")]
    Cancelled,
    #[error("destination already exists")]
    Conflict,
    #[error("internal error: {message}")]
    Internal { message: String },
}

impl CameraError {
    pub const fn code(&self) -> CameraErrorCode {
        match self {
            Self::InvalidInput { .. } => CameraErrorCode::InvalidInput,
            Self::ConnectionFailed { .. } => CameraErrorCode::ConnectionFailed,
            Self::Timeout { .. } => CameraErrorCode::Timeout,
            Self::CameraBusy => CameraErrorCode::CameraBusy,
            Self::CameraAsleep => CameraErrorCode::CameraAsleep,
            Self::UnsupportedFirmware { .. } => CameraErrorCode::UnsupportedFirmware,
            Self::UnsupportedOperation { .. } => CameraErrorCode::UnsupportedOperation,
            Self::Protocol { .. } => CameraErrorCode::Protocol,
            Self::ObjectNotFound => CameraErrorCode::ObjectNotFound,
            Self::PermissionDenied { .. } => CameraErrorCode::PermissionDenied,
            Self::InsufficientSpace { .. } => CameraErrorCode::InsufficientSpace,
            Self::Io { .. } => CameraErrorCode::Io,
            Self::SessionInterrupted { .. } => CameraErrorCode::SessionInterrupted,
            Self::Cancelled => CameraErrorCode::Cancelled,
            Self::Conflict => CameraErrorCode::Conflict,
            Self::Internal { .. } => CameraErrorCode::Internal,
        }
    }

    pub fn from_io(error: io::Error) -> Self {
        Self::Io {
            kind: format!("{:?}", error.kind()),
            message: error.to_string(),
        }
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        Self::Protocol {
            message: message.into(),
        }
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput {
            message: message.into(),
        }
    }
}

impl From<io::Error> for CameraError {
    fn from(value: io::Error) -> Self {
        Self::from_io(value)
    }
}

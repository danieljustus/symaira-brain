//! Gateway errors and stable broker-failure classifications.

use std::error::Error;
use std::fmt;

use symbrain_broker::BrokerError;

/// The broker operations a gateway backend can expose to the domain layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    /// A child returned a JSON-RPC error.
    Rpc { code: i64, message: String },
    /// A child did not answer before the operation deadline.
    Timeout { op: String },
    /// A child closed or exited before completing the operation.
    Closed { op: String, detail: String },
    /// The connection or caller cancelled the operation.
    Cancelled { op: String },
    /// A protocol, I/O, or decoding failure that is not retryable by the gateway.
    Internal(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rpc { code, message } => write!(f, "broker: rpc error {code}: {message}"),
            Self::Timeout { op } => write!(f, "broker: {op}: timeout"),
            Self::Closed { op, detail } if detail.is_empty() => {
                write!(f, "broker: {op}: child closed")
            }
            Self::Closed { op, detail } => {
                write!(f, "broker: {op}: child closed: {detail}")
            }
            Self::Cancelled { op } => write!(f, "broker: {op}: cancelled"),
            Self::Internal(message) => f.write_str(message),
        }
    }
}

impl Error for BackendError {}

impl From<BrokerError> for BackendError {
    fn from(error: BrokerError) -> Self {
        match error {
            BrokerError::Rpc { code, message } => Self::Rpc { code, message },
            BrokerError::Timeout { op } => Self::Timeout { op },
            BrokerError::Closed { op, detail } => Self::Closed { op, detail },
            BrokerError::Cancelled { op } => Self::Cancelled { op },
            other => Self::Internal(other.to_string()),
        }
    }
}

/// Error raised by catalog construction or a routed tool call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GatewayError {
    /// Two namespaced child tools have the same exposed name.
    Collision(symbrain_catalog::CollisionError),
    /// A profile policy could not be evaluated.
    Policy(String),
    /// A routed backend failed.
    Backend {
        server: String,
        source: BackendError,
    },
    /// Arguments were not a JSON object.
    InvalidArguments(String),
    /// No exposed server tool matched the requested name.
    UnknownTool(String),
    /// A catalog entry references a backend that is no longer available.
    UnknownServer(String),
    /// A response payload could not be serialized.
    Serialization(String),
    /// The connection was cancelled while an embedded operation was running.
    Cancelled,
}

impl fmt::Display for GatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Collision(error) => error.fmt(f),
            Self::Policy(message) => f.write_str(message),
            Self::Backend { source, .. } => source.fmt(f),
            Self::InvalidArguments(message) => write!(f, "gateway: decode arguments: {message}"),
            Self::UnknownTool(name) => write!(f, "Unknown tool: {name}"),
            Self::UnknownServer(name) => write!(f, "server \"{name}\" not found"),
            Self::Serialization(message) => write!(f, "gateway: serialize response: {message}"),
            Self::Cancelled => f.write_str("gateway: cancelled"),
        }
    }
}

impl Error for GatewayError {}

impl From<symbrain_catalog::CollisionError> for GatewayError {
    fn from(error: symbrain_catalog::CollisionError) -> Self {
        Self::Collision(error)
    }
}

impl From<symbrain_memory::StoreError> for GatewayError {
    fn from(error: symbrain_memory::StoreError) -> Self {
        match error {
            symbrain_memory::StoreError::Cancelled => Self::Cancelled,
            other => Self::Policy(format!("embedded memory store: {other}")),
        }
    }
}

impl GatewayError {
    /// Returns the audit classification matching the Go gateway contract.
    #[must_use]
    pub fn classification(&self) -> symbrain_audit::Classification {
        match self {
            Self::Backend { source, .. } => match source {
                BackendError::Rpc { .. } => symbrain_audit::Classification {
                    category: "rpc".to_string(),
                    retryable: false,
                },
                BackendError::Timeout { .. } => symbrain_audit::Classification {
                    category: "timeout".to_string(),
                    retryable: true,
                },
                BackendError::Closed { .. } => symbrain_audit::Classification {
                    category: "closed".to_string(),
                    retryable: true,
                },
                BackendError::Cancelled { .. } => symbrain_audit::Classification {
                    category: "cancelled".to_string(),
                    retryable: false,
                },
                BackendError::Internal(_) => symbrain_audit::Classification {
                    category: "internal".to_string(),
                    retryable: false,
                },
            },
            Self::Collision(_)
            | Self::Policy(_)
            | Self::InvalidArguments(_)
            | Self::UnknownTool(_)
            | Self::UnknownServer(_)
            | Self::Serialization(_)
            | Self::Cancelled => symbrain_audit::Classification {
                category: "internal".to_string(),
                retryable: false,
            },
        }
    }
}

/// Joins child text blocks exactly as the Go gateway does.
#[must_use]
pub fn join_content(content: &[symbrain_broker::ContentBlock]) -> String {
    content
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

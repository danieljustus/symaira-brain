//! Generic JSON-RPC dispatch loop for MCP stdio transports.

#[path = "loop_helpers.rs"]
mod loop_helpers;

use std::fmt;
use std::io::{self, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;

use crate::{CODE_INVALID_REQUEST, CODE_PARSE_ERROR, Decoder, FrameError, Request};

use self::loop_helpers::{
    dispatch_one, panic_safe_error_response, record_async_error, write_shared,
};

/// A dispatch failure that must be surfaced to the caller rather than silently
/// converted into a tool result. Domain adapters use this for response-build
/// and other fatal errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchError {
    message: String,
}

impl DispatchError {
    /// Creates a dispatch failure with a stable human-readable message.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for DispatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for DispatchError {}

/// Cancellation and connection-scoped state supplied to every dispatch.
///
/// The context deliberately contains no gateway or broker types. It is a
/// borrow of the connection's cancellation flag, so a handler can observe EOF
/// or caller cancellation without creating a dependency from the MCP crate to
/// a domain adapter.
#[derive(Clone, Copy)]
pub struct DispatchContext<'a> {
    cancelled: &'a AtomicBool,
}

impl<'a> DispatchContext<'a> {
    /// Creates a context backed by an externally-owned cancellation flag.
    #[must_use]
    pub const fn new(cancelled: &'a AtomicBool) -> Self {
        Self { cancelled }
    }

    /// Returns whether the connection has been cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// Error returned by [`Server::serve_io`].
#[derive(Debug)]
pub enum ServerError {
    /// A malformed or incomplete transport frame stopped the loop.
    Frame(FrameError),
    /// The domain dispatcher failed to produce a response.
    Dispatch(DispatchError),
    /// A response could not be serialized or written.
    Response(io::Error),
    /// The caller cancelled the connection.
    Cancelled,
}

impl fmt::Display for ServerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame(error) => write!(formatter, "mcp: read frame: {error}"),
            Self::Dispatch(error) => write!(formatter, "mcp: dispatch: {error}"),
            Self::Response(error) => write!(formatter, "mcp: write response: {error}"),
            Self::Cancelled => formatter.write_str("mcp: cancelled"),
        }
    }
}

impl std::error::Error for ServerError {}

/// Domain seam consumed by the generic MCP loop.
///
/// The MCP crate owns framing, notification silence, response-mode symmetry,
/// and transport error propagation. A domain crate owns method semantics and
/// supplies its serializable response type, keeping the MCP layer independent
/// of any gateway implementation.
pub trait Dispatcher {
    /// The response wire type owned by the domain adapter.
    type Response: Serialize;

    /// Dispatches one non-notification JSON-RPC request.
    ///
    /// The context is connection-scoped and is also passed to concurrent tool
    /// calls. A handler must cooperate by checking `is_cancelled()`.
    ///
    /// Returning `Ok(None)` is useful for domain-level notifications, although
    /// the server filters protocol notifications before reaching this method.
    ///
    /// # Errors
    /// A returned error aborts the connection without manufacturing a success
    /// or tool-error response.
    fn dispatch(
        &self,
        request: &Request,
        context: DispatchContext<'_>,
    ) -> Result<Option<Self::Response>, DispatchError>;

    /// Builds a JSON-RPC error response for parse or invalid-request errors
    /// detected by the loop.
    fn error_response(&self, id: Value, code: i64, message: String) -> Self::Response;
}

impl<D: Dispatcher + ?Sized> Dispatcher for &D {
    type Response = D::Response;

    fn dispatch(
        &self,
        request: &Request,
        context: DispatchContext<'_>,
    ) -> Result<Option<Self::Response>, DispatchError> {
        (*self).dispatch(request, context)
    }

    fn error_response(&self, id: Value, code: i64, message: String) -> Self::Response {
        (*self).error_response(id, code, message)
    }
}

/// A reusable MCP server loop over an arbitrary domain dispatcher.
pub struct Server<D> {
    dispatcher: D,
}

impl<D> Server<D> {
    /// Creates a server around a domain dispatcher.
    #[must_use]
    pub const fn new(dispatcher: D) -> Self {
        Self { dispatcher }
    }

    /// Returns the wrapped dispatcher.
    #[must_use]
    pub fn into_inner(self) -> D {
        self.dispatcher
    }
}

impl<D: Dispatcher + Sync> Server<D> {
    /// Serves line-delimited and Content-Length-framed requests until EOF or
    /// a fatal transport/dispatch error.
    ///
    /// `tools/call` requests are dispatched concurrently. The writer is shared
    /// through one mutex and each complete frame is written under that lock, so
    /// headers and bodies cannot interleave. Non-tool methods remain synchronous.
    /// In-flight calls are joined before EOF, parse errors, and read failures
    /// are returned.
    ///
    /// # Errors
    /// Returns framing, dispatch, serialization, writer, or cancellation errors.
    pub fn serve_io<R: Read, W: Write + Send>(
        &self,
        reader: R,
        writer: &mut W,
    ) -> Result<(), ServerError> {
        static NEVER_CANCEL: AtomicBool = AtomicBool::new(false);
        self.serve_io_with_cancel(&NEVER_CANCEL, reader, writer)
    }

    /// Serves a connection while checking an externally-owned cancellation
    /// flag between reads and exposing it to every in-flight dispatch. A
    /// blocking reader must be closed by the caller to interrupt a read that
    /// is already in progress.
    ///
    /// # Errors
    /// Returns framing, dispatch, serialization, writer, or cancellation
    /// errors.
    #[allow(clippy::too_many_lines)]
    pub fn serve_io_with_cancel<R: Read, W: Write + Send>(
        &self,
        cancelled: &AtomicBool,
        reader: R,
        writer: &mut W,
    ) -> Result<(), ServerError> {
        let mut decoder = Decoder::new(BufReader::new(reader));
        let shared_writer = Arc::new(Mutex::new(writer));

        std::thread::scope(|scope| {
            let in_flight = Arc::new((Mutex::new(0_usize), std::sync::Condvar::new()));
            let async_error = Arc::new(Mutex::new(None::<ServerError>));
            let mut terminal_error = None;
            let context = DispatchContext::new(cancelled);

            let wait_for_in_flight = || -> Result<(), ServerError> {
                let (count, wake) = &*in_flight;
                let mut count = count.lock().map_err(|_| {
                    ServerError::Dispatch(DispatchError::new("in-flight lock poisoned"))
                })?;
                while *count != 0 {
                    count = wake.wait(count).map_err(|_| {
                        ServerError::Dispatch(DispatchError::new("in-flight lock poisoned"))
                    })?;
                }
                if let Some(error) = async_error
                    .lock()
                    .map_err(|_| {
                        ServerError::Dispatch(DispatchError::new("async error lock poisoned"))
                    })?
                    .take()
                {
                    return Err(error);
                }
                Ok(())
            };

            loop {
                if cancelled.load(Ordering::Acquire) {
                    break;
                }
                let next_frame = match decoder.read_request() {
                    Ok(value) => value,
                    Err(FrameError::Parse { mode, message }) => {
                        wait_for_in_flight()?;
                        let response = panic_safe_error_response(
                            &self.dispatcher,
                            Value::Null,
                            CODE_PARSE_ERROR,
                            format!("Parse error: {message}"),
                        )
                        .map_err(ServerError::Dispatch)?;
                        write_shared(&shared_writer, mode, &response)
                            .map_err(ServerError::Response)?;
                        continue;
                    }
                    Err(FrameError::InvalidRequest { mode }) => {
                        wait_for_in_flight()?;
                        let response = panic_safe_error_response(
                            &self.dispatcher,
                            Value::Null,
                            CODE_INVALID_REQUEST,
                            "Invalid Request".to_string(),
                        )
                        .map_err(ServerError::Dispatch)?;
                        write_shared(&shared_writer, mode, &response)
                            .map_err(ServerError::Response)?;
                        continue;
                    }
                    Err(error) => {
                        terminal_error = Some(ServerError::Frame(error));
                        break;
                    }
                };
                let Some((request, mode)) = next_frame else {
                    break;
                };
                if request.is_notification() {
                    continue;
                }

                if request.method == "tools/call" {
                    let request = request.clone();
                    let dispatcher = &self.dispatcher;
                    let shared_writer = Arc::clone(&shared_writer);
                    let in_flight = Arc::clone(&in_flight);
                    let async_error = Arc::clone(&async_error);
                    {
                        let (count, _) = &*in_flight;
                        let mut count = count.lock().map_err(|_| {
                            ServerError::Dispatch(DispatchError::new("in-flight lock poisoned"))
                        })?;
                        *count += 1;
                    }
                    scope.spawn(move || {
                        let result = (|| -> Result<(), ServerError> {
                            let response = dispatch_one(dispatcher, &request, context)
                                .map_err(ServerError::Dispatch)?;
                            if let Some(response) = response {
                                write_shared(&shared_writer, mode, &response)
                                    .map_err(ServerError::Response)?;
                            }
                            Ok(())
                        })();
                        if let Err(error) = result {
                            record_async_error(&async_error, error);
                        }
                        let (count, wake) = &*in_flight;
                        match count.lock() {
                            Ok(mut count) => *count = count.saturating_sub(1),
                            Err(poisoned) => {
                                let mut count = poisoned.into_inner();
                                *count = count.saturating_sub(1);
                                record_async_error(
                                    &async_error,
                                    ServerError::Dispatch(DispatchError::new(
                                        "in-flight lock poisoned",
                                    )),
                                );
                            }
                        }
                        wake.notify_one();
                    });
                } else {
                    let response = dispatch_one(&self.dispatcher, &request, context)
                        .map_err(ServerError::Dispatch)?;
                    if let Some(response) = response {
                        write_shared(&shared_writer, mode, &response)
                            .map_err(ServerError::Response)?;
                    }
                }
            }

            wait_for_in_flight()?;
            if let Some(error) = terminal_error {
                return Err(error);
            }
            if cancelled.load(Ordering::Acquire) {
                return Err(ServerError::Cancelled);
            }
            Ok(())
        })
    }

    /// Alias for [`Self::serve_io`] for callers that want a short entry point.
    ///
    /// # Errors
    /// Returns framing, dispatch, serialization, writer, or cancellation
    /// errors.
    pub fn serve<R: Read, W: Write + Send>(
        &self,
        reader: R,
        writer: &mut W,
    ) -> Result<(), ServerError> {
        self.serve_io(reader, writer)
    }
}

/// Runs a dispatcher directly without first constructing [`Server`].
///
/// # Errors
/// Returns the same errors as [`Server::serve_io`].
pub fn serve_io<D: Dispatcher + Sync, R: Read, W: Write + Send>(
    dispatcher: &D,
    reader: R,
    writer: &mut W,
) -> Result<(), ServerError> {
    Server::new(dispatcher).serve_io(reader, writer)
}

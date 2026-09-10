//! JSON-RPC models and dual-mode MCP stdio framing.

#![deny(unsafe_code)]

mod frame;
mod model;
mod server;

pub use frame::{
    Decoder, FrameError, MAX_HEADER_BYTES, MAX_HEADER_LINES, MAX_MESSAGE_BYTES, Mode, write_message,
};
pub use model::{
    CODE_INTERNAL_ERROR, CODE_INVALID_PARAMS, CODE_INVALID_REQUEST, CODE_METHOD_NOT_FOUND,
    CODE_PARSE_ERROR, ClientInfo, ErrorObject, InitializeParams, JSONRPC_VERSION, PROTOCOL_VERSION,
    Request, RequestMessage, Response,
};
pub use server::{DispatchContext, DispatchError, Dispatcher, Server, ServerError, serve_io};

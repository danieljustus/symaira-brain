use std::io::{self, BufWriter, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::Serialize;
use serde::ser::{Error as _, Serializer};
use serde_json::{Value, json};
use symbrain_mcp::{
    CODE_INTERNAL_ERROR, CODE_INVALID_PARAMS, CODE_PARSE_ERROR, DispatchContext, DispatchError,
    Dispatcher, FrameError, Request, Response, Server, ServerError, write_message,
};

include!("fixtures/gateway_protocol_tests_part1.rs");
include!("fixtures/gateway_protocol_tests_part2.rs");

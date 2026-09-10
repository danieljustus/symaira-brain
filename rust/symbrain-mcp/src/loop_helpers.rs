use super::{DispatchContext, DispatchError, Dispatcher, ServerError};
use crate::{CODE_INTERNAL_ERROR, Request, write_message};
use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

pub(super) fn record_async_error(slot: &Mutex<Option<ServerError>>, error: ServerError) {
    match slot.lock() {
        Ok(mut slot) => {
            if slot.is_none() {
                *slot = Some(error);
            }
        }
        Err(poisoned) => {
            let mut slot = poisoned.into_inner();
            if slot.is_none() {
                *slot = Some(ServerError::Dispatch(DispatchError::new(
                    "async error lock poisoned",
                )));
            }
        }
    }
}

pub(super) fn dispatch_one<D: Dispatcher + ?Sized>(
    dispatcher: &D,
    request: &Request,
    context: DispatchContext<'_>,
) -> Result<Option<D::Response>, DispatchError> {
    if let Ok(result) = catch_unwind(AssertUnwindSafe(|| dispatcher.dispatch(request, context))) {
        result
    } else {
        let id = request.id.clone().unwrap_or(Value::Null);
        panic_safe_error_response(
            dispatcher,
            id,
            CODE_INTERNAL_ERROR,
            "Internal error: handler panicked".to_string(),
        )
        .map(Some)
    }
}

pub(super) fn panic_safe_error_response<D: Dispatcher + ?Sized>(
    dispatcher: &D,
    id: Value,
    code: i64,
    message: String,
) -> Result<D::Response, DispatchError> {
    catch_unwind(AssertUnwindSafe(|| {
        dispatcher.error_response(id, code, message)
    }))
    .map_err(|_| DispatchError::new("dispatcher panicked while building an error response"))
}

pub(super) fn write_shared<W: Write>(
    writer: &Arc<Mutex<&mut W>>,
    mode: crate::Mode,
    response: &impl Serialize,
) -> io::Result<()> {
    let mut writer = writer
        .lock()
        .map_err(|_| io::Error::other("writer lock poisoned"))?;
    write_message(&mut **writer, mode, response)
}

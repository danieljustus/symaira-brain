use serde_json::Value;
use symbrain_mcp::{DispatchContext, DispatchError, Dispatcher, Request};

use crate::{Gateway, GatewayResponse};

impl Dispatcher for Gateway {
    type Response = GatewayResponse;

    fn dispatch(
        &self,
        request: &Request,
        context: DispatchContext<'_>,
    ) -> Result<Option<Self::Response>, DispatchError> {
        self.handle_with_context(request, context)
            .map_err(|error| DispatchError::new(error.to_string()))
    }

    fn error_response(&self, id: Value, code: i64, message: String) -> Self::Response {
        GatewayResponse::error(id, code, message)
    }
}

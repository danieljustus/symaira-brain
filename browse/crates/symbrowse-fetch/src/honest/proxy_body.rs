//! Retain Hyper's length/trailer semantics and own its connection until body drop.
use hyper::body::{Body, Frame, Incoming, SizeHint};
use std::{
    pin::Pin,
    task::{Context, Poll},
};

pub(super) struct ConnectionTask(pub(super) tokio::task::JoinHandle<()>);
impl Drop for ConnectionTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) struct OwnedBody {
    body: Incoming,
    _task: ConnectionTask,
}
impl OwnedBody {
    pub(super) fn new(body: Incoming, task: ConnectionTask) -> Self {
        Self { body, _task: task }
    }
}
impl Body for OwnedBody {
    type Data = <Incoming as Body>::Data;
    type Error = hyper::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Pin::new(&mut self.body).poll_frame(cx)
    }
    fn is_end_stream(&self) -> bool {
        self.body.is_end_stream()
    }
    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}

//! Install, status, and synchronization primitives for rendered skills.

mod base;
mod cache;
mod core;
mod destination;
mod drift;
mod event;
mod lock;
mod marker;
mod modes;
mod ops;
mod replace;
mod replace_io;
mod status;
mod status_compare;
mod sync;
mod transaction;
mod uninstall;

pub use base::{
    BASE_MANIFEST, BASE_SCHEMA_VERSION, BaseFile, BaseManifest, base_path, base_path_for,
    base_path_for_scope, legacy_project_base_path, manifest_hashes, read_manifest,
    read_manifest_for, write_snapshot, write_snapshot_for, write_snapshot_for_scope,
};
pub use core::ModeChange;
pub use core::{
    InstallOptions, InstallResult, install_copy, install_path, install_path_for, install_rendered,
};
pub use drift::{DriftKind, DriftSummary, FileDrift, classify_drift, classify_file, summarize};
pub use event::{EVENT_MAX_BYTES, OperationEvent, record_event, record_event_at};
pub use marker::{
    MARKER_FILE, MARKER_SCHEMA_VERSION, Marker, MarkerState, encode_marker, new_marker,
    parse_marker, read_marker,
};
pub use replace::FaultPoint;
pub use status::{InstallStatus, StatusKind, StatusOptions, status};
pub use sync::{ConflictPolicy, SyncOptions, SyncResult, sync};
pub use uninstall::{uninstall, uninstall_skill};

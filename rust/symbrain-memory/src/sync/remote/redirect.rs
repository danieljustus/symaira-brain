//! RFC3986 reference handling following pinned net/url.ResolveReference.
// Source-derived compatibility logic: Go1.26.7 SDK, Go Authors, BSD-3-Clause.
// Retained license: scripts/memory-sync-oracle/reference/GO-SDK-LICENSE.

use super::{SyncError, Url};
pub(super) fn resolve(base: &Url, location: &str) -> Result<Url, SyncError> {
    let mut value = Url::parse(location)?;
    let absolute = !value.scheme.is_empty();
    if !absolute {
        value.scheme.clone_from(&base.scheme);
    }
    if absolute || !value.host.is_empty() || value.user.is_some() {
        value.path = path(&value.path, "");
        return Ok(value);
    }
    if !value.opaque.is_empty() {
        return Ok(value);
    }
    if value.path.is_empty() && !value.force_query && value.query.is_empty() {
        value.query.clone_from(&base.query);
        if value.fragment.is_empty() {
            value.fragment.clone_from(&base.fragment);
        }
    }
    if value.path.is_empty() && !base.opaque.is_empty() {
        value.opaque.clone_from(&base.opaque);
        return Ok(value);
    }
    value.host.clone_from(&base.host);
    value.user.clone_from(&base.user);
    value.password.clone_from(&base.password);
    value.path = path(&base.path, &value.path);
    Ok(value)
}
fn path(base: &str, reference: &str) -> String {
    let full = if reference.is_empty() {
        base.to_owned()
    } else if reference.starts_with('/') {
        reference.to_owned()
    } else {
        format!(
            "{}{reference}",
            base.rsplit_once('/')
                .map_or("", |(prefix, _)| &base[..prefix.len() + 1])
        )
    };
    if full.is_empty() {
        return String::new();
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in full.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    let mut value = parts.join("/");
    if !value.starts_with('/') {
        value.insert(0, '/');
    }
    if full.ends_with("/.") || full.ends_with("/..") {
        if !value.ends_with('/') {
            value.push('/');
        }
    }
    value
}

// Bounded same-handle reads shared by bundle loading and library discovery.

fn open_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits().cast_signed());
    options
}

fn open_read_with(
    root: &Dir,
    relative: &Path,
    name: &str,
    nofollow: bool,
) -> Result<cap_std::fs::File, SkillError> {
    let mut options = open_options();
    if nofollow {
        options.follow(FollowSymlinks::No);
    }
    root.open_with(relative, &options)
        .map_err(|error| SkillError(format!("read {name}: {}", go_io_error(&error))))
}

fn go_io_error(error: &std::io::Error) -> String {
    let message = error.to_string();
    #[cfg(windows)]
    if let Some(context) = message.strip_suffix(": no such file or directory") {
        return format!("{context}: The system cannot find the file specified.");
    }
    message
}

pub(crate) struct ReadBudget {
    remaining: u64,
    limit: u64,
    label: &'static str,
}

impl ReadBudget {
    pub(crate) fn new(limit: u64, label: &'static str) -> Self {
        Self {
            remaining: limit,
            limit,
            label,
        }
    }

    fn error(&self) -> SkillError {
        SkillError(format!(
            "{} exceeds maximum total size of {} bytes",
            self.label, self.limit
        ))
    }
}

fn read_control(root: &Dir, _anchor: &Path, name: &str) -> Result<Vec<u8>, SkillError> {
    let symlink = root
        .symlink_metadata(name)
        .is_ok_and(|metadata| metadata.file_type().is_symlink());
    read_limited(
        root,
        Path::new(name),
        &format!("read {name}"),
        MAX_INPUT_SIZE,
    )
    .map_err(|error| {
        if symlink {
            SkillError(format!(
                "{name} escapes skill root or is not a regular file"
            ))
        } else {
            error
        }
    })
}

pub(crate) fn read_skill_document(
    root: &Dir,
    relative: &Path,
    name: &str,
    budget: Option<&mut ReadBudget>,
) -> Result<Vec<u8>, SkillError> {
    let symlink = root
        .symlink_metadata(relative)
        .is_ok_and(|metadata| metadata.file_type().is_symlink());
    read_limited_inner(
        root,
        relative,
        name,
        MAX_INPUT_SIZE,
        None,
        budget,
        Some(crate::model::MAX_FRONTMATTER_SIZE),
        false,
    )
    .map_err(|error| {
        if symlink {
            SkillError(format!(
                "{name} escapes skill root or is not a regular file"
            ))
        } else {
            error
        }
    })
}

fn read_limited(
    root: &Dir,
    relative: &Path,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, SkillError> {
    read_limited_inner(root, relative, name, limit, None, None, None, false)
}

fn read_limited_expected(
    root: &Dir,
    relative: &Path,
    name: &str,
    limit: u64,
    expected_size: u64,
    budget: Option<&mut ReadBudget>,
) -> Result<Vec<u8>, SkillError> {
    read_limited_inner(
        root,
        relative,
        name,
        limit,
        Some(expected_size),
        budget,
        None,
        false,
    )
}

pub(crate) fn read_limited_nofollow(
    root: &Dir,
    relative: &Path,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, SkillError> {
    read_limited_inner(root, relative, name, limit, None, None, None, true)
}

#[allow(clippy::too_many_arguments)]
fn read_limited_inner(
    root: &Dir,
    relative: &Path,
    name: &str,
    limit: u64,
    expected_size: Option<u64>,
    mut budget: Option<&mut ReadBudget>,
    frontmatter_limit: Option<usize>,
    nofollow: bool,
) -> Result<Vec<u8>, SkillError> {
    let mut file = open_read_with(root, relative, name, nofollow)?;
    let metadata = file
        .metadata()
        .map_err(|error| SkillError(format!("read {name}: {error}")))?;
    if !metadata.is_file() {
        return Err(SkillError(format!("{name} must be a regular file")));
    }
    if metadata.len() > limit {
        return Err(SkillError(format!(
            "{name} exceeds maximum input size of {limit} bytes"
        )));
    }
    if expected_size.is_some_and(|expected| metadata.len() > expected) {
        return Err(SkillError(format!(
            "resource {name} changed since inventory"
        )));
    }
    let budget_remaining = budget.as_ref().map(|budget| budget.remaining);
    if budget_remaining.is_some_and(|remaining| metadata.len() > remaining) {
        return Err(budget.as_deref().expect("budget exists").error());
    }
    let read_limit = expected_size.map_or(limit, |expected| limit.min(expected));
    let read_limit = budget_remaining.map_or(read_limit, |remaining| read_limit.min(remaining));
    let scan_capacity = frontmatter_limit.map(|size| size.saturating_mul(2).saturating_add(16));
    let initial_capacity = metadata.len().min(read_limit);
    let initial_capacity =
        scan_capacity.map_or(initial_capacity, |scan| initial_capacity.min(scan as u64));
    let capacity = usize::try_from(initial_capacity)
        .map_err(|_| SkillError(format!("{name} input size cannot be represented")))?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut scan_frontmatter = frontmatter_limit.is_some();
    let mut chunk = [0_u8; 8192];

    loop {
        let remaining = read_limit.saturating_sub(bytes.len() as u64);
        if remaining == 0 {
            let mut extra = [0_u8; 1];
            match file.read(&mut extra) {
                Ok(0) => break,
                Ok(_) => {
                    if let Some(expected) = expected_size.filter(|expected| *expected <= read_limit)
                    {
                        return Err(SkillError(format!(
                            "resource {name} changed since inventory (expected {expected} bytes)"
                        )));
                    }
                    if let Some(remaining) = budget_remaining.filter(|left| *left <= read_limit)
                        && remaining < limit
                    {
                        return Err(budget.as_deref().expect("budget exists").error());
                    }
                    return Err(SkillError(format!(
                        "{name} exceeds maximum input size of {limit} bytes"
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(SkillError(format!("read {name}: {error}"))),
            }
        }
        let mut request = usize::try_from(remaining.min(chunk.len() as u64))
            .map_err(|_| SkillError(format!("{name} input size cannot be represented")))?;
        if scan_frontmatter {
            let remaining_scan = scan_capacity
                .expect("frontmatter scan has a bounded capacity")
                .saturating_sub(bytes.len());
            if remaining_scan == 0 {
                return Err(SkillError(format!(
                    "SKILL.md frontmatter exceeds maximum size of {} bytes",
                    crate::model::MAX_FRONTMATTER_SIZE
                )));
            }
            request = request.min(remaining_scan);
        }
        let read = match file.read(&mut chunk[..request]) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(SkillError(format!("read {name}: {error}"))),
        };
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        // Charge actual I/O even when the following header check rejects it.
        if let Some(budget) = budget.as_deref_mut() {
            budget.remaining = budget.remaining.saturating_sub(read as u64);
        }
        if scan_frontmatter {
            scan_frontmatter = !frontmatter_scan(&bytes)?;
            if scan_frontmatter && bytes.len() >= scan_capacity.expect("scan capacity exists") {
                return Err(SkillError(format!(
                    "SKILL.md frontmatter exceeds maximum size of {} bytes",
                    crate::model::MAX_FRONTMATTER_SIZE
                )));
            }
        }
    }
    Ok(bytes)
}

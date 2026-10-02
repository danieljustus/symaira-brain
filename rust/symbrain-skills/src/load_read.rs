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

#[derive(Debug)]
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

    fn error(&self) -> InputReadError {
        InputReadError::Budget(SkillError(format!(
            "{} exceeds maximum total size of {} bytes",
            self.label, self.limit
        )))
    }
}

#[derive(Debug)]
pub(crate) enum InputReadError {
    Read(SkillError),
    Rejected(SkillError),
    Budget(SkillError),
}

impl std::fmt::Display for InputReadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(error) | Self::Rejected(error) | Self::Budget(error) => {
                std::fmt::Display::fmt(error, formatter)
            }
        }
    }
}

impl From<InputReadError> for SkillError {
    fn from(error: InputReadError) -> Self {
        match error {
            InputReadError::Read(error)
            | InputReadError::Rejected(error)
            | InputReadError::Budget(error) => error,
        }
    }
}

fn read_control(root: &Dir, name: &str, budget: &mut ReadBudget) -> Result<Vec<u8>, SkillError> {
    let symlink = root
        .symlink_metadata(name)
        .is_ok_and(|metadata| metadata.file_type().is_symlink());
    read_limited_inner(
        root,
        Path::new(name),
        &format!("read {name}"),
        MAX_INPUT_SIZE,
        None,
        Some(budget),
        None,
        false,
    )
    .map_err(|error| {
        if symlink && matches!(error, InputReadError::Read(_)) {
            SkillError(format!(
                "{name} escapes skill root or is not a regular file"
            ))
        } else {
            error.into()
        }
    })
}

pub(crate) fn read_skill_document(
    root: &Dir,
    relative: &Path,
    name: &str,
    budget: Option<&mut ReadBudget>,
) -> Result<Vec<u8>, InputReadError> {
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
        if symlink && matches!(error, InputReadError::Read(_)) {
            InputReadError::Rejected(SkillError(format!(
                "{name} escapes skill root or is not a regular file"
            )))
        } else {
            error
        }
    })
}

#[cfg(test)]
fn read_limited(
    root: &Dir,
    relative: &Path,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, SkillError> {
    read_limited_inner(root, relative, name, limit, None, None, None, false).map_err(Into::into)
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
    .map_err(Into::into)
}

pub(crate) fn read_limited_nofollow(
    root: &Dir,
    relative: &Path,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, SkillError> {
    read_limited_inner(root, relative, name, limit, None, None, None, true).map_err(Into::into)
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
) -> Result<Vec<u8>, InputReadError> {
    let mut file = open_read_with(root, relative, name, nofollow).map_err(InputReadError::Read)?;
    let metadata = file
        .metadata()
        .map_err(|error| InputReadError::Read(SkillError(format!("read {name}: {error}"))))?;
    if !metadata.is_file() {
        return Err(InputReadError::Rejected(SkillError(format!(
            "{name} must be a regular file"
        ))));
    }
    if metadata.len() > limit {
        return Err(InputReadError::Rejected(SkillError(format!(
            "{name} exceeds maximum input size of {limit} bytes"
        ))));
    }
    if expected_size.is_some_and(|expected| metadata.len() > expected) {
        return Err(InputReadError::Rejected(SkillError(format!(
            "resource {name} changed since inventory"
        ))));
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
    let capacity = usize::try_from(initial_capacity).map_err(|_| {
        InputReadError::Rejected(SkillError(format!(
            "{name} input size cannot be represented"
        )))
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    let mut scan_frontmatter = frontmatter_limit.is_some();
    let mut chunk = [0_u8; 8192];

    loop {
        let remaining = read_limit.saturating_sub(bytes.len() as u64);
        if remaining == 0 {
            check_read_end(
                &file,
                name,
                limit,
                read_limit,
                expected_size,
                budget_remaining,
                budget.as_deref(),
            )?;
            break;
        }
        let mut request = usize::try_from(remaining.min(chunk.len() as u64)).map_err(|_| {
            InputReadError::Rejected(SkillError(format!(
                "{name} input size cannot be represented"
            )))
        })?;
        if scan_frontmatter {
            let remaining_scan = scan_capacity
                .expect("frontmatter scan has a bounded capacity")
                .saturating_sub(bytes.len());
            if remaining_scan == 0 {
                return Err(InputReadError::Rejected(SkillError(format!(
                    "SKILL.md frontmatter exceeds maximum size of {} bytes",
                    crate::model::MAX_FRONTMATTER_SIZE
                ))));
            }
            request = request.min(remaining_scan);
        }
        let read = match file.read(&mut chunk[..request]) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(InputReadError::Read(SkillError(format!(
                    "read {name}: {error}"
                ))));
            }
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
            scan_frontmatter = !frontmatter_scan(&bytes).map_err(InputReadError::Rejected)?;
            if scan_frontmatter && bytes.len() >= scan_capacity.expect("scan capacity exists") {
                return Err(InputReadError::Rejected(SkillError(format!(
                    "SKILL.md frontmatter exceeds maximum size of {} bytes",
                    crate::model::MAX_FRONTMATTER_SIZE
                ))));
            }
        }
    }
    Ok(bytes)
}

// Re-stat the same open handle instead of consuming an unbudgeted probe byte.
fn check_read_end(
    file: &cap_std::fs::File,
    name: &str,
    limit: u64,
    read_limit: u64,
    expected_size: Option<u64>,
    budget_remaining: Option<u64>,
    budget: Option<&ReadBudget>,
) -> Result<(), InputReadError> {
    let size = file
        .metadata()
        .map_err(|error| InputReadError::Read(SkillError(format!("read {name}: {error}"))))?
        .len();
    if size <= read_limit {
        return Ok(());
    }
    if let Some(expected) = expected_size.filter(|expected| *expected <= read_limit) {
        return Err(InputReadError::Rejected(SkillError(format!(
            "resource {name} changed since inventory (expected {expected} bytes)"
        ))));
    }
    if budget_remaining.is_some_and(|left| left <= read_limit && left < limit) {
        return Err(budget.expect("budget exists").error());
    }
    Err(InputReadError::Rejected(SkillError(format!(
        "{name} exceeds maximum input size of {limit} bytes"
    ))))
}

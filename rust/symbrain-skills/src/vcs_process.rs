//! Scoped bounded Git captures: archive bytes never share diagnostic stderr.
use crate::SkillError;
use std::path::Path;
use std::process::Command;

pub(super) fn run(dir: &Path, args: &[&str]) -> Result<Vec<u8>, SkillError> {
    capture(dir, args, false, &format!("git {}", args.join(" ")))
}

pub(super) fn archive(dir: &Path, rev: &str) -> Result<Vec<u8>, SkillError> {
    capture(
        dir,
        &["archive", "--format=tar", rev],
        true,
        &format!("git archive {rev}"),
    )
}

fn capture(dir: &Path, args: &[&str], separate: bool, label: &str) -> Result<Vec<u8>, SkillError> {
    let capture = tempfile::NamedTempFile::new().map_err(|error| SkillError(error.to_string()))?;
    let stdout = capture
        .reopen()
        .map_err(|error| SkillError(error.to_string()))?;
    let errors = separate
        .then(tempfile::NamedTempFile::new)
        .transpose()
        .map_err(|error| SkillError(error.to_string()))?;
    let stderr = if let Some(errors) = &errors {
        errors.reopen()
    } else {
        stdout.try_clone()
    }
    .map_err(|error| SkillError(error.to_string()))?;
    // Combined output shares one open-file description like Go's bytes.Buffer.
    // File capture avoids allocating child output before the skills read bound.
    let executable = crate::binary::executable(Path::new("git"))?
        .ok_or_else(|| SkillError("git binary not available".into()))?;
    #[cfg(windows)]
    if let Some(error) = crate::binary::batch_error(Path::new("git"), &executable.spelling) {
        return Err(SkillError(format!("{label}: {error}: ")));
    }
    let mut command = Command::new(&executable.owner);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.arg0(&executable.spelling);
    }
    let mut child = ChildOwner(
        command
            .args(args)
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env("LANGUAGE", "")
            .stdin(std::process::Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .spawn()
            .map_err(|error| SkillError(format!("{label}: {error}: ")))?,
    );
    let status = loop {
        let stderr_size = errors
            .as_ref()
            .map_or(Ok(0), |file| {
                file.as_file().metadata().map(|metadata| metadata.len())
            })
            .map_err(|error| SkillError(error.to_string()))?;
        if capture
            .as_file()
            .metadata()
            .map_err(|error| SkillError(error.to_string()))?
            .len()
            .saturating_add(stderr_size)
            > crate::MAX_TOTAL_RESOURCE_BYTES
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SkillError("git output exceeds skills input limit".into()));
        }
        match child
            .try_wait()
            .map_err(|error| SkillError(error.to_string()))?
        {
            Some(status) => break status,
            None => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    };
    let bytes = read_capture(&capture)?;
    let error_bytes = errors.as_ref().map(read_capture).transpose()?;
    let total = bytes
        .len()
        .saturating_add(error_bytes.as_ref().map_or(0, Vec::len));
    if u64::try_from(total).unwrap_or(u64::MAX) > crate::MAX_TOTAL_RESOURCE_BYTES {
        return Err(SkillError("git output exceeds skills input limit".into()));
    }
    if !status.success() {
        return Err(SkillError(format!(
            "{label}: exit status {}: {}",
            status.code().unwrap_or(1),
            String::from_utf8_lossy(error_bytes.as_deref().unwrap_or(&bytes)).trim()
        )));
    }
    Ok(bytes)
}

fn read_capture(capture: &tempfile::NamedTempFile) -> Result<Vec<u8>, SkillError> {
    let parent = capture
        .path()
        .parent()
        .ok_or_else(|| SkillError("git capture parent missing".into()))?;
    let root = cap_std::fs::Dir::open_ambient_dir(parent, ambient_authority::ambient_authority())
        .map_err(|error| SkillError(error.to_string()))?;
    let name = Path::new(
        capture
            .path()
            .file_name()
            .ok_or_else(|| SkillError("git capture name missing".into()))?,
    );
    crate::load::read_limited_nofollow(&root, name, "git output", crate::MAX_TOTAL_RESOURCE_BYTES)
}

// Only a per-skill git child is owned; no signal or global subprocess changes.
struct ChildOwner(std::process::Child);
impl std::ops::Deref for ChildOwner {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ChildOwner {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for ChildOwner {
    fn drop(&mut self) {
        if self.0.try_wait().is_ok_and(|status| status.is_some()) {
            return;
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

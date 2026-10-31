use crate::{
    Cancellation,
    history::{Event, parse_line},
};
use std::path::Path;

#[cfg(not(unix))]
pub(crate) fn read(_: &Path, _: &Cancellation) -> Result<(Vec<Event>, usize), ()> {
    Err(())
}

#[cfg(unix)]
pub(crate) fn read(home: &Path, cancel: &Cancellation) -> Result<(Vec<Event>, usize), ()> {
    read_json(home, "sessions", false, cancel, parse_line)
}

#[cfg(not(unix))]
pub(crate) fn read_json_reset(
    _: &Path,
    _: &str,
    _: bool,
    _: &Cancellation,
    _: Option<std::time::Instant>,
    _: impl FnMut(Option<&[u8]>, &Path) -> Result<Option<Vec<Event>>, ()>,
) -> Result<(Vec<Event>, usize), ()> {
    Err(())
}

pub(crate) fn read_json(
    home: &Path,
    subdir: &str,
    claude: bool,
    cancel: &Cancellation,
    mut parse: impl FnMut(&[u8]) -> Result<Vec<Event>, ()>,
) -> Result<(Vec<Event>, usize), ()> {
    read_json_reset(home, subdir, claude, cancel, None, |line, _| match line {
        Some(line) => parse(line).map(Some),
        None => Ok(Some(vec![])),
    })
}

#[cfg(unix)]
pub(crate) fn read_json_reset(
    home: &Path,
    subdir: &str,
    claude: bool,
    cancel: &Cancellation,
    deadline: Option<std::time::Instant>,
    mut parse: impl FnMut(Option<&[u8]>, &Path) -> Result<Option<Vec<Event>>, ()>,
) -> Result<(Vec<Event>, usize), ()> {
    use cap_std::fs::Dir;
    use rustix::fs::{Mode, OFlags, open, openat};
    let flags =
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let mut root = open("/", flags, Mode::empty()).map_err(|_| ())?;
    for component in home.components() {
        match component {
            std::path::Component::RootDir => (),
            std::path::Component::Normal(part) => {
                root = match openat(&root, part, flags, Mode::empty()) {
                    Ok(fd) => fd,
                    Err(rustix::io::Errno::NOENT) => return Ok((vec![], 0)),
                    Err(_) => return Err(()),
                };
            }
            _ => return Err(()),
        }
    }
    let sessions = match openat(&root, subdir, flags, Mode::empty()) {
        Ok(fd) => fd,
        Err(rustix::io::Errno::NOENT) => return Ok((vec![], 0)),
        Err(_) => return Err(()),
    };
    let mut state = State {
        cancel,
        claude,
        parse: &mut parse,
        deadline: deadline
            .unwrap_or_else(|| std::time::Instant::now() + std::time::Duration::from_secs(2)),
        entries: 0,
        files: 0,
        bytes: 0,
        lines: 0,
        excluded: 0,
        events: vec![],
    };
    state.walk(&Dir::from_std_file(sessions.into()), 0, Path::new(""))?;
    Ok((state.events, state.excluded))
}
#[cfg(unix)]
struct State<'a, F> {
    claude: bool,
    parse: &'a mut F,
    cancel: &'a Cancellation,
    deadline: std::time::Instant,
    entries: usize,
    files: usize,
    bytes: usize,
    lines: usize,
    excluded: usize,
    events: Vec<Event>,
}
#[cfg(unix)]
impl<F: FnMut(Option<&[u8]>, &Path) -> Result<Option<Vec<Event>>, ()>> State<'_, F> {
    fn check(&self) -> Result<(), ()> {
        if self.cancel.is_cancelled() || std::time::Instant::now() >= self.deadline {
            Err(())
        } else {
            Ok(())
        }
    }
    fn walk(&mut self, dir: &cap_std::fs::Dir, depth: usize, relative: &Path) -> Result<(), ()> {
        self.check()?;
        if depth > 12 {
            return Err(());
        }
        let mut children = Vec::new();
        for child in dir.entries().map_err(|_| ())? {
            self.check()?;
            self.entries += 1;
            if self.entries > 4096 {
                return Err(());
            }
            children.push(child.map_err(|_| ())?);
        }
        children.sort_by_key(cap_std::fs::DirEntry::file_name);
        for child in children {
            self.check()?;
            let kind = child.file_type().map_err(|_| ())?;
            if kind.is_symlink() {
                return Err(());
            }
            if kind.is_dir() {
                use rustix::fs::{Mode, OFlags, openat};
                let fd = openat(
                    dir,
                    child.file_name(),
                    OFlags::RDONLY
                        | OFlags::DIRECTORY
                        | OFlags::NOFOLLOW
                        | OFlags::NONBLOCK
                        | OFlags::CLOEXEC,
                    Mode::empty(),
                )
                .map_err(|_| ())?;
                let sub = cap_std::fs::Dir::from_std_file(fd.into());
                self.walk(&sub, depth + 1, &relative.join(child.file_name()))?;
            } else if (!self.claude && child.file_name() == "updates.jsonl")
                || (self.claude
                    && Path::new(&child.file_name())
                        .extension()
                        .is_some_and(|e| e == "jsonl"))
            {
                self.file(dir, &child.file_name(), &relative.join(child.file_name()))?;
            }
        }
        Ok(())
    }
    fn file(
        &mut self,
        dir: &cap_std::fs::Dir,
        name: &std::ffi::OsStr,
        relative: &Path,
    ) -> Result<(), ()> {
        use std::io::Read;
        if (self.parse)(None, relative)?.is_none() {
            return Ok(());
        }
        self.files += 1;
        if self.files > 128 {
            return Err(());
        }
        let mut options = cap_std::fs::OpenOptions::new();
        options.read(true);
        cap_std::fs::OpenOptionsExt::custom_flags(
            &mut options,
            libc::O_NOFOLLOW | libc::O_NONBLOCK,
        );
        let f = dir.open_with(name, &options).map_err(|_| ())?;
        let meta = f.metadata().map_err(|_| ())?;
        if !meta.is_file() || meta.len() > 4 << 20 {
            return Err(());
        }
        let mut data = Vec::new();
        f.take((4 << 20) + 1)
            .read_to_end(&mut data)
            .map_err(|_| ())?;
        self.bytes += data.len();
        if data.len() > 4 << 20 || self.bytes > 32 << 20 {
            return Err(());
        }
        let mut records = data.split(|b| *b == b'\n').peekable();
        while let Some(line) = records.next() {
            self.check()?;
            if records.peek().is_none() {
                if !line.is_empty() {
                    self.excluded += 1;
                }
                break;
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            self.lines += 1;
            if self.lines > 20000 || line.len() > 256 << 10 {
                return Err(());
            }
            match (self.parse)(Some(line), relative) {
                Ok(Some(events)) => self.events.extend(events),
                Ok(None) => (),
                Err(()) => self.excluded += 1,
            }
            if self.events.len() > 10000 {
                return Err(());
            }
        }
        Ok(())
    }
}

//! Isolated portable writer seam tests, prepared without operator-state access.
use std::ffi::OsString;
use std::io::{self, Write};

struct FailedWriter;
impl Write for FailedWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("owned report writer failed"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn owned_writer_child() {
    let Some(verb) = std::env::var_os("SKILLS_REPORT_TEST_VERB") else {
        return;
    };
    let verb = verb.to_str().unwrap();
    let format = if std::env::var_os("SKILLS_REPORT_TEST_JSON").is_some() {
        symbrain_core::output::OutputFormat::Json
    } else {
        symbrain_core::output::OutputFormat::Table
    };
    let mut stderr = Vec::new();
    let args = vec![OsString::from(verb)];
    assert_eq!(
        super::run(&args, &mut FailedWriter, &mut stderr, format),
        Some(1)
    );
    assert_eq!(
        stderr,
        format!("symbrain skills {verb}: format output: owned report writer failed\n").as_bytes()
    );
}

#[test]
fn all_six_report_owners_return_writer_error_in_both_formats() {
    let root = tempfile::tempdir().unwrap();
    for name in ["home", "config", "data", "cache", "project", "tmp"] {
        std::fs::create_dir(root.path().join(name)).unwrap();
    }
    for verb in ["list", "status", "targets", "log", "sync", "doctor"] {
        for json in [false, true] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child
                .env_clear()
                .arg("--exact")
                .arg("skills_cli::report_tests::owned_writer_child")
                .env("SKILLS_REPORT_TEST_VERB", verb)
                .env("HOME", root.path().join("home"))
                .env("USERPROFILE", root.path().join("home"))
                .env("XDG_CONFIG_HOME", root.path().join("config"))
                .env("XDG_DATA_HOME", root.path().join("data"))
                .env("XDG_CACHE_HOME", root.path().join("cache"))
                .env("TMPDIR", root.path().join("tmp"))
                .env("TEMP", root.path().join("tmp"))
                .env("TMP", root.path().join("tmp"))
                .env("PATH", "")
                .current_dir(root.path().join("project"));
            for key in ["SystemRoot", "windir"] {
                if let Some(value) = std::env::var_os(key) {
                    child.env(key, value);
                }
            }
            if json {
                child.env("SKILLS_REPORT_TEST_JSON", "1");
            }
            let out = child.output().unwrap();
            assert!(out.status.success(), "{verb}/{json}: {out:?}");
        }
    }
}

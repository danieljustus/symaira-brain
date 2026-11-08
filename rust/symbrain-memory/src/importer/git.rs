//! Git history importer (ported for #761 from `importer/git/git.go` at
//! v0.12.0): one session per non-merge commit of the repository the caller
//! names, read with the `git` binary (`git -C <repo> log`).

use super::{ImportResult, ImportedFact, PrivacyLevel, SessionImporter, SessionRef};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::process::Command;

/// Imports commits of one repository.
pub struct Git {
    repo_path: String,
    author: String,
}

impl Git {
    /// `author` (optional) is passed to `git log --author`.
    #[must_use]
    pub fn new(repo_path: &str, author: &str) -> Self {
        Self {
            repo_path: repo_path.to_owned(),
            author: author.to_owned(),
        }
    }

    fn run(args: &[String]) -> Result<String, String> {
        let output = Command::new("git")
            .args(args)
            .stderr(std::process::Stdio::null())
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(output.status.to_string());
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

#[derive(Deserialize, Default)]
struct Commit {
    #[serde(default)]
    author: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    parents: String,
}

fn parse_date(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw).map_or_else(
        |_| crate::sync_state::zero_time(),
        |t| t.with_timezone(&Utc),
    )
}

impl SessionImporter for Git {
    fn name(&self) -> &'static str {
        "git"
    }
    fn category(&self) -> Option<&str> {
        Some("code")
    }
    fn privacy_level(&self) -> Option<PrivacyLevel> {
        Some(PrivacyLevel::Public)
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        let mut args: Vec<String> = [
            "-C",
            &self.repo_path,
            "log",
            "--format=%H|%an|%ae|%aI|%s|%P",
            "--no-merges",
        ]
        .map(String::from)
        .into();
        if since > crate::sync_state::zero_time() {
            args.push(format!(
                "--since={}",
                since.to_rfc3339_opts(SecondsFormat::AutoSi, true)
            ));
        }
        if !self.author.is_empty() {
            args.push(format!("--author={}", self.author));
        }
        let out = Self::run(&args).map_err(|e| format!("git log failed: {e}"))?;
        let mut sessions = Vec::new();
        for line in out.lines().filter(|l| !l.is_empty()) {
            let parts: Vec<&str> = line.splitn(6, '|').collect();
            if parts.len() < 5 {
                continue;
            }
            sessions.push(SessionRef {
                tool: "git".into(),
                session_id: parts[0].to_owned(),
                path: self.repo_path.clone(),
                modified_at: parse_date(parts[3]),
                metadata: BTreeMap::from([
                    ("author".to_owned(), parts[1].to_owned()),
                    ("subject".to_owned(), parts[4].to_owned()),
                ]),
            });
        }
        Ok(sessions)
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let hash = &session.session_id;
        let format = r#"{"hash":"%H","author":"%an","email":"%ae","date":"%aI","message":"%s","parents":"%P"}"#;
        let args: Vec<String> = vec![
            "-C".into(),
            self.repo_path.clone(),
            "log".into(),
            "-1".into(),
            format!("--format={format}"),
            hash.clone(),
        ];
        let out = Self::run(&args).map_err(|e| format!("git log failed for {hash}: {e}"))?;
        let commit: Commit =
            serde_json::from_str(&out).map_err(|e| format!("failed to parse commit JSON: {e}"))?;
        let (mut insertions, mut deletions, mut files) = (0_i64, 0_i64, Vec::<String>::new());
        let diff_args: Vec<String> = [
            "-C",
            &self.repo_path,
            "diff",
            "--stat",
            "--numstat",
            &format!("{hash}^..{hash}"),
        ]
        .map(String::from)
        .into();
        if let Ok(diff) = Self::run(&diff_args) {
            for line in diff.lines() {
                let nums: Vec<&str> = line.split_whitespace().collect();
                if nums.len() >= 3 {
                    // Best effort like Sscanf: unparsable numbers keep the previous value.
                    insertions = nums[0].parse().unwrap_or(insertions);
                    deletions = nums[1].parse().unwrap_or(deletions);
                }
                if let Some(i) = line.rfind('\t') {
                    files.push(line[i + 1..].to_owned());
                }
            }
        }
        let short = hash.get(..7).unwrap_or(hash);
        let mut metadata = BTreeMap::from([
            ("commit".to_owned(), hash.clone()),
            ("author".to_owned(), commit.author.clone()),
            ("email".to_owned(), commit.email.clone()),
            ("date".to_owned(), commit.date.clone()),
            ("insertions".to_owned(), insertions.to_string()),
            ("deletions".to_owned(), deletions.to_string()),
        ]);
        if !files.is_empty() {
            metadata.insert(
                "files_changed".into(),
                serde_json::to_string(&files).map_err(|e| e.to_string())?,
            );
        }
        if commit.parents.contains(' ') {
            metadata.insert("merge".into(), "true".into());
        }
        Ok(vec![ImportedFact {
            content: format!("Commit {short} by {}: {}", commit.author, commit.message),
            source: "git".into(),
            session_id: hash.clone(),
            timestamp: parse_date(&commit.date),
            metadata,
            evidence: Vec::new(),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::test_support::TestDir;
    use chrono::TimeZone;

    /// Runs git in the synthetic repo with a pinned identity and no user config.
    fn git(dir: &std::path::Path, args: &[&str], date: &str) {
        let status = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture Author")
            .env("GIT_AUTHOR_EMAIL", "author@fixture.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture Author")
            .env("GIT_COMMITTER_EMAIL", "author@fixture.invalid")
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date)
            .output()
            .expect("git runs");
        assert!(
            status.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&status.stderr)
        );
    }

    fn fixture() -> TestDir {
        let dir = TestDir::new("git");
        git(
            dir.path(),
            &["init", "-q", "-b", "main"],
            "2026-01-01T00:00:00Z",
        );
        dir.write("a.txt", "one\ntwo\n");
        git(dir.path(), &["add", "."], "2026-01-01T00:00:00Z");
        git(
            dir.path(),
            &["commit", "-q", "-m", "first commit"],
            "2026-01-02T10:00:00Z",
        );
        dir.write("a.txt", "one\ntwo\nthree\n");
        dir.write("b.txt", "new\n");
        git(dir.path(), &["add", "."], "2026-01-03T00:00:00Z");
        git(
            dir.path(),
            &["commit", "-q", "-m", "second commit"],
            "2026-03-04T11:30:00Z",
        );
        dir
    }

    #[test]
    fn discovers_and_imports_commits() {
        let dir = fixture();
        let importer = Git::new(&dir.path().to_string_lossy(), "");
        let all = importer
            .discover_sessions(crate::sync_state::zero_time())
            .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].metadata["subject"], "second commit", "newest first");
        assert_eq!(all[0].metadata["author"], "Fixture Author");
        assert_eq!(
            all[0].modified_at,
            Utc.with_ymd_and_hms(2026, 3, 4, 11, 30, 0).unwrap()
        );
        let recent = importer
            .discover_sessions(Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap())
            .unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(
            Git::new(&dir.path().to_string_lossy(), "nobody")
                .discover_sessions(crate::sync_state::zero_time())
                .unwrap()
                .len(),
            0
        );

        let fact = &importer.import_session(&all[0]).unwrap()[0];
        let short = &all[0].session_id[..7];
        assert_eq!(
            fact.content,
            format!("Commit {short} by Fixture Author: second commit")
        );
        assert_eq!(fact.metadata["email"], "author@fixture.invalid");
        assert_eq!(fact.metadata["files_changed"], "[\"a.txt\",\"b.txt\"]");
        assert_eq!(fact.metadata["commit"], all[0].session_id);
        assert!(!fact.metadata.contains_key("merge"));
        assert_eq!(fact.timestamp, all[0].modified_at);
        assert_eq!(
            (importer.category(), importer.privacy_level()),
            (Some("code"), Some(PrivacyLevel::Public))
        );
    }

    #[test]
    fn git_failures_are_reported() {
        let dir = TestDir::new("git-none");
        let importer = Git::new(&dir.path().to_string_lossy(), "");
        let error = importer
            .discover_sessions(crate::sync_state::zero_time())
            .unwrap_err();
        assert!(error.starts_with("git log failed: "), "{error}");
        let session = SessionRef {
            session_id: "deadbeefcafe".into(),
            ..SessionRef::default()
        };
        assert!(
            importer
                .import_session(&session)
                .unwrap_err()
                .starts_with("git log failed for deadbeefcafe: ")
        );
    }
}

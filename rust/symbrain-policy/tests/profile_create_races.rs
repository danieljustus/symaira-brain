//! Actual process races for no-clobber publication and temporary-file ownership.
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use symbrain_policy::profile::create::{create_in, render_template};

struct Creator(Child);
impl Drop for Creator {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(description: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {description}"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn creator_subprocess() {
    let Some(root) = std::env::var_os("SYMBRAIN_PROFILE_RACE_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let id = std::env::var("SYMBRAIN_PROFILE_RACE_ID").expect("owned creator id");
    let from = std::env::var("SYMBRAIN_PROFILE_RACE_FROM").expect("owned template");
    fs::write(root.join(format!("ready-{id}")), b"ready").expect("ready signal");
    wait_for("start signal", || root.join("start").exists());
    let result = match create_in(&root.join("profiles"), "race", &from) {
        Ok(_) => format!("winner:{from}"),
        Err(error) => format!("error:{:?}", error.kind()),
    };
    fs::write(root.join(format!("result-{id}")), result).expect("creator result");
}

#[test]
fn eight_processes_publish_one_complete_profile_without_temporary_files() {
    let root = tempfile::tempdir().expect("owned process root");
    let mut children = Vec::new();
    for id in 0..8 {
        let from = if id % 2 == 0 {
            "personal"
        } else {
            "restricted"
        };
        let child = Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "creator_subprocess", "--nocapture"])
            .env("SYMBRAIN_PROFILE_RACE_ROOT", root.path())
            .env("SYMBRAIN_PROFILE_RACE_ID", id.to_string())
            .env("SYMBRAIN_PROFILE_RACE_FROM", from)
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("creator process");
        children.push(Creator(child));
    }
    wait_for("eight ready creators", || {
        (0..8).all(|id| root.path().join(format!("ready-{id}")).exists())
    });
    fs::write(root.path().join("start"), b"start").expect("release creators");
    for child in &mut children {
        wait_for("creator completion", || {
            child.0.try_wait().expect("creator status").is_some()
        });
        assert!(child.0.wait().expect("creator exit").success());
    }
    let results: Vec<_> = (0..8)
        .map(|id| fs::read_to_string(root.path().join(format!("result-{id}"))).unwrap())
        .collect();
    let winners: Vec<_> = results
        .iter()
        .filter_map(|r| r.strip_prefix("winner:"))
        .collect();
    assert_eq!(winners.len(), 1, "{results:?}");
    assert_eq!(
        results
            .iter()
            .filter(|r| *r == "error:AlreadyExists")
            .count(),
        7,
        "{results:?}"
    );
    let profiles = root.path().join("profiles");
    assert_eq!(
        fs::read(profiles.join("race.toml")).unwrap(),
        render_template(winners[0], "race").unwrap().as_bytes()
    );
    assert_eq!(
        fs::read_dir(profiles).unwrap().count(),
        1,
        "temporary files must be removed"
    );
    println!(
        "PROFILE_CREATE_PROCESS_RACE {}",
        serde_json::json!({
            "creators": 8, "results": results, "winner_template": winners[0],
            "winner_bytes": render_template(winners[0], "race").unwrap(),
            "remaining_profile_files": 1
        })
    );
}

#[test]
fn legacy_temporary_slots_are_never_reused_or_removed() {
    let root = tempfile::tempdir().unwrap();
    for id in 0..100 {
        fs::write(
            root.path().join(format!(".race.toml.{id}.tmp")),
            b"owned unrelated file",
        )
        .unwrap();
    }
    create_in(root.path(), "race", "restricted").unwrap();
    assert_eq!(
        fs::read(root.path().join("race.toml")).unwrap(),
        render_template("restricted", "race").unwrap().as_bytes()
    );
    for id in 0..100 {
        assert_eq!(
            fs::read(root.path().join(format!(".race.toml.{id}.tmp"))).unwrap(),
            b"owned unrelated file"
        );
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 101);
}

#[test]
fn existing_profile_and_validation_failures_leave_no_temporaries() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("race.toml");
    fs::write(&path, b"existing winner").unwrap();
    assert_eq!(
        create_in(root.path(), "race", "personal")
            .unwrap_err()
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read(path).unwrap(), b"existing winner");
    assert_eq!(
        create_in(root.path(), "other", "unknown")
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        create_in(root.path(), "../other", "personal")
            .unwrap_err()
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

//! Lifecycle metadata uses link-entry time without following its target.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use rustix::fs::{AtFlags, CWD, Timespec, Timestamps, utimensat};
use symbrain_skills::metadata::{Options, collect};

fn stamp(path: &Path, seconds: i64) {
    let time = Timespec {
        tv_sec: seconds,
        tv_nsec: 0,
    };
    utimensat(
        CWD,
        path,
        &Timestamps {
            last_access: time,
            last_modification: time,
        },
        AtFlags::SYMLINK_NOFOLLOW,
    )
    .unwrap();
}

#[test]
fn modified_at_observes_links_not_their_targets() {
    let old = 1_577_934_245;
    let newer = old + 60;
    for kind in [
        "file",
        "directory",
        "dangling",
        "outside-file",
        "outside-directory",
    ] {
        for link_newer in [false, true] {
            let owned = tempfile::tempdir().unwrap();
            let root = owned.path().join("skill");
            fs::create_dir(&root).unwrap();
            let document = root.join("SKILL.md");
            fs::write(&document, b"old content").unwrap();
            stamp(&document, old - 60);
            let outside = kind.starts_with("outside-") || kind == "dangling";
            let target = if outside {
                owned.path().join("outside-target")
            } else {
                root.join("target")
            };
            let (link_time, target_time) = if link_newer {
                (newer, old)
            } else {
                (old, newer)
            };
            if kind != "dangling" {
                if kind.ends_with("directory") {
                    fs::create_dir(&target).unwrap();
                    let child = target.join("child");
                    fs::write(&child, b"must not recurse through link").unwrap();
                    stamp(&child, if outside { newer + 60 } else { old - 60 });
                } else {
                    fs::write(&target, b"target content is not read").unwrap();
                }
                stamp(&target, target_time);
            }
            let link = root.join("resource-link");
            symlink(&target, &link).unwrap();
            stamp(&link, link_time);
            let observed = fs::symlink_metadata(&link).unwrap().modified().unwrap();
            assert_eq!(
                chrono::DateTime::<chrono::Utc>::from(observed).timestamp(),
                link_time
            );
            // A confined ordinary file is also independently visited by WalkDir.
            // Real directory mtimes and outside target/descendant mtimes are not.
            let expected_time = if kind == "file" {
                link_time.max(target_time)
            } else {
                link_time
            };
            let expected = chrono::DateTime::from_timestamp(expected_time, 0)
                .unwrap()
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string();
            assert_eq!(
                collect(&root, "demo", &Options::default()).modified_at,
                expected,
                "{kind}, link_newer={link_newer}"
            );
        }
    }
}

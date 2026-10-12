//! Public install inputs, results, and rendered-tree entry points.
#![allow(
    clippy::assigning_clones,
    clippy::cloned_ref_to_slice_refs,
    clippy::doc_markdown,
    clippy::missing_errors_doc
)]

use std::path::{Path, PathBuf};

use super::cache::cache_path;
use super::destination::{effective_home, effective_mode};
use super::event::record_install_outcome;
use super::ops::{
    install_locks_for, install_target, install_target_inner_locked, preflight_destination,
};
use super::replace::{FaultPoint, replace_tree};
use super::transaction::{backup_existing, remove_entry, remove_tree, restore_backup};
use crate::materialize::{Materialized, lock_render_tree, materialize};
use crate::model::{Bundle, SkillError, validate_skill_name};
use crate::render::Rendered;

/// Options for installing a rendered skill at any registered target and scope.
// These independent flags retain the public installer and Go adapter inputs.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    /// User home used to resolve global harness skill roots.
    pub home_dir: PathBuf,
    /// Project root used for project-scope installs.
    pub project_dir: Option<PathBuf>,
    /// Retain the historical project base without identity when an adapter's
    /// Go Options omitted ProjectDir and only the destination defaults to cwd.
    /// Other callers keep project identities; this never alters their defaults.
    pub legacy_project_base: bool,
    /// Optional replacement for the default base snapshot root.
    pub base_dir: Option<PathBuf>,
    /// Retained render root for applying installs in either mode.
    ///
    /// Go's runner renders through `render.RenderAll(bundle, opts.RenderDir,
    /// [target])` and then installs a symlink at `item.Path`, i.e.
    /// `<RenderDir>/<target>/<name>` — flat, with no scope or identity segment.
    /// When set, the stable tree is written there; a symlink points at it and
    /// a copy records that same retained source path in its marker.
    /// When unset, symlinks use the per-user cache and copies use temporary
    /// staging, preserving direct callers' existing behavior.
    pub render_dir: Option<PathBuf>,
    /// `copy` (default) or managed `symlink`.
    pub mode: String,
    /// Adopt an unmanaged destination after moving it to a collision-free backup.
    pub force: bool,
    /// Report planned changes without writing destination or base bytes.
    pub dry_run: bool,
    /// Preserve executable bits on resource files.
    pub allow_executable: bool,
    /// Deterministic replacement fault for rollback tests.
    pub fault: Option<FaultPoint>,
    /// Optional JSONL operation-log path. Logging is best effort.
    pub events_path: Option<PathBuf>,
}

/// Result of an install attempt.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InstallResult {
    /// `installed` or `planned`.
    pub action: String,
    /// Harness target.
    pub target: String,
    /// Installed skill name.
    pub name: String,
    /// Absolute destination path.
    #[serde(serialize_with = "crate::text::serialize_path")]
    pub path: PathBuf,
    /// `copy` or `symlink`.
    pub mode: String,
    /// Files whose executable bits would be stripped.
    pub mode_changes: Vec<ModeChange>,
    /// Backup created when force adopts an unmanaged directory.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(serialize_with = "crate::text::serialize_optional_path")]
    pub backup_path: Option<PathBuf>,
}

/// One planned executable-bit change.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ModeChange {
    /// Slash-separated relative path.
    pub path: String,
    /// Original octal mode.
    pub from: String,
    /// Resulting octal mode.
    pub to: String,
}

/// Resolves the installation path for a registered target and scope.
pub fn install_path(home: &Path, name: &str) -> Result<PathBuf, SkillError> {
    install_path_for("opencode", home, None, "user", name)
}

/// Resolves a target's user- or project-scope installation path.
pub fn install_path_for(
    target: &str,
    home: &Path,
    project: Option<&Path>,
    scope: &str,
    name: &str,
) -> Result<PathBuf, SkillError> {
    validate_skill_name(name)?;
    let home = if home.as_os_str().is_empty() {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| SkillError("cannot determine user home directory".to_owned()))?
    } else {
        home.to_path_buf()
    };
    crate::target::skill_root(target, &home, project, scope)
        .map(|root| root.join(name))
        .ok_or_else(|| {
            SkillError(format!(
                "unknown target or missing project directory: {target:?}"
            ))
        })
}

/// Installs an OpenCode rendered tree in copy mode (the Phase 7.3A API).
pub fn install_copy(
    source: &Path,
    name: &str,
    source_hash: &str,
    options: &InstallOptions,
) -> Result<InstallResult, SkillError> {
    let mut copy_options = options.clone();
    if copy_options.mode.is_empty() {
        copy_options.mode = "copy".to_owned();
    }
    let result = install_target("opencode", source, name, source_hash, &copy_options);
    record_install_outcome("opencode", name, &copy_options, &result);
    result
}

/// Installs a rendered tree using its target identity.
pub fn install_rendered(
    bundle: &Bundle,
    rendered: &Rendered,
    options: &InstallOptions,
) -> Result<InstallResult, SkillError> {
    let result = install_rendered_inner(bundle, rendered, options);
    record_install_outcome(&rendered.target, &rendered.name, options, &result);
    result
}

fn install_rendered_inner(
    bundle: &Bundle,
    rendered: &Rendered,
    options: &InstallOptions,
) -> Result<InstallResult, SkillError> {
    let output = tempfile::tempdir()
        .map_err(|error| SkillError(format!("create install staging root: {error}")))?;
    let materialized = materialize(bundle, rendered, output.path())?;
    install_materialized(&materialized, rendered, options, || Ok(()))
}

// The private callback is only a deterministic post-publication fault/race
// seam; production passes a no-op and exposes no new installer options.
fn install_materialized(
    materialized: &Materialized,
    rendered: &Rendered,
    options: &InstallOptions,
    after_publication: impl FnOnce() -> Result<(), SkillError>,
) -> Result<InstallResult, SkillError> {
    // Applying callers with an explicit render root retain the source in both
    // modes. Direct copy callers without a root keep private staging only.
    if (effective_mode(options) == "symlink" || options.render_dir.is_some()) && !options.dry_run {
        // Validate the destination before changing the cache. A rejected
        // unmanaged collision must not refresh or replace the cache tree.
        preflight_destination(&rendered.target, &rendered.name, options)?;
        let home = effective_home(&options.home_dir)?;
        // Go links at `<RenderDir>/<target>/<name>`; only a caller that has no
        // render root (a direct install without the runner) falls back to the
        // per-user cache.
        let stable = if let Some(render_dir) = options.render_dir.as_deref() {
            // Go links at `<RenderDir>/<target>/<name>`: flat, no scope or
            // identity segment.
            render_dir.join(&rendered.target).join(&rendered.name)
        } else {
            // A direct install with no render root keeps the per-user cache.
            let cache = home.join(".local/share/symskills/rendered");
            cache_path(&cache, rendered, options)?
        };
        let _locks =
            install_locks_for(&rendered.target, &rendered.name, options, &[stable.clone()])?;
        // Install locks precede the renderer lock everywhere. A standalone
        // renderer takes only this lock, so there is no reverse acquisition.
        let _render_lock = options
            .render_dir
            .as_deref()
            .map(|root| lock_render_tree(root, &rendered.target, &rendered.name))
            .transpose()?;
        // Keep the previous cache tree until destination and base publication
        // both succeed. The install lock also serializes other processes.
        let cache_backup = backup_existing(&stable, None)?;
        let operation = (|| {
            // Publish the exact staging bytes which produced source_hash;
            // never reread mutable bundle resources into a second render.
            replace_tree(
                &materialized.root,
                &stable,
                // An explicit render retains original modes until the
                // installer records mode_changes and strips executable bits.
                if options.render_dir.is_some() || options.allow_executable {
                    0o777
                } else {
                    0
                },
                options.fault,
            )?;
            after_publication()?;
            install_target_inner_locked(
                &rendered.target,
                &stable,
                &rendered.name,
                &materialized.source_hash,
                options,
            )
        })();
        return match operation {
            Ok(result) => {
                if let Some(backup) = cache_backup {
                    remove_tree(&backup)?;
                }
                Ok(result)
            }
            Err(error) => {
                let _ = remove_entry(&stable);
                if let Some(backup) = cache_backup {
                    let _ = restore_backup(&backup, &stable);
                }
                Err(error)
            }
        };
    }
    install_target(
        &rendered.target,
        &materialized.root,
        &rendered.name,
        &materialized.source_hash,
        options,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn retained_install_binds_staging_and_serializes_renderer_through_late_rollback() {
        use std::sync::mpsc;
        use std::time::Duration;
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: fixture\n---\nbody\n",
        )
        .unwrap();
        std::fs::write(source.join("notes.md"), b"before\n").unwrap();
        let bundle = crate::load_bundle(&source).unwrap();
        let rendered =
            crate::render_target(&bundle, "opencode", &crate::RenderMetadata::default()).unwrap();
        let stage = materialize(&bundle, &rendered, &temp.path().join("stage")).unwrap();
        let retained = temp.path().join("retained");
        let options = InstallOptions {
            home_dir: temp.path().join("home"),
            render_dir: Some(retained.clone()),
            mode: "copy".into(),
            ..Default::default()
        };
        // Same-size mutation after staging must not leak into retained output
        // under the earlier staging hash.
        std::fs::write(source.join("notes.md"), b"after!\n").unwrap();
        let installed = install_materialized(&stage, &rendered, &options, || Ok(())).unwrap();
        let tree = retained.join("opencode/demo");
        assert_eq!(std::fs::read(tree.join("notes.md")).unwrap(), b"before\n");
        assert_eq!(
            std::fs::read(installed.path.join("notes.md")).unwrap(),
            b"before\n"
        );
        let marker: serde_json::Value =
            serde_json::from_slice(&std::fs::read(installed.path.join(".symskills.json")).unwrap())
                .unwrap();
        assert_eq!(marker["source_hash"], stage.source_hash);
        std::thread::scope(|scope| {
            let (started, start_rx) = mpsc::channel();
            let (done, done_rx) = mpsc::channel();
            let mut writer = None;
            let result = install_materialized(&stage, &rendered, &options, || {
                let (bundle, rendered, retained) = (&bundle, &rendered, &retained);
                writer = Some(scope.spawn(move || {
                    started.send(()).unwrap();
                    materialize(bundle, rendered, retained).unwrap();
                    done.send(()).unwrap();
                }));
                start_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                let other = std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(retained.join("opencode/.symskills-lock-demo"))
                    .unwrap();
                assert_eq!(
                    fs2::FileExt::try_lock_exclusive(&other).unwrap_err().kind(),
                    std::io::ErrorKind::WouldBlock
                );
                assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
                Err(SkillError("late installer failure".into()))
            });
            assert_eq!(result.unwrap_err().0, "late installer failure");
            writer.unwrap().join().unwrap();
            done_rx.recv().unwrap();
        });
        // The competing renderer publishes only after rollback and its new
        // successful output must survive the failed installation's cleanup.
        assert_eq!(std::fs::read(tree.join("notes.md")).unwrap(), b"after!\n");
        assert_eq!(
            std::fs::read(installed.path.join("notes.md")).unwrap(),
            b"before\n"
        );
    }

    #[test]
    fn explicit_copy_render_root_is_retained_but_dry_run_does_not_publish() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: fixture\n---\nbody\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(source.join("script.sh"), b"fixture, never executed\n").unwrap();
            std::fs::set_permissions(
                source.join("script.sh"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        let bundle = crate::load_bundle(&source).unwrap();
        let rendered =
            crate::render_target(&bundle, "opencode", &crate::RenderMetadata::default()).unwrap();
        let retained = temp.path().join("retained");
        let mut options = InstallOptions {
            home_dir: temp.path().join("home"),
            render_dir: Some(retained.clone()),
            mode: "copy".into(),
            dry_run: true,
            ..Default::default()
        };
        let planned = install_rendered(&bundle, &rendered, &options).unwrap();
        assert_eq!(planned.action, "planned");
        assert!(!retained.exists());
        assert!(!planned.path.exists());
        options.dry_run = false;
        let installed = install_rendered(&bundle, &rendered, &options).unwrap();
        assert_eq!(installed.mode_changes, planned.mode_changes);
        let tree = retained.join("opencode/demo");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(installed.mode_changes.len(), 1);
            assert_eq!(installed.mode_changes[0].from, "0755");
            assert_eq!(installed.mode_changes[0].to, "0644");
            for path in [tree.join("script.sh"), installed.path.join("script.sh")] {
                assert_eq!(
                    std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o644
                );
            }
        }
        assert_eq!(
            std::fs::read(tree.join("SKILL.md")).unwrap(),
            rendered.skill_md
        );
        let marker: serde_json::Value =
            serde_json::from_slice(&std::fs::read(installed.path.join(".symskills.json")).unwrap())
                .unwrap();
        assert_eq!(marker["rendered_at"], tree.to_string_lossy().as_ref());
        assert_eq!(
            std::fs::read(tree.join(".symskills.json")).unwrap(),
            std::fs::read(installed.path.join(".symskills.json")).unwrap()
        );
    }
}

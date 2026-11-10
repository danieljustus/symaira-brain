//! Doctor check for a first-run state backup that has not finished (#1020).

use super::doctor_types::PendingBackup;

/// Unfinished `.<app>-resume.partial` backups in the cutover store. Read-only.
pub(super) fn check_pending_backups() -> Vec<PendingBackup> {
    let Some(homes) = symaira_cutover::Homes::from_env() else {
        return Vec::new();
    };
    let disabled = symaira_cutover::disabled();
    symaira_cutover::pending(&homes.store())
        .into_iter()
        .map(|pending| PendingBackup {
            finish: finish_hint(&pending.app, pending.running, disabled),
            partial_path: pending.partial.display().to_string(),
            running: pending.running,
            app: pending.app,
        })
        .collect()
}

fn finish_hint(app: &str, running: bool, disabled: bool) -> String {
    if running {
        return "a backup is copying right now; let it finish, then run `symbrain doctor` again"
            .to_owned();
    }
    let env = symaira_cutover::DISABLE_ENV;
    if disabled {
        format!(
            "backups are switched off in this process; unset {env} (debug builds need {env}=on), then run `{app} version`"
        )
    } else {
        format!(
            "run any `{app}` command (for example `{app} version`); it resumes the copy, verifies it and writes the marker. Short runs may pause it again, repeat until this entry disappears"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::finish_hint;

    #[test]
    fn hint_names_how_to_finish() {
        assert!(finish_hint("symbrain", false, false).contains("symbrain version"));
        assert!(finish_hint("symbrain", false, true).contains("SYMAIRA_CUTOVER_BACKUP"));
        assert!(finish_hint("symbrain", true, false).contains("copying right now"));
    }
}

//! Human-table lines for the optional snapshot metadata. Absent metadata
//! prints nothing, so the established table bytes stay unchanged.
use std::io::Write;

use symbrain_usage::UsageSnapshot;

pub(super) fn write_details(
    writer: &mut dyn Write,
    snapshot: &UsageSnapshot,
) -> std::io::Result<()> {
    if let Some(plan) = &snapshot.plan {
        writeln!(writer, "  plan\t{plan}")?;
    }
    if let Some(status) = &snapshot.reset_grants {
        let left: u64 = status.grants.iter().map(|grant| grant.resets_left).sum();
        write!(
            writer,
            "  reset grants\teligible={} resets_left={left}",
            status.eligible
        )?;
        if let Some(next) = &status.next_grant_id {
            write!(writer, " next={next}")?;
        }
        writeln!(writer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use symbrain_usage::{ResetGrant, ResetGrantStatus};

    fn text(snapshot: &UsageSnapshot) -> String {
        let mut out = Vec::new();
        write_details(&mut out, snapshot).expect("render");
        String::from_utf8(out).expect("utf8")
    }

    #[test]
    fn absent_metadata_prints_nothing() {
        assert_eq!(text(&UsageSnapshot::default()), "");
    }

    #[test]
    fn plan_and_reset_grants_render_without_ids_of_accounts() {
        let snapshot = UsageSnapshot {
            plan: Some("Max 5x".into()),
            reset_grants: Some(ResetGrantStatus {
                eligible: true,
                grants: vec![
                    ResetGrant {
                        id: "g1".into(),
                        resets_left: 2,
                        ..ResetGrant::default()
                    },
                    ResetGrant {
                        id: "g2".into(),
                        resets_left: 1,
                        ..ResetGrant::default()
                    },
                ],
                next_grant_id: Some("g1".into()),
            }),
            ..UsageSnapshot::default()
        };
        assert_eq!(
            text(&snapshot),
            "  plan\tMax 5x\n  reset grants\teligible=true resets_left=3 next=g1\n"
        );
    }
}

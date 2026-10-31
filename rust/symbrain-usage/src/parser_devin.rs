use super::{numeric, reset};
use crate::{UsageMeter, UsageSnapshot};
use serde_json::Value;

pub(super) fn parse(root: &Value, s: &mut UsageSnapshot) -> Result<(), ()> {
    let ps = root
        .pointer("/userStatus/planStatus")
        .filter(|v| v.is_object())
        .ok_or(())?;
    if ps.get("planInfo").is_some_and(|info| {
        !info.is_null()
            && (!info.is_object()
                || info
                    .get("hideDailyQuota")
                    .is_some_and(|v| !v.is_null() && !v.is_boolean()))
    }) {
        return Err(());
    }
    let hide = ps
        .pointer("/planInfo/hideDailyQuota")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let daily = numeric(ps.get("dailyQuotaRemainingPercent"));
    let mut weekly = numeric(ps.get("weeklyQuotaRemainingPercent"));
    if ps.get("weeklyQuotaRemainingPercent").is_some() && weekly.is_none() {
        return Err(());
    }
    let daily_reset = reset(ps.get("dailyQuotaResetAtUnix"), false)?;
    let weekly_reset = reset(ps.get("weeklyQuotaResetAtUnix"), false)?;
    if let Some(daily) = daily.filter(|_| !hide) {
        s.meters.push(quota("Daily quota", daily, daily_reset));
    }
    if weekly.is_none() && weekly_reset.is_some() {
        weekly = Some(0.0);
    }
    if let Some(weekly) = weekly {
        s.meters.push(quota("Weekly quota", weekly, weekly_reset));
    } else if let Some(daily) = daily.filter(|_| hide) {
        s.meters.push(quota("Weekly quota", daily, None));
    }
    if let Some(micros) = numeric(ps.get("overageBalanceMicros")) {
        let amount = (micros.max(0.0) / 1e6).to_string();
        s.balance = Some(amount.clone());
        s.currency = Some("USD".into());
        s.meters.push(UsageMeter {
            label: "Extra usage balance".into(),
            used: Some(amount),
            unit: "USD".into(),
            ..UsageMeter::default()
        });
    }
    if s.meters.is_empty() {
        return Err(());
    }
    Ok(())
}
fn quota(
    label: &str,
    remaining: f64,
    resets_at: Option<chrono::DateTime<chrono::FixedOffset>>,
) -> UsageMeter {
    UsageMeter {
        label: label.into(),
        used: Some((100.0 - remaining).clamp(0.0, 100.0).to_string()),
        limit: Some("100".into()),
        unit: "%".into(),
        resets_at,
    }
}

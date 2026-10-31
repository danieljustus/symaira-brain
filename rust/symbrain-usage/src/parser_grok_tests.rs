use super::*;
use serde_json::json;

fn weekly() -> Value {
    json!({"config": {"creditUsagePercent": 23.5, "currentPeriod": {
        "type": "USAGE_PERIOD_TYPE_WEEKLY", "start": "2026-07-03T04:01:09.238389+00:00", "end": "2026-07-10T04:01:09.238389+00:00"
    }, "onDemandCap": {"val":2500}}})
}

#[test]
fn grok_weekly_missing_zero_clamp_and_payg() {
    let now = Utc::now();
    for (percent, expected) in [
        (json!(23.5), "23.5"),
        (json!(0), "0"),
        (json!(125), "100"),
        (json!(-3), "0"),
        (json!("25.5"), "25.5"),
    ] {
        let mut value = weekly();
        value["config"]["creditUsagePercent"] = percent;
        let snapshot = parse(&serde_json::to_vec(&value).unwrap(), now).unwrap();
        assert_eq!(snapshot.meters.len(), 1);
        let meter = &snapshot.meters[0];
        assert_eq!(meter.used.as_deref(), Some(expected));
        assert_eq!(meter.label, "Weekly limit");
        assert_eq!(meter.limit.as_deref(), Some("100"));
        assert_eq!(meter.unit, "%");
        assert_eq!(
            meter.resets_at.unwrap().to_rfc3339(),
            "2026-07-10T04:01:09.238389+00:00"
        );
        assert_eq!(snapshot.fetched_at, now);
        assert!(snapshot.balance.is_none());
        assert!(snapshot.currency.is_none());
    }
    let mut value = weekly();
    value["config"]
        .as_object_mut()
        .unwrap()
        .remove("creditUsagePercent");
    let snapshot = parse(&serde_json::to_vec(&value).unwrap(), now).unwrap();
    assert!(snapshot.meters[0].used.is_none());
    assert!(
        serde_json::to_value(&snapshot).unwrap()["meters"][0]
            .get("used")
            .is_none()
    );
}

#[test]
fn grok_rejects_monthly_and_malformed_fields() {
    for percent in [
        json!(null),
        json!(true),
        json!({}),
        json!("NaN"),
        json!("Infinity"),
        json!("null"),
    ] {
        let mut value = weekly();
        value["config"]["creditUsagePercent"] = percent;
        assert!(parse(&serde_json::to_vec(&value).unwrap(), Utc::now()).is_err());
    }
    for (field, replacement) in [
        ("type", "USAGE_PERIOD_TYPE_MONTHLY"),
        ("start", "invalid"),
        ("end", "2026-07-01T00:00:00Z"),
    ] {
        let mut value = weekly();
        value["config"]["currentPeriod"][field] = json!(replacement);
        assert!(parse(&serde_json::to_vec(&value).unwrap(), Utc::now()).is_err());
    }
    for body in [
        b"{}".as_slice(),
        b"null",
        b"[]",
        b"not json",
        b"{\"config\":{}}",
    ] {
        assert!(parse(body, Utc::now()).is_err());
    }
}

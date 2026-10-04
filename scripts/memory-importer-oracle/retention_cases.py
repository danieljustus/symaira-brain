"""PREPARED real-owner nanosecond deletion cases; no expected algorithm copy."""
import json


def cases():
    rows = []
    for fraction in (400, 500, 600, 999):
        rows.append({"ID": f"R761-expire-{fraction}", "Operation": "expire",
            "Started": "2026-01-01T00:00:00Z", "Ended": "2026-01-01T00:00:00.0000005Z",
            "Expiry": "2026-01-02T00:00:00.0000005Z", "At": f"2026-01-02T00:00:00.{fraction:09}Z"})
        rows.append({"ID": f"R761-range-{fraction}", "Operation": "range",
            "Started": "2026-01-01T00:00:00Z", "Ended": "2026-01-01T00:00:00.0000005Z",
            "Expiry": "2026-01-02T00:00:00Z", "Start": f"2026-01-01T00:00:00.{fraction:09}Z",
            "End": "2026-01-02T00:00:00Z"})
    for fraction in (0, 1, 999999999):
        rows.append({"ID": f"R761-fraction-{fraction}", "Operation": "expire",
            "Started": "2026-01-01T00:00:00Z", "Ended": "2026-01-01T00:00:01Z",
            "Expiry": f"2026-01-02T00:00:00.{fraction:09}Z",
            "At": f"2026-01-02T00:00:00.{fraction:09}Z"})
    rows.append({"ID": "R761-utc-conversion", "Operation": "expire",
        "Started": "2026-01-01T00:00:00Z", "Ended": "2026-01-01T00:00:01Z",
        "Expiry": "2026-01-02T00:00:00.0000005Z", "At": "2026-01-02T01:00:00.0000006+01:00"})
    return rows


if __name__ == "__main__": print(json.dumps(cases(), indent=2))

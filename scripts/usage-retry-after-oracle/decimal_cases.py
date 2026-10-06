"""Additive SDK long-decimal regressions; original 1600/1164 remain intact."""
import argparse
import json
from pathlib import Path
import subprocess
import sys


def inputs():
    rows = [
        ("positive-capped", "0." + "0" * 10000 + "1e100000"),
        ("negative-mantissa-capped", "1" + "0" * 10000 + "e-100000"),
        ("five-digit-control", "0." + "0" * 10000 + "1e10001"),
        ("underscore-capped", "0." + "0_" * 10000 + "1e100_000"),
        ("negative-policy", "-0." + "0" * 10000 + "1e100000"),
        ("plus-capped", "+0." + "0" * 10000 + "1e100000"),
        ("negative-zero-fallback", "-1" + "0" * 10000 + "e-100000"),
        ("public-no-length-waiver", "0." + "0" * 100000 + "1e100000"),
    ]
    for zeros in (9999, 10000, 10001):
        for exponent in ("9999", "10000", "10001", "100000", "100001", "99999", "999999"):
            rows.append((f"point-{zeros}-exponent-{exponent}", "0." + "0" * zeros + "1e" + exponent))
    for length in (18, 19, 20, 798, 799, 800, 801, 802, 10000):
        for sticky in (False, True):
            digits = "1" + "0" * (length - 2) + ("1" if sticky else "0")
            rows.append((f"digits-{length}-sticky-{sticky}", digits + "e-" + str(length - 1)))
    # Exact halfway rationals written as decimal coefficient *10^exponent.
    for name, coefficient, exponent in (
        ("half-even-down", "100000000000000011102230246251565404236316680908203125", -53),
        ("half-even-up", "100000000000000033306690738754696212708950042724609375", -53),
        ("half-subnormal", str(5 ** 1075), -1075),
        ("half-overflow", str(((1 << 54) - 1) << 970), 0),
    ):
        for suffix in ("exact", "zeros", "sticky"):
            padding = 1000 if suffix != "exact" else 0
            digits = coefficient + "0" * padding
            if suffix == "sticky":
                digits = digits[:-1] + "1"
            rows.append((name + "-" + suffix, digits + "e" + str(exponent - padding)))
        if name != "half-overflow":
            point = len(coefficient) + exponent
            text = (coefficient[:point] + "." + coefficient[point:] if point > 0
                    else "0." + "0" * -point + coefficient)
            rows += [("dot-" + name + "-exact", text),
                     ("dot-" + name + "-zeros", text + "0" * 1000),
                     ("dot-" + name + "-sticky", text + "0" * 999 + "1")]
    rows += [("bad-tail", "0." + "0" * 10000 + "1e100000x"),
             ("bad-underscore", "0." + "0" * 10000 + "1e100000_"),
             ("bad-sign", "0." + "0" * 10000 + "1e+-100000")]
    rows += [("deep-subnormal", "3e-326"), ("deep-subnormal-negative", "-3e-326"),
             ("half-subnormal-below", "2.4703282292062327e-324"),
             ("half-subnormal-above", "2.4703282292062328e-324"),
             ("minimum-subnormal", "4.9406564584124654e-324"),
             ("largest-subnormal", "2.225073858507201e-308"),
             ("minimum-normal", "2.2250738585072014e-308"),
             ("maximum-finite", "1.7976931348623157e308"),
             ("overflow-neighbor", "1.7976931348623159e308"),
             ("zero-huge-exp", "-0e999999")]
    assert len(rows) == 81 and len({name for name, _ in rows}) == 81
    return [dict(id="decimal-" + name, value_hex=value.encode().hex()) for name, value in rows]


def expand(base):
    public = inputs()
    selected = ["positive-capped", "negative-mantissa-capped", "five-digit-control", "underscore-capped",
                "negative-policy", "negative-zero-fallback", "half-even-down-exact", "half-even-down-sticky",
                "half-subnormal-exact", "half-subnormal-sticky", "half-overflow-exact", "bad-underscore",
                "dot-half-even-down-sticky", "dot-half-subnormal-sticky",
                "deep-subnormal", "half-subnormal-above"]
    values = {row["id"]: bytes.fromhex(row["value_hex"]).decode() for row in public}
    strategies = {}
    for row in base["wire"]:
        strategies.setdefault((row["provider"], row["source"]), row)
    assert len(strategies) == 12
    wire = []
    for prototype in strategies.values():
        for name in selected:
            row = {key: value for key, value in prototype.items()
                   if key not in ("id", "retry_after", "retry_after_values_hex")}
            row.update(id=f"decimal-wire-{row['provider']}-{row['source']}-{name}",
                       retry_after=values["decimal-" + name])
            assert len(row["retry_after"].encode()) < 30000
            wire.append(row)
    assert len(base["public"]) == 1600 and len(base["wire"]) == 1164
    return dict(base, public=base["public"] + public, wire=base["wire"] + wire,
                original_public_cases=1600, original_wire_cases=1164,
                additive_public_cases=81, additive_wire_cases=192,
                public_cases=1681, wire_cases=1356)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("output", type=Path)
    parser.add_argument("--sdk-only", action="store_true")
    args = parser.parse_args()
    if args.sdk_only:
        result = dict(public=inputs())
    else:
        # Run the unchanged original generator; append without filtering or
        # replacing a single original case, input byte, ID or assertion.
        temporary = args.output.with_suffix(".original.json")
        subprocess.run([sys.executable, str(Path(__file__).with_name("cases.py")), str(temporary)], check=True)
        result = expand(json.loads(temporary.read_bytes()))
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()

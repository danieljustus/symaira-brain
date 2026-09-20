#!/usr/bin/env python3
"""Generate the Rust port of Go's math/rand rngSource + NormFloat64.

The shipped LSH projections come from `rand.New(rand.NewSource(42))` followed
by 16*768 `NormFloat64()` calls. Reproducing them byte-for-byte needs Go's
lagged-Fibonacci source, its 607 cooked seed values and the ziggurat tables —
all frozen by Go's compatibility promise. This script reads them straight out
of GOROOT so nothing is transcribed by hand.

Usage: python3 scripts/gen-gorand.py <goroot|auto> <output.rs>
"""

import re
import sys
from pathlib import Path


def read_go_source(goroot: Path, name: str) -> str:
    return (goroot / "src/math/rand" / name).read_text(encoding="utf-8")


def block(text: str, start: str, end: str) -> str:
    begin = text.index(start)
    stop = text.index(end, begin)
    return text[begin:stop]


def int64_table(text: str, name: str) -> list[int]:
    body = block(text, f"{name} [rngLen]int64 = [...]int64{{", "}")
    # Drop the declaration line: it carries the `int64` type widths, which the
    # number regex would otherwise read as table entries.
    entries = body.split("\n", 1)[1]
    return [int(value) for value in re.findall(r"-?\d+", entries)]


def typed_table(text: str, name: str, kind: str) -> list[str]:
    body = block(text, f"{name} = [128]{kind}{{", "}")
    if kind == "uint32":
        return re.findall(r"0x[0-9a-fA-F]+", body)
    # `wn` and `fn` mix exponent and plain decimal spellings; keep both, and
    # drop the declaration line (its `128`/`32` widths are not entries).
    entries = body.split("\n", 1)[1]
    values = re.findall(r"\d+(?:\.\d+)?(?:e[+-]?\d+)?", entries)
    # Every entry is an `f32` literal: Go writes the integral ones without a
    # decimal point, which Rust would read as an integer.
    return [value if ("." in value or "e" in value) else f"{value}.0" for value in values]


def grouped(value: int) -> str:
    """Spells an integer with `_` separators, as the lints require."""
    sign = "-" if value < 0 else ""
    digits = str(abs(value))
    parts = []
    while len(digits) > 3:
        parts.insert(0, digits[-3:])
        digits = digits[:-3]
    parts.insert(0, digits)
    return sign + "_".join(parts)


def chunk(values, per_line: int) -> str:
    """Spells a table body, `per_line` entries per row.

    Values keep the spelling of the Go source; the tables carry
    `#[rustfmt::skip]` so regenerating this file never fights the formatter.
    """
    entries = [str(value) for value in values]
    lines = []
    for index in range(0, len(entries), per_line):
        lines.append("    " + ", ".join(entries[index : index + per_line]) + ",")
    return "\n".join(lines)


TESTS = (Path(__file__).resolve().parent / "templates/gorand-tests.rs").read_text()


def main() -> int:
    goroot = Path(sys.argv[1]) if sys.argv[1] != "auto" else None
    if goroot is None:
        import subprocess

        goroot = Path(subprocess.run(["go", "env", "GOROOT"], capture_output=True, text=True, check=True).stdout.strip())
    output = Path(sys.argv[2])

    rng = read_go_source(goroot, "rng.go")
    normal = read_go_source(goroot, "normal.go")

    cooked = [str(value) for value in int64_table(rng, "rngCooked")]
    kn = typed_table(normal, "kn", "uint32")
    wn = typed_table(normal, "wn", "float32")
    fn = typed_table(normal, "fn", "float32")

    assert len(cooked) == 607, len(cooked)
    assert len(kn) == 128, len(kn)
    assert len(wn) == 128, len(wn)
    assert len(fn) == 128, len(fn)
    assert cooked.count(str(-4181792142133755926)) == 1

    body = f'''//! Go's `math/rand` source and normal distribution, ported exactly.
//!
//! The shipped LSH projections are generated with
//! `rand.New(rand.NewSource(42))` and 16*768 `NormFloat64()` calls, so the
//! bucketed candidate selection only matches when this port reproduces Go's
//! value stream bit for bit: the lagged-Fibonacci `rngSource` with its 607
//! cooked seed values, `Uint32`/`Float64`, and the ziggurat `NormFloat64`.
//! Go freezes those values under its compatibility promise, so the constants
//! below are stable.
//!
//! The tables are generated from GOROOT by `scripts/gen-gorand.py`; do not
//! edit them by hand. `gorand_matches_go` in `tests/` pins the port against
//! values produced by the Go toolchain.

// Every cast below mirrors a Go int32/int64/uint32/uint64/float64 conversion
// one for one, so truncation and sign changes are intentional and bounded by
// the algorithm; "cleaning them up" would break the value stream.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss
)]

const RNG_LEN: usize = 607;
const RNG_TAP: usize = 273;
const RNG_MASK: i64 = i64::MAX;
const INT32_MAX: i32 = i32::MAX;

/// The cooked seed values `rngSource.Seed` XORs into its initial state.
#[rustfmt::skip]
#[allow(clippy::unreadable_literal)]
const RNG_COOKED: [i64; RNG_LEN] = [
{chunk(cooked, 4)}
];

/// Ziggurat `kn` table (`uint32`).
#[rustfmt::skip]
#[allow(clippy::unreadable_literal)]
const KN: [u32; 128] = [
{chunk(kn, 5)}
];

/// Ziggurat `wn` table.
#[rustfmt::skip]
#[allow(clippy::unreadable_literal)]
const WN: [f32; 128] = [
{chunk(wn, 4)}
];

/// Ziggurat `fn` table.
#[rustfmt::skip]
#[allow(clippy::unreadable_literal)]
const FN: [f32; 128] = [
{chunk(fn, 5)}
];

/// The ziggurat tail bound `rn`.
const RN: f64 = 3.442_619_855_899;

/// A port of Go's `rngSource`: a lagged-Fibonacci generator over `i64`.
pub(crate) struct GoRand {{
    vec: [i64; RNG_LEN],
    tap: i32,
    feed: i32,
}}

impl GoRand {{
    /// Seeds the generator exactly like `rand.New(rand.NewSource(seed))`.
    pub(crate) fn new(seed: i64) -> Self {{
        let mut generator = Self {{
            vec: [0; RNG_LEN],
            tap: 0,
            feed: 0,
        }};
        generator.seed(seed);
        generator
    }}

    fn seed(&mut self, seed: i64) {{
        self.tap = 0;
        self.feed = (RNG_LEN - RNG_TAP) as i32;

        let mut seed = (seed % i64::from(INT32_MAX)) as i32;
        if seed < 0 {{
            seed += INT32_MAX;
        }}
        if seed == 0 {{
            seed = 89_482_311;
        }}

        let mut x = seed;
        for index in -20..RNG_LEN as i32 {{
            x = seedrand(x);
            if index >= 0 {{
                let mut u = i64::from(x) << 40;
                x = seedrand(x);
                u ^= i64::from(x) << 20;
                x = seedrand(x);
                u ^= i64::from(x);
                u ^= RNG_COOKED[index as usize];
                self.vec[index as usize] = u;
            }}
        }}
    }}

    /// Go's `rngSource.Uint64`.
    fn uint64(&mut self) -> u64 {{
        self.tap -= 1;
        if self.tap < 0 {{
            self.tap += RNG_LEN as i32;
        }}
        self.feed -= 1;
        if self.feed < 0 {{
            self.feed += RNG_LEN as i32;
        }}
        let x = self.vec[self.feed as usize].wrapping_add(self.vec[self.tap as usize]);
        self.vec[self.feed as usize] = x;
        x as u64
    }}

    /// Go's `Rand.Int63`.
    fn int63(&mut self) -> i64 {{
        (self.uint64() & RNG_MASK as u64) as i64
    }}

    /// Go's `Rand.Uint32`, i.e. `uint32(Int63() >> 31)`.
    fn uint32(&mut self) -> u32 {{
        (self.int63() >> 31) as u32
    }}

    /// Go's `Rand.Float64`: `float64(Int63()) / (1 << 63)`, retrying the rare
    /// rounding case that would produce exactly `1.0`.
    fn float64(&mut self) -> f64 {{
        loop {{
            let value = self.int63() as f64 / 9_223_372_036_854_775_808.0;
            if value < 1.0 {{
                return value;
            }}
        }}
    }}

    /// Go's `Rand.NormFloat64` (ziggurat, Marsaglia & Tsang 2000).
    pub(crate) fn norm_float64(&mut self) -> f64 {{
        loop {{
            let j = self.uint32() as i32;
            let index = (j & 0x7F) as usize;
            let x = f64::from(j) * f64::from(WN[index]);
            let magnitude = if j < 0 {{ j.unsigned_abs() }} else {{ j as u32 }};
            if magnitude < KN[index] {{
                return x;
            }}
            if index == 0 {{
                // The base-strip rejection loop: draw until the point falls
                // under the tail curve.
                let x = loop {{
                    let x = -self.float64().ln() * (1.0 / RN);
                    let y = -self.float64().ln();
                    if y + y >= x * x {{
                        break x;
                    }}
                }};
                if j > 0 {{
                    return RN + x;
                }}
                return -RN - x;
            }}
            let threshold = FN[index] + (self.float64() as f32) * (FN[index - 1] - FN[index]);
            if threshold < (-0.5 * x * x).exp() as f32 {{
                return x;
            }}
        }}
    }}
}}

/// Go's `seedrand`, the Park-Miller step used while seeding.
fn seedrand(x: i32) -> i32 {{
    const A: i32 = 48_271;
    const Q: i32 = 44_488;
    const R: i32 = 3_399;

    let hi = x / Q;
    let lo = x % Q;
    let mut x = A * lo - R * hi;
    if x < 0 {{
        x += INT32_MAX;
    }}
    x
}}

/// The projections the shipped `ComputeLSH` uses: 16 vectors of 768 normal
/// samples, drawn from `rand.New(rand.NewSource(42))` in row-major order.
pub(crate) fn lsh_projections() -> &'static [[f32; 768]] {{
    static PROJECTIONS: std::sync::OnceLock<Vec<[f32; 768]>> = std::sync::OnceLock::new();
    PROJECTIONS.get_or_init(|| {{
        let mut generator = GoRand::new(42);
        let mut rows = Vec::with_capacity(16);
        for _ in 0..16 {{
            let mut row = [0.0_f32; 768];
            for value in &mut row {{
                {{
                    *value = generator.norm_float64() as f32;
                }}
            }}
            rows.push(row);
        }}
        rows
    }})
}}

{TESTS.rstrip()}
'''

    output.write_text(body, encoding="utf-8")
    print(f"wrote {output} ({len(body)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
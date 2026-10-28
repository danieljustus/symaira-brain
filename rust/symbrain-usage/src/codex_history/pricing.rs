use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Rate {
    input: u64,
    output: u64,
    read: u64,
    input_long: Option<u64>,
    output_long: Option<u64>,
    read_long: Option<u64>,
    fast2: u64,
    ultra2: u64,
}
#[derive(Deserialize)]
pub(super) struct Prices {
    pub(super) snapshot: String,
    rates: BTreeMap<String, Rate>,
    aliases: BTreeMap<String, String>,
}
pub(super) fn estimate(m: &str, b: [u64; 4], tier: &str, prices: &Prices) -> Option<u64> {
    let m = prices.aliases.get(m).map_or(m, String::as_str);
    let fast = m.ends_with("-fast");
    let m = m.strip_suffix("-fast").unwrap_or(m);
    let r = prices.rates.get(m)?;
    let mut rates = [r.input, r.output, r.read];
    if b[0] > 272_000 {
        for (n, long) in rates
            .iter_mut()
            .zip([r.input_long, r.output_long, r.read_long])
        {
            if let Some(v) = long {
                *n = v;
            }
        }
    }
    let mul = match tier {
        "" | "standard" | "default" => {
            if fast {
                r.fast2
            } else {
                2
            }
        }
        "fast" | "priority" => r.fast2,
        "ultrafast" => r.ultra2,
        _ => return None,
    };
    for r in &mut rates {
        *r = r.checked_mul(mul)? / 2;
    }
    let n = [b[0] - b[1], b[2], b[1]]
        .into_iter()
        .zip(rates)
        .try_fold(0_u64, |n, (b, r)| n.checked_add(b.checked_mul(r)?))?;
    (n <= 1_000_000_000_000_000).then_some(n)
}

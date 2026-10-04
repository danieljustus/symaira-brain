//! Explicit-input package oracle probe, not an application command or fallback.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use symbrain_memory::engine::{
    AgingConfig, BudgetPiece, PatternExtractor, decay_factor, enforce_budget, estimate_tokens,
    summarize_session,
};

#[derive(Deserialize)]
struct Piece {
    layer_hex: String,
    tokens: i64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Case {
    id: String,
    operation: String,
    text_hex: String,
    max: i64,
    created: String,
    last: String,
    now: String,
    access_count: i64,
    enabled: bool,
    half_life_bits: String,
    boost_cap: i64,
    pieces: Vec<Piece>,
}

fn bytes(raw: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if !raw.len().is_multiple_of(2) {
        return Err("odd hex input".into());
    }
    raw.as_bytes()
        .chunks_exact(2)
        .map(|pair| Ok(u8::from_str_radix(std::str::from_utf8(pair)?, 16)?))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut output = String::new();
    for byte in bytes {
        write!(&mut output, "{byte:02x}").expect("String write cannot fail");
    }
    output
}

fn time(raw: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(raw).map(|time| time.with_timezone(&Utc))
}

fn observe(case: Case) -> Result<Value, Box<dyn std::error::Error>> {
    let text = bytes(&case.text_hex)?;
    let mut result = json!({"id":case.id,"operation":case.operation});
    match case.operation.as_str() {
        "pattern" => {
            let facts = PatternExtractor::new()?.extract(std::str::from_utf8(&text)?);
            result["facts"] = if facts.is_empty() {
                Value::Null
            } else {
                serde_json::to_value(facts)?
            };
        }
        "summary" => {
            result["summary_hex"] =
                hex(&summarize_session(&text, usize::try_from(case.max)?)).into()
        }
        "tokens" => result["tokens"] = estimate_tokens(&text).into(),
        "aging" => {
            let config = AgingConfig {
                enabled: case.enabled,
                access_half_life_days: f64::from_bits(u64::from_str_radix(
                    &case.half_life_bits,
                    16,
                )?),
                access_boost_cap: case.boost_cap,
                ..AgingConfig::default()
            };
            let last = if case.last.is_empty() {
                None
            } else {
                Some(time(&case.last)?)
            };
            let factor = decay_factor(
                config,
                time(&case.created)?,
                last,
                case.access_count,
                time(&case.now)?,
            );
            result["factor_bits"] = format!("{:x}", factor.to_bits()).into();
        }
        "budget" => {
            let mut pieces = case
                .pieces
                .into_iter()
                .map(|piece| {
                    Ok(BudgetPiece {
                        layer: bytes(&piece.layer_hex)?,
                        tokens: piece.tokens,
                    })
                })
                .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
            result["report"] = enforce_budget(&mut pieces, case.max).map_or(Value::Null, |report| json!({"max_tokens":report.max_tokens,"estimated_tokens":report.estimated_tokens,"dropped_pieces":report.dropped_pieces,"dropped_ids_hex":report.dropped_ids.iter().map(|id| hex(id)).collect::<Vec<_>>(),"reason":report.reason,"fit":report.fit}));
            result["remaining_layers_hex"] = json!(
                pieces
                    .iter()
                    .map(|piece| hex(&piece.layer))
                    .collect::<Vec<_>>()
            );
        }
        _ => return Err("unknown operation".into()),
    }
    Ok(result)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input = PathBuf::from(args.next().ok_or("explicit input required")?);
    let output = PathBuf::from(args.next().ok_or("explicit output required")?);
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(input)?)?;
    if cases.is_empty() {
        return Err("zero-case probe is not evidence".into());
    }
    let results = cases
        .into_iter()
        .map(observe)
        .collect::<Result<Vec<_>, _>>()?;
    std::fs::write(output, serde_json::to_vec(&results)?)?;
    Ok(())
}

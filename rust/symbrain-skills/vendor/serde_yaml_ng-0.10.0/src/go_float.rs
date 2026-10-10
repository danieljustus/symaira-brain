// Go FormatFloat('g', -1): retain Ryu's shortest digits, choose Go's layout.
// This changes scalar formatting before emission, never YAML output bytes.
pub(super) fn finite(text: &str) -> String {
    let (sign, text) = text.strip_prefix('-').map_or(("", text), |s| ("-", s));
    let (mantissa, suffix) = text.split_once('e').unwrap_or((text, "0"));
    let exponent: i32 = suffix.parse().expect("Ryu exponent");
    let decimal = mantissa.find('.').unwrap_or(mantissa.len());
    let mut digits = mantissa.replace('.', "");
    let leading = digits.bytes().take_while(|b| *b == b'0').count();
    if leading == digits.len() {
        return format!("{sign}0");
    }
    let power = exponent + i32::try_from(decimal).expect("Ryu decimal")
        - i32::try_from(leading).expect("Ryu leading zeros")
        - 1;
    digits.drain(..leading);
    while digits.ends_with('0') {
        digits.pop();
    }
    if !(-4..6).contains(&power) {
        let first = &digits[..1];
        let rest = &digits[1..];
        let fraction = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        format!("{sign}{first}{fraction}e{power:+03}")
    } else {
        let point = power + 1;
        if point <= 0 {
            format!(
                "{sign}0.{}{digits}",
                "0".repeat(usize::try_from(-point).expect("small power"))
            )
        } else {
            let point = usize::try_from(point).expect("positive power");
            if point >= digits.len() {
                format!("{sign}{digits}{}", "0".repeat(point - digits.len()))
            } else {
                format!("{sign}{}.{}", &digits[..point], &digits[point..])
            }
        }
    }
}

//! Local time.Time JSON owner for persisted rotation expiry, not Store timestamps.
//! Go-derived rules: BSD-3-Clause license in migration/fixtures/brain-config13.
use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveDateTime, Timelike, Utc};
use symbrain_core::GoText;

const LAYOUT: &str = "2006-01-02T15:04:05Z07:00";
pub(super) struct Time { local: NaiveDateTime, offset: i32 }

fn quote(bytes: &[u8]) -> String {
    // time.ParseError has its own bytewise quote, distinct from strconv.Quote.
    let mut result = String::from("\"");
    for &byte in bytes {
        if byte >= 128 || byte < 32 {
            use std::fmt::Write;
            write!(result, "\\x{byte:02x}").expect("String write");
        } else {
            if matches!(byte, b'"' | b'\\') { result.push('\\'); }
            result.push(char::from(byte));
        }
    }
    result.push('"'); result
}
fn cannot(raw: &[u8], rest: &[u8], element: &str) -> GoText {
    format!("parsing time {} as {}: cannot parse {} as {}", quote(raw), quote(LAYOUT.as_bytes()), quote(rest), quote(element.as_bytes())).into()
}
fn range(raw: &[u8], name: &str) -> GoText {
    format!("parsing time {}: {name} out of range", quote(raw)).into()
}
struct Parser<'a> { raw: &'a [u8], at: usize }
impl Parser<'_> {
    fn digits(&mut self, n: usize, element: &str) -> Result<u32, GoText> {
        let rest = &self.raw[self.at..];
        let value = rest.get(..n).filter(|v| v.iter().all(u8::is_ascii_digit))
            .ok_or_else(|| cannot(self.raw, rest, element))?;
        self.at += n;
        Ok(value.iter().fold(0, |sum, b| sum * 10 + u32::from(b-b'0')))
    }
    fn literal(&mut self, byte: u8, element: &str) -> Result<(), GoText> {
        if self.raw.get(self.at) != Some(&byte) { return Err(cannot(self.raw, &self.raw[self.at..], element)); }
        self.at += 1; Ok(())
    }
}
impl Time {
    pub(super) fn zero() -> Self {
        Self { local: NaiveDate::from_ymd_opt(1,1,1).expect("Go zero date").and_hms_opt(0,0,0).expect("Go zero time"), offset: 0 }
    }
    pub(super) fn after(&self, now: DateTime<Utc>) -> bool {
        (self.local - Duration::seconds(i64::from(self.offset))).and_utc() > now
    }
    pub(super) fn json(raw: &[u8]) -> Result<Self, GoText> {
        // Go does not JSON-unescape timestamps; a literal \u0032 is rejected.
        if raw.len() < 2 || raw.first() != Some(&b'"') || raw.last() != Some(&b'"') {
            return Err("Time.UnmarshalJSON: input is not a JSON string".into());
        }
        let raw = &raw[1..raw.len()-1];
        let mut p = Parser { raw, at: 0 };
        let year = i32::try_from(p.digits(4,"2006")?).expect("four digits");
        p.literal(b'-', "-")?;
        let month = p.digits(2,"01")?;
        if !(1..=12).contains(&month) { return Err(range(raw,"month")); }
        p.literal(b'-', "-")?;
        let day = p.digits(2,"02")?;
        p.literal(b'T', "T")?;
        // RFC3339's template has stdHour, accepting one or two digits.
        let hour_digits = if raw.get(p.at+1).is_some_and(u8::is_ascii_digit) { 2 } else { 1 };
        let hour = p.digits(hour_digits,"15")?;
        if hour >= 24 { return Err(range(raw,"hour")); }
        p.literal(b':', ":")?;
        let minute = p.digits(2,"04")?;
        if minute >= 60 { return Err(range(raw,"minute")); }
        p.literal(b':', ":")?;
        let second = p.digits(2,"05")?;
        if second >= 60 { return Err(range(raw,"second")); }
        let mut nanos = 0;
        if raw.get(p.at).is_some_and(|b| matches!(b,b'.'|b',')) && raw.get(p.at+1).is_some_and(u8::is_ascii_digit) {
            p.at += 1;
            let mut digits = 0;
            while raw.get(p.at).is_some_and(u8::is_ascii_digit) {
                if digits < 9 { nanos = nanos*10 + u32::from(raw[p.at]-b'0'); }
                digits += 1; p.at += 1;
            }
            for _ in digits..9 { nanos *= 10; }
        }
        let zone_start = p.at;
        let offset = if raw.get(p.at) == Some(&b'Z') { p.at += 1; 0 }
        else {
            let zone = raw.get(p.at..p.at+6).filter(|zone| zone[3] == b':')
                .ok_or_else(|| cannot(raw, &raw[zone_start..], "Z07:00"))?;
            if !zone[1..3].iter().chain(&zone[4..6]).all(u8::is_ascii_digit) {
                return Err(cannot(raw,&raw[zone_start..],"Z07:00"));
            }
            let hours = i32::from(zone[1]-b'0')*10 + i32::from(zone[2]-b'0');
            let minutes = i32::from(zone[4]-b'0')*10 + i32::from(zone[5]-b'0');
            // Go's fallback intentionally allows exactly hour24/minute60.
            if minutes > 60 { return Err(range(raw,"time zone offset minute")); }
            if hours > 24 { return Err(range(raw,"time zone offset hour")); }
            if !matches!(zone[0],b'+'|b'-') { return Err(cannot(raw,&raw[zone_start..],"Z07:00")); }
            p.at += 6;
            let seconds = (hours*60 + minutes)*60;
            if zone[0] == b'-' { -seconds } else { seconds }
        };
        if p.at != raw.len() {
            return Err(format!("parsing time {}: extra text: {}",quote(raw),quote(&raw[p.at..])).into());
        }
        let date = NaiveDate::from_ymd_opt(year,month,day).ok_or_else(|| range(raw,"day"))?;
        let local = date.and_hms_nano_opt(hour,minute,second,nanos).expect("checked clock range");
        Ok(Self { local, offset })
    }
    pub(super) fn render(&self) -> Result<String, GoText> {
        // Parse admission and marshal admission deliberately differ in Go.
        if !(0..=9999).contains(&self.local.year()) {
            return Err("Time.MarshalJSON: year outside of range [0,9999]".into());
        }
        if self.offset.abs()/3600 >= 24 {
            return Err("Time.MarshalJSON: timezone hour outside of range [0,23]".into());
        }
        let mut result = self.local.format("%Y-%m-%dT%H:%M:%S").to_string();
        let nanos = self.local.nanosecond();
        if nanos != 0 { result.push_str(&format!(".{}",format!("{nanos:09}").trim_end_matches('0'))); }
        if self.offset == 0 { result.push('Z'); }
        else {
            let minutes = self.offset.abs()/60;
            result.push_str(&format!("{}{:02}:{:02}", if self.offset < 0 { '-' } else { '+' },minutes/60,minutes%60));
        }
        Ok(result)
    }
}

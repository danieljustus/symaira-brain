//! Byte-exact model of Go's `flag.FlagSet.Parse` for string flags.
//!
//! The shipped Go commands call `fs.Parse(normalizeFlags(args))` with
//! `flag.ContinueOnError`. The standard library stops at the first positional
//! argument, treats `--` as a terminator, reports bad syntax and unknown flags
//! on the flag set's output followed by its usage, and answers `-h`/`-help`
//! (any dash count) with the usage alone when no such flag is defined.
//! Mirroring that scanner keeps every flag-grammar diagnostic native instead of
//! routing it to the Go binary.

use std::ffi::OsString;
use std::io::Write;

use symbrain_core::config::os_bytes;

/// What Go prints after a parse failure: the flag set's usage text.
pub(crate) struct FlagSet<'a> {
    /// Flag names (without dashes) that take a value.
    pub string_flags: &'a [&'static str],
    /// Usage text written to the output after each diagnostic.
    pub usage: &'a str,
}

/// Successful parse: last value per flag plus the remaining positionals.
#[derive(Debug, Default)]
pub(crate) struct Parsed {
    values: Vec<(&'static str, OsString)>,
    pub rest: Vec<OsString>,
}

impl Parsed {
    /// Returns the last value set for `name`, like Go's flag value pointer.
    pub fn value(&self, name: &str) -> Option<&OsString> {
        self.values
            .iter()
            .rev()
            .find(|(flag, _)| *flag == name)
            .map(|(_, value)| value)
    }
}

/// Parse failure; diagnostics have already been written to the output.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ParseFailed;

fn bytes_to_os(bytes: &[u8]) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(bytes.to_vec())
    }
    #[cfg(not(unix))]
    {
        OsString::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn fail(output: &mut dyn Write, set: &FlagSet<'_>, message: &[u8]) -> ParseFailed {
    let mut line = message.to_vec();
    line.push(b'\n');
    let _ = output.write_all(&line);
    let _ = output.write_all(set.usage.as_bytes());
    ParseFailed
}

impl FlagSet<'_> {
    /// Parses already-normalized arguments the way `flag.FlagSet.Parse` does.
    ///
    /// # Errors
    /// Returns [`ParseFailed`] after writing Go's diagnostic and usage text.
    pub fn parse(&self, args: &[OsString], output: &mut dyn Write) -> Result<Parsed, ParseFailed> {
        let mut parsed = Parsed::default();
        let mut index = 0;
        while index < args.len() {
            let token = os_bytes(&args[index]);
            let token: &[u8] = &token;
            if token.len() < 2 || token[0] != b'-' {
                break;
            }
            let mut dashes = 1;
            if token[1] == b'-' {
                dashes = 2;
                if token.len() == 2 {
                    index += 1;
                    break;
                }
            }
            let name_and_value = &token[dashes..];
            if name_and_value[0] == b'-' || name_and_value[0] == b'=' {
                let mut message = b"bad flag syntax: ".to_vec();
                message.extend_from_slice(token);
                return Err(fail(output, self, &message));
            }
            index += 1;
            let split = name_and_value
                .iter()
                .skip(1)
                .position(|byte| *byte == b'=')
                .map(|offset| offset + 1);
            let (name, inline) = match split {
                Some(at) => (&name_and_value[..at], Some(&name_and_value[at + 1..])),
                None => (name_and_value, None),
            };
            let Some(flag) = self
                .string_flags
                .iter()
                .find(|flag| flag.as_bytes() == name)
            else {
                if name == b"help" || name == b"h" {
                    let _ = output.write_all(self.usage.as_bytes());
                    return Err(ParseFailed);
                }
                let mut message = b"flag provided but not defined: -".to_vec();
                message.extend_from_slice(name);
                return Err(fail(output, self, &message));
            };
            let value = if let Some(inline) = inline {
                bytes_to_os(inline)
            } else if let Some(next) = args.get(index) {
                index += 1;
                next.clone()
            } else {
                let mut message = b"flag needs an argument: -".to_vec();
                message.extend_from_slice(name);
                return Err(fail(output, self, &message));
            };
            parsed.values.push((flag, value));
        }
        parsed.rest = args[index..].to_vec();
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const USAGE: &str = "USAGE\n";

    fn parse(args: &[&str]) -> (Result<Parsed, ParseFailed>, String) {
        let set = FlagSet {
            string_flags: &["project"],
            usage: USAGE,
        };
        let args = args.iter().map(OsString::from).collect::<Vec<_>>();
        let mut output = Vec::new();
        let result = set.parse(&args, &mut output);
        (result, String::from_utf8(output).unwrap())
    }

    #[test]
    fn stops_at_first_positional_and_terminator() {
        let (parsed, out) = parse(&["-project", "a", "x", "-project", "b"]);
        let parsed = parsed.unwrap();
        assert!(out.is_empty());
        assert_eq!(parsed.value("project"), Some(&OsString::from("a")));
        assert_eq!(parsed.rest, ["x", "-project", "b"].map(OsString::from));
        let (parsed, _) = parse(&["--", "-project"]);
        assert_eq!(parsed.unwrap().rest, [OsString::from("-project")]);
        let (parsed, _) = parse(&["-"]);
        assert_eq!(parsed.unwrap().rest, [OsString::from("-")]);
    }

    #[test]
    fn inline_values_may_be_empty_and_last_wins() {
        let (parsed, _) = parse(&["-project=", "-project=b=c"]);
        let parsed = parsed.unwrap();
        assert_eq!(parsed.value("project"), Some(&OsString::from("b=c")));
    }

    #[test]
    fn diagnostics_match_the_go_flag_package() {
        for (args, expected) in [
            (&["-x"][..], "flag provided but not defined: -x\nUSAGE\n"),
            (&["-x=1"], "flag provided but not defined: -x\nUSAGE\n"),
            (&["--=x"], "bad flag syntax: --=x\nUSAGE\n"),
            (&["---x"], "bad flag syntax: ---x\nUSAGE\n"),
            (&["-=x"], "bad flag syntax: -=x\nUSAGE\n"),
            (&["-project"], "flag needs an argument: -project\nUSAGE\n"),
            (&["-h"], "USAGE\n"),
            (&["--help"], "USAGE\n"),
            (&["-help=1"], "USAGE\n"),
        ] {
            let (result, out) = parse(args);
            assert_eq!(result.unwrap_err(), ParseFailed, "{args:?}");
            assert_eq!(out, expected, "{args:?}");
        }
    }
}

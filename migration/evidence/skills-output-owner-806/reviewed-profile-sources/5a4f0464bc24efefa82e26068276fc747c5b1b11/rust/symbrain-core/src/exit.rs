//! CLI process exit codes matching the Symaira ecosystem conventions.

pub const OK: u8 = 0;
pub const GENERIC: u8 = 1;
pub const NO_INPUT: u8 = 2;
pub const USAGE: u8 = 2;
pub const NO_AUTH: u8 = 3;
pub const FORBIDDEN: u8 = 4;
pub const NOT_FOUND: u8 = 5;
pub const CONFLICT: u8 = 6;
pub const SOFTWARE: u8 = 7;
pub const DATA: u8 = 8;
pub const CONFIG: u8 = 9;
pub const INTERRUPTED: u8 = 10;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_constants_match_symaira_specification() {
        assert_eq!(OK, 0);
        assert_eq!(GENERIC, 1);
        assert_eq!(NO_INPUT, 2);
        assert_eq!(USAGE, 2);
        assert_eq!(NO_AUTH, 3);
        assert_eq!(FORBIDDEN, 4);
        assert_eq!(NOT_FOUND, 5);
        assert_eq!(CONFLICT, 6);
        assert_eq!(SOFTWARE, 7);
        assert_eq!(DATA, 8);
        assert_eq!(CONFIG, 9);
        assert_eq!(INTERRUPTED, 10);
    }
}

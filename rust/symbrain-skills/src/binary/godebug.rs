// Copyright 2009 The Go Authors.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are
// met:
//
//    * Redistributions of source code must retain the above copyright
// notice, this list of conditions and the following disclaimer.
//    * Redistributions in binary form must reproduce the above
// copyright notice, this list of conditions and the following disclaimer
// in the documentation and/or other materials provided with the
// distribution.
//    * Neither the name of Google LLC nor the names of its
// contributors may be used to endorse or promote products derived from
// this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
// OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
// LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
// DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

//! Scoped execerrdot values; never guess Go runtime call-stack bisect IDs.
//! Grammar adapted from Go Authors internal/godebug (2021) and bisect (2023).
use crate::SkillError;

pub(super) fn allow_relative() -> Result<bool, SkillError> {
    setting(
        std::env::var_os("GODEBUG")
            .as_deref()
            .map(std::ffi::OsStr::as_encoded_bytes)
            .unwrap_or_default(),
    )
}

fn setting(env: &[u8]) -> Result<bool, SkillError> {
    let Some(value) = env
        .split(|byte| *byte == b',')
        .filter_map(|part| part.strip_prefix(b"execerrdot="))
        .next_back()
    else {
        return Ok(false);
    };
    let (text, pattern) = match value.iter().position(|byte| *byte == b'#') {
        Some(index) => (&value[..index], Some(&value[index + 1..])),
        None => (value, None),
    };
    // Both "1" and the empty disabled Setting.Value refuse relative owners.
    // Such a setting never authorizes execution regardless of a stack matcher.
    if text != b"0" {
        return Ok(false);
    }
    let Some(pattern) = pattern else {
        return Ok(true);
    };
    // Go parse ignores bisect.New errors, retaining text with a nil matcher.
    let Some(matcher) = parse(pattern) else {
        return Ok(true);
    };
    match matcher.tree {
        Tree::Leaf(result) if matcher.quiet || !result => Ok(result == matcher.enable),
        _ => Err(SkillError("skills execerrdot bisect requires Go runtime stack/report identity; unsupported conditional or reporting pattern".to_owned())),
    }
}

struct Matcher {
    quiet: bool,
    enable: bool,
    tree: Tree,
}
enum Tree {
    Leaf(bool),
    Branch(Box<[Tree; 2]>),
}
impl Tree {
    // Suffix masks form a trie from the low bit, not a sampled hash corpus.
    fn assign(&mut self, bits: u64, width: usize, result: bool) {
        // Go tests id & mask == bits without masking the stored bits. A
        // condition with any bit outside its mask matches no ID (including
        // accepted hex tokens whose leading y reduces the mask to zero).
        // Ignore that condition without changing earlier additions/removals.
        if width < 64 && bits >> width != 0 {
            return;
        }
        if width == 0 {
            *self = Self::Leaf(result);
            return;
        }
        if let Self::Leaf(previous) = *self {
            *self = Self::Branch(Box::new([Self::Leaf(previous), Self::Leaf(previous)]));
        }
        let Self::Branch(children) = self else {
            unreachable!()
        };
        children[(bits & 1) as usize].assign(bits >> 1, width - 1, result);
        if let (Self::Leaf(a), Self::Leaf(b)) = (&children[0], &children[1])
            && a == b
        {
            *self = Self::Leaf(*a);
        }
    }
}

// Grammar follows pinned internal/bisect.New, including q/v/! ordering,
// hex width, leading subtraction, n alias, invalid patterns and later rules.
// This parses static sets only: it does not synthesize Go stack PCs/reports.
fn parse(pattern: &[u8]) -> Option<Matcher> {
    if pattern.is_empty() {
        return None;
    }
    let mut p = pattern;
    let mut quiet = false;
    if p[0] == b'q' {
        quiet = true;
        p = &p[1..];
    }
    while p.first() == Some(&b'v') {
        quiet = false;
        p = &p[1..];
    }
    let mut enable = true;
    while p.first() == Some(&b'!') {
        enable = !enable;
        p = &p[1..];
    }
    if p.is_empty() {
        return None;
    }
    if p == b"n" {
        enable = !enable;
        p = b"y";
    }
    let mut matcher = Matcher {
        quiet,
        enable,
        tree: Tree::Leaf(false),
    };
    let (mut result, mut bits, mut start, mut width) = (true, 0u64, 0usize, 1u8);
    for index in 0..=p.len() {
        let byte = p.get(index).copied().unwrap_or(b'-');
        if index == start && width == 1 && byte == b'x' {
            start = index + 1;
            width = 4;
            continue;
        }
        match byte {
            b'0'..=b'9' if width == 4 || byte <= b'1' => {
                bits = bits.wrapping_shl(u32::from(width)) | u64::from(byte - b'0');
            }
            b'a'..=b'f' | b'A'..=b'F' if width == 4 => {
                bits = bits.wrapping_shl(4) | u64::from(byte.to_ascii_uppercase() - b'A' + 10);
            }
            b'y' => {
                if p.get(index + 1).is_some_and(|b| matches!(*b, b'0' | b'1')) {
                    return None;
                }
                bits = 0;
            }
            b'+' | b'-' => {
                if byte == b'+' && !result {
                    return None;
                }
                if index > 0 {
                    let mut count = (index - start) * usize::from(width);
                    if count == 0 || count > 64 {
                        return None;
                    }
                    if p[start] == b'y' {
                        count = 0;
                    }
                    matcher.tree.assign(bits, count, result);
                } else if byte == b'-' {
                    matcher.tree.assign(0, 0, true);
                }
                bits = 0;
                result = byte == b'+';
                start = index + 1;
                width = 1;
            }
            _ => return None,
        }
    }
    Some(matcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suffixes_and_last_setting_keep_exact_quiet_universal_semantics() {
        for (env, expected) in [
            ("", false),
            ("execerrdot=0", true),
            ("execerrdot=0#", true),
            ("execerrdot=0#qy", true),
            ("execerrdot=0#qn", false),
            ("execerrdot=0#q!!y", true),
            ("execerrdot=0#q0+1", true),
            ("execerrdot=0#qy-y", false),
            ("execerrdot=0#q!y-y", true),
            ("execerrdot=0#not-a-pattern", true),
            ("execerrdot=0#q", true),
            ("execerrdot=0#q0-1+0", true),
            ("execerrdot=0#qyy", true),
            ("execerrdot=0#qxy", true),
            ("execerrdot=0#q-0-1", false),
            ("execerrdot=0#qy,execerrdot=1", false),
            ("execerrdot=1,execerrdot=0#qy", true),
            ("execerrdot=00#qy", false),
            ("execerrdot=0#qy#", true),
            ("execerrdot=0=1#qy", false),
        ] {
            assert_eq!(setting(env.as_bytes()).unwrap(), expected, "{env}");
        }
    }
    #[test]
    fn stack_selective_and_reported_patterns_are_not_a_silent_opt_in() {
        for pattern in ["q0", "q1", "qxf", "y", "n", "qy-0", "qvy", "vqy", "!y"] {
            // vqy is invalid in Go, so nil matcher preserves the text.
            if pattern == "vqy" {
                assert!(setting(b"execerrdot=0#vqy").unwrap());
                continue;
            }
            assert!(
                setting(format!("execerrdot=0#{pattern}").as_bytes()).is_err(),
                "{pattern}"
            );
        }
    }

    #[test]
    fn impossible_conditions_preserve_prior_rules_and_valid_nil_matchers() {
        for (pattern, expected) in [
            ("qxyf", false),
            ("qxya", false),
            ("qxyF", false),
            ("qxy9", false),
            ("q!xyf", true),
            ("q!!xyf", false),
            ("q-xyf", true),
            ("qy-xyf", true),
            ("q!y-xyf", false),
            ("qxyf+y", true),
            ("qy+xyf", true),
            ("qxyf+0+1", true),
            ("q0+1-xyf", true),
            ("qxyf+y-y", false),
            ("q-xyf-y", false),
            ("qxy0", true), // Invalid syntax keeps Go's nil matcher.
            ("qxy00", true),
            ("qxy", true), // Valid zero bits with mask zero still match.
        ] {
            assert_eq!(
                setting(format!("execerrdot=0#{pattern}").as_bytes()).unwrap(),
                expected,
                "{pattern}"
            );
        }
        for pattern in ["qxyf+0", "q!xyf+0", "q0-xyf", "q-xyf-0"] {
            assert!(
                setting(format!("execerrdot=0#{pattern}").as_bytes()).is_err(),
                "{pattern} must retain its real conditional set"
            );
        }
    }

    #[test]
    fn trie_assignment_preserves_mask_bits_for_every_width() {
        fn value(tree: &Tree, id: u64) -> bool {
            match tree {
                Tree::Leaf(result) => *result,
                Tree::Branch(children) => value(&children[(id & 1) as usize], id >> 1),
            }
        }
        for width in 0..64 {
            for previous in [false, true] {
                let mut tree = Tree::Leaf(previous);
                tree.assign(1 << width, width, !previous);
                assert!(matches!(tree, Tree::Leaf(result) if result == previous));
            }
        }
        let mut tree = Tree::Leaf(false);
        tree.assign(1 << 63, 64, true);
        assert!(value(&tree, 1 << 63));
        assert!(!value(&tree, 0));
        tree.assign(3, 1, false); // Unsatisfiable subtraction leaves it intact.
        assert!(value(&tree, 1 << 63));
        tree.assign(0, 0, true); // The satisfiable universal rule still wins.
        assert!(matches!(tree, Tree::Leaf(true)));
    }
}

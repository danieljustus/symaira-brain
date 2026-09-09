use std::fmt::Write;

const CONTEXT: usize = 3;
const MAX_LINES: usize = 10_000;
const MAX_COMPARISON_CELLS: usize = 4_000_000;

#[derive(Clone, Copy)]
enum Kind {
    Equal,
    Delete,
    Insert,
}
struct Op {
    kind: Kind,
    line: String,
}
struct Hunk {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
    ops: Vec<Op>,
}

/// Produces the Go-compatible three-context unified diff.
#[must_use]
pub fn unified_diff(path: &str, old: &[u8], new: &[u8]) -> String {
    let old = split_lines(old);
    let new = split_lines(new);
    let comparison_cells = old.len().checked_add(1).and_then(|old| {
        new.len()
            .checked_add(1)
            .and_then(|new| old.checked_mul(new))
    });
    if old.len() > MAX_LINES
        || new.len() > MAX_LINES
        || comparison_cells.is_none_or(|cells| cells > MAX_COMPARISON_CELLS)
    {
        return format!(
            "--- {path}\n+++ {path}\nfile too large to diff safely: {} old lines, {} new lines (max {MAX_LINES} lines and {MAX_COMPARISON_CELLS} comparison cells); full diff skipped\n",
            old.len(),
            new.len()
        );
    }
    let ops = lcs(&old, &new);
    let hunks = group_hunks(&ops);
    if hunks.is_empty() {
        return String::new();
    }
    let mut output = format!("--- {path}\n+++ {path}\n");
    for hunk in hunks {
        write_hunk(&mut output, hunk);
    }
    output
}

fn split_lines(bytes: &[u8]) -> Vec<String> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_suffix('\n').unwrap_or(&text);
    text.split('\n').map(str::to_owned).collect()
}

fn lcs(old: &[String], new: &[String]) -> Vec<Op> {
    let mut table = vec![vec![0usize; new.len() + 1]; old.len() + 1];
    for i in (0..old.len()).rev() {
        for j in (0..new.len()).rev() {
            table[i][j] = if old[i] == new[j] {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let mut ops = Vec::with_capacity(old.len() + new.len());
    let (mut i, mut j) = (0, 0);
    while i < old.len() && j < new.len() {
        if old[i] == new[j] {
            ops.push(Op {
                kind: Kind::Equal,
                line: old[i].clone(),
            });
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            ops.push(Op {
                kind: Kind::Delete,
                line: old[i].clone(),
            });
            i += 1;
        } else {
            ops.push(Op {
                kind: Kind::Insert,
                line: new[j].clone(),
            });
            j += 1;
        }
    }
    while i < old.len() {
        ops.push(Op {
            kind: Kind::Delete,
            line: old[i].clone(),
        });
        i += 1;
    }
    while j < new.len() {
        ops.push(Op {
            kind: Kind::Insert,
            line: new[j].clone(),
        });
        j += 1;
    }
    ops
}

fn group_hunks(ops: &[Op]) -> Vec<Hunk> {
    let mut old_pos = vec![0usize; ops.len() + 1];
    let mut new_pos = vec![0usize; ops.len() + 1];
    let mut changes = Vec::new();
    for (index, op) in ops.iter().enumerate() {
        old_pos[index + 1] = old_pos[index];
        new_pos[index + 1] = new_pos[index];
        match op.kind {
            Kind::Equal => {
                old_pos[index + 1] += 1;
                new_pos[index + 1] += 1;
            }
            Kind::Delete => old_pos[index + 1] += 1,
            Kind::Insert => new_pos[index + 1] += 1,
        }
        if !matches!(op.kind, Kind::Equal) {
            changes.push(index);
        }
    }
    let mut hunks = Vec::new();
    let mut start = 0;
    while start < changes.len() {
        let mut end = start;
        while end + 1 < changes.len() && changes[end + 1] - changes[end] - 1 <= 2 * CONTEXT {
            end += 1;
        }
        let lo = changes[start].saturating_sub(CONTEXT);
        let hi = (changes[end] + CONTEXT).min(ops.len().saturating_sub(1));
        hunks.push(build_hunk(&ops[lo..=hi], old_pos[lo], new_pos[lo]));
        start = end + 1;
    }
    hunks
}

fn build_hunk(ops: &[Op], old_from: usize, new_from: usize) -> Hunk {
    let (mut old_count, mut new_count) = (0, 0);
    for op in ops {
        match op.kind {
            Kind::Equal => {
                old_count += 1;
                new_count += 1;
            }
            Kind::Delete => old_count += 1,
            Kind::Insert => new_count += 1,
        }
    }
    Hunk {
        old_start: old_from + 1,
        old_count,
        new_start: new_from + 1,
        new_count,
        ops: ops
            .iter()
            .map(|op| Op {
                kind: op.kind,
                line: op.line.clone(),
            })
            .collect(),
    }
}

fn write_hunk(output: &mut String, hunk: Hunk) {
    let _ = writeln!(
        output,
        "@@ -{},{} +{},{} @@",
        hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
    );
    for op in hunk.ops {
        let prefix = match op.kind {
            Kind::Equal => ' ',
            Kind::Delete => '-',
            Kind::Insert => '+',
        };
        let _ = writeln!(output, "{prefix}{}", op.line);
    }
}

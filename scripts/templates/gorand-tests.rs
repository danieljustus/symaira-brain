#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::{GoRand, lsh_projections};

    /// Raw capture of the Go toolchain's values; the file header documents how
    /// to regenerate it.
    const REFERENCE: &str = include_str!("../tests/fixtures/gorand-reference.txt");

    fn reference() -> (Vec<String>, Vec<String>, String) {
        let mut normals = Vec::new();
        let mut rows = Vec::new();
        let mut all = String::new();
        for line in REFERENCE.lines().filter(|line| !line.starts_with('#')) {
            if let Some(value) = line.strip_prefix("row ") {
                rows.push(
                    value
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default()
                        .to_owned(),
                );
            } else if let Some(value) = line.strip_prefix("all ") {
                all = value.to_owned();
            } else {
                normals.push(line.to_owned());
            }
        }
        (normals, rows, all)
    }

    fn digest(values: impl Iterator<Item = f32>) -> String {
        let mut hasher = Sha256::new();
        for value in values {
            hasher.update(value.to_bits().to_le_bytes());
        }
        format!("{:x}", hasher.finalize())
    }

    #[test]
    fn the_normal_stream_matches_go() {
        let (expected, _, _) = reference();
        let mut generator = GoRand::new(42);
        for (index, hex) in expected.iter().enumerate() {
            let sample = generator.norm_float64();
            assert_eq!(format!("{:x}", sample.to_bits()), *hex, "sample {index}");
        }
    }

    #[test]
    fn the_projection_rows_match_go() {
        let (_, expected, all) = reference();
        let rows = lsh_projections();
        assert_eq!(rows.len(), expected.len());
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(digest(row.iter().copied()), expected[index], "row {index}");
        }
        assert_eq!(
            digest(rows.iter().flat_map(|row| row.iter().copied())),
            all,
            "all rows"
        );
    }
}

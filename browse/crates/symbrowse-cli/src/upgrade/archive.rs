//! Extract the `symbrowse` binary from a release `.tar.gz`.
//!
//! The symaira-brain release ships gzip-compressed ustar archives
//! (`scripts/rust-release/package.py`, rebuilt by `tar -czf` after darwin
//! signing) with the binary at the archive root next to the docs. Only the
//! member's bytes are returned; its name is never used as a path, so a hostile
//! archive cannot write outside the staging file.

use std::io::Read;

const BLOCK: usize = 512;

/// Returns the contents of the first regular-file member whose base name is
/// `binary`. The uncompressed archive may not exceed `limit` bytes.
pub(super) fn extract_binary(
    archive: impl Read,
    binary: &str,
    limit: u64,
) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    flate2::read::GzDecoder::new(archive)
        .take(limit + 1)
        .read_to_end(&mut data)
        .map_err(|error| format!("decompress archive: {error}"))?;
    if data.len() as u64 > limit {
        return Err(format!("archive exceeds {limit} bytes uncompressed"));
    }
    let mut offset = 0;
    let mut long_name: Option<String> = None;
    while offset + BLOCK <= data.len() {
        let header = &data[offset..offset + BLOCK];
        if header.iter().all(|&byte| byte == 0) {
            break;
        }
        let size = parse_octal(&header[124..136])?;
        let start = offset + BLOCK;
        let end = start
            .checked_add(size)
            .filter(|&end| end <= data.len())
            .ok_or_else(|| "truncated tar member".to_owned())?;
        let name = long_name.take().unwrap_or_else(|| {
            let (prefix, name) = (field(&header[345..500]), field(&header[..100]));
            if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            }
        });
        match header[156] {
            // GNU long name: the next header's real name.
            b'L' => long_name = Some(field(&data[start..end])),
            b'0' | 0 if name.trim_end_matches('/').rsplit('/').next() == Some(binary) => {
                return Ok(data[start..end].to_vec());
            }
            _ => {}
        }
        offset = start + size.div_ceil(BLOCK) * BLOCK;
    }
    Err(format!("archive contains no {binary} binary"))
}

fn field(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn parse_octal(bytes: &[u8]) -> Result<usize, String> {
    let text = field(bytes);
    let text = text.trim_matches(|c: char| c == ' ' || c == '\0');
    if text.is_empty() {
        return Ok(0);
    }
    usize::from_str_radix(text, 8).map_err(|_| format!("invalid tar size field {text:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn member(name: &str, typeflag: u8, data: &[u8]) -> Vec<u8> {
        let mut header = [0u8; BLOCK];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[100..107].copy_from_slice(b"0000755");
        header[124..135].copy_from_slice(format!("{:011o}", data.len()).as_bytes());
        header[156] = typeflag;
        header[257..263].copy_from_slice(b"ustar\0");
        header[148..156].fill(b' ');
        let sum: u32 = header.iter().map(|&b| u32::from(b)).sum();
        header[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        let mut out = header.to_vec();
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(BLOCK) * BLOCK, 0);
        out
    }

    fn gzip(members: &[Vec<u8>]) -> Vec<u8> {
        let mut tar: Vec<u8> = members.concat();
        tar.extend_from_slice(&[0u8; BLOCK * 2]);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&tar).unwrap();
        encoder.finish().unwrap()
    }

    type Case = (
        &'static str,
        Vec<Vec<u8>>,
        Result<&'static [u8], &'static str>,
    );

    #[test]
    fn extracts_only_the_named_regular_file() {
        let cases: [Case; 5] = [
            (
                "release layout",
                vec![
                    member("AGENTS.md", b'0', b"docs"),
                    member("symbrowse", b'0', b"bin"),
                ],
                Ok(b"bin"),
            ),
            (
                "dot-slash prefix",
                vec![member("./symbrowse", b'0', b"bin2")],
                Ok(b"bin2"),
            ),
            (
                "directory is skipped",
                vec![
                    member("symbrowse/", b'5', b""),
                    member("x/symbrowse", 0, b"bin3"),
                ],
                Ok(b"bin3"),
            ),
            (
                "gnu long name",
                vec![
                    member("././@LongLink", b'L', b"deep/dir/symbrowse\0"),
                    member("deep/dir/symbro", b'0', b"bin4"),
                ],
                Ok(b"bin4"),
            ),
            (
                "other product only",
                vec![member("symbrain", b'0', b"brain")],
                Err("archive contains no symbrowse binary"),
            ),
        ];
        for (label, members, want) in cases {
            let got = extract_binary(gzip(&members).as_slice(), "symbrowse", 1 << 20);
            match want {
                Ok(bytes) => assert_eq!(got.as_deref(), Ok(bytes), "{label}"),
                Err(message) => assert_eq!(got, Err(message.to_owned()), "{label}"),
            }
        }
    }

    #[test]
    fn rejects_oversized_and_truncated_archives() {
        let big = gzip(&[member("symbrowse", b'0', &[7u8; 4096])]);
        assert!(
            extract_binary(big.as_slice(), "symbrowse", 1024)
                .unwrap_err()
                .contains("exceeds")
        );
        let mut tar = member("symbrowse", b'0', &[1u8; 2048]);
        tar.truncate(BLOCK + 100);
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&tar).unwrap();
        let truncated = encoder.finish().unwrap();
        assert_eq!(
            extract_binary(truncated.as_slice(), "symbrowse", 1 << 20),
            Err("truncated tar member".to_owned())
        );
    }
}

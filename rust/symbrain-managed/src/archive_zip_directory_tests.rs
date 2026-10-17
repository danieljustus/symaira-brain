use std::io::{Cursor, Seek, SeekFrom};

use super::zip_record_end;

#[test]
fn central_record_lengths_are_raw_bounded_and_checked() {
    let mut header = [0; 46];
    header[..4].copy_from_slice(b"PK\x01\x02");
    for (lengths, file_len, expected) in [
        ([0_u16, 0, 0], 46, Some(46)),
        ([3, 6, 2], 57, Some(57)),
        ([3, 6, 2], 56, None),
        (
            [u16::MAX; 3],
            46 + 3 * u64::from(u16::MAX),
            Some(46 + 3 * u64::from(u16::MAX)),
        ),
        ([u16::MAX; 3], 46, None),
    ] {
        for (offset, value) in [28, 30, 32].into_iter().zip(lengths) {
            header[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(
            zip_record_end(&mut Cursor::new(header), 0, file_len).ok(),
            expected
        );
    }
    assert!(zip_record_end(&mut Cursor::new(header), u64::MAX - 45, u64::MAX).is_err());
    // The fixed header fits, but adding variable fields overflows.
    let mut reader = NearEnd {
        header: Cursor::new(header),
    };
    assert!(zip_record_end(&mut reader, u64::MAX - 46, u64::MAX).is_err());
    for len in 0..46 {
        assert!(zip_record_end(&mut Cursor::new(&header[..len]), 0, 46).is_err());
    }
    header[0] = 0;
    assert!(zip_record_end(&mut Cursor::new(header), 0, 46).is_err());
}

struct NearEnd {
    header: Cursor<[u8; 46]>,
}

impl std::io::Read for NearEnd {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.header, bytes)
    }
}

impl Seek for NearEnd {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        match position {
            SeekFrom::Start(value) => Ok(value),
            _ => unreachable!(),
        }
    }
}

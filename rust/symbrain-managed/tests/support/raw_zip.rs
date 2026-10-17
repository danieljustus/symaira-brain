//! Independent ZIP records: unlike `ZipWriter`, this preserves repeated raw names.
use std::io::Write;

pub fn archive(entries: &[(&[u8], &[u8], u32)], zip64: bool, prefix: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut central = Vec::new();
    for &(name, payload, mode) in entries {
        let offset = u32::try_from(bytes.len()).unwrap();
        let mut crc = flate2::Crc::new();
        crc.update(payload);
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(payload).unwrap();
        let compressed = encoder.finish().unwrap();
        let size = u32::try_from(payload.len()).unwrap();
        let compressed_size = u32::try_from(compressed.len()).unwrap();
        let name_len = u16::try_from(name.len()).unwrap();
        // An unknown extra field and a non-UTF8 comment exercise raw byte lengths.
        let mut extra = b"\xfe\xca\x02\x00\x80\xff".to_vec();
        if zip64 {
            extra.extend_from_slice(b"\x01\x00\x10\x00");
            extra.extend_from_slice(&u64::from(size).to_le_bytes());
            extra.extend_from_slice(&u64::from(compressed_size).to_le_bytes());
        }
        let extra_len = u16::try_from(extra.len()).unwrap();
        let version = if zip64 { 45_u16 } else { 20 };
        let sizes = if zip64 {
            [u32::MAX; 2]
        } else {
            [compressed_size, size]
        };
        let comment = b"\x82\xff";
        bytes.extend_from_slice(b"PK\x03\x04");
        for value in [version, 0, 8, 0, 0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in [crc.sum(), sizes[0], sizes[1]] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&name_len.to_le_bytes());
        bytes.extend_from_slice(&extra_len.to_le_bytes());
        bytes.extend_from_slice(name);
        bytes.extend_from_slice(&extra);
        bytes.extend_from_slice(&compressed);

        central.extend_from_slice(b"PK\x01\x02");
        for value in [0x0314_u16, version, 0, 8, 0, 0] {
            central.extend_from_slice(&value.to_le_bytes());
        }
        for value in [crc.sum(), sizes[0], sizes[1]] {
            central.extend_from_slice(&value.to_le_bytes());
        }
        for value in [name_len, extra_len, 2, 0, 0] {
            central.extend_from_slice(&value.to_le_bytes());
        }
        central.extend_from_slice(&(mode << 16).to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
        central.extend_from_slice(&extra);
        central.extend_from_slice(comment);
    }
    let count = u16::try_from(entries.len()).unwrap();
    let start = u32::try_from(bytes.len()).unwrap();
    let size = u32::try_from(central.len()).unwrap();
    bytes.extend_from_slice(&central);
    if zip64 {
        let end = u64::try_from(bytes.len()).unwrap();
        bytes.extend_from_slice(b"PK\x06\x06");
        bytes.extend_from_slice(&44_u64.to_le_bytes());
        bytes.extend_from_slice(&45_u16.to_le_bytes());
        bytes.extend_from_slice(&45_u16.to_le_bytes());
        bytes.extend_from_slice(&[0; 8]);
        for value in [
            u64::from(count),
            u64::from(count),
            u64::from(size),
            u64::from(start),
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(b"PK\x06\x07");
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&end.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
    }
    bytes.extend_from_slice(b"PK\x05\x06");
    bytes.extend_from_slice(&[0; 4]);
    for _ in 0..2 {
        bytes.extend_from_slice(&(if zip64 { u16::MAX } else { count }).to_le_bytes());
    }
    for value in [size, start] {
        bytes.extend_from_slice(&(if zip64 { u32::MAX } else { value }).to_le_bytes());
    }
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(b"\x82\xff");
    [prefix, &bytes].concat()
}

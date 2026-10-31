// SQLite can silently ignore corrupt WAL frames and expose a previous login.
#[cfg(unix)]
fn opencode_valid_wal(database: &[u8], data: &[u8]) -> bool {
    if database.len() < 100 || &database[..16] != b"SQLite format 3\0" {
        return false;
    }

    if data.is_empty() {
        return true;
    }
    if data.len() < 32 {
        return false;
    }
    let big = |bytes: &[u8]| u32::from_be_bytes(bytes[..4].try_into().expect("checked WAL slice"));
    let magic = big(data);
    if !matches!(magic, 0x377f_0682 | 0x377f_0683) || big(&data[4..]) != 3_007_000 {
        return false;
    }
    let page = big(&data[8..]) as usize;
    let mut db_page = usize::from(u16::from_be_bytes([database[16], database[17]]));
    if db_page == 1 {
        db_page = 65536;
    }
    if database[18] != 2 || database[19] != 2 || db_page != page {
        return false;
    }
    if !(512..=65536).contains(&page)
        || !page.is_power_of_two()
        || !(data.len() - 32).is_multiple_of(page + 24)
    {
        return false;
    }
    let word = |bytes: &[u8]| {
        let bytes = bytes[..4].try_into().expect("checked WAL word");
        if magic == 0x377f_0682 {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    };
    let mut sums = (0_u32, 0_u32);
    let checksum = |bytes: &[u8], sums: &mut (u32, u32)| {
        for pair in bytes.as_chunks::<8>().0 {
            sums.0 = sums.0.wrapping_add(word(pair)).wrapping_add(sums.1);
            sums.1 = sums.1.wrapping_add(word(&pair[4..])).wrapping_add(sums.0);
        }
    };
    checksum(&data[..24], &mut sums);
    if sums != (big(&data[24..]), big(&data[28..])) {
        return false;
    }
    let mut committed = false;
    let mut obsolete = None;
    for frame in data[32..].chunks_exact(page + 24) {
        if big(frame) == 0 {
            return false;
        }
        // RESTART increments salt-1 and may leave the previous generation's tail.
        // Require a committed current prefix and one consistent preceding generation.
        if frame[8..16] != data[16..24] {
            if !committed || big(&frame[8..]) != big(&data[16..]).wrapping_sub(1) {
                return false;
            }
            let salts = obsolete.get_or_insert(&frame[8..16]);
            if *salts != &frame[8..16] {
                return false;
            }
            continue;
        }
        if obsolete.is_some() {
            return false;
        }
        checksum(&frame[..8], &mut sums);
        checksum(&frame[24..], &mut sums);
        if sums != (big(&frame[16..]), big(&frame[20..])) {
            return false;
        }
        committed = big(&frame[4..]) != 0;
    }
    true
}

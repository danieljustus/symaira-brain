package usage

import "encoding/binary"

// SQLite may ignore a damaged WAL and expose an older login. Reject instead.
func openCodeValidWAL(database, data []byte) bool {
	if len(database) < 100 || string(database[:16]) != "SQLite format 3\x00" {
		return false
	}

	if len(data) == 0 {
		return true
	}
	if len(data) < 32 {
		return false
	}
	magic := binary.BigEndian.Uint32(data)
	if magic != 0x377f0682 && magic != 0x377f0683 || binary.BigEndian.Uint32(data[4:]) != 3007000 {
		return false
	}
	page := int(binary.BigEndian.Uint32(data[8:]))
	dbPage := int(binary.BigEndian.Uint16(database[16:18]))
	if dbPage == 1 {
		dbPage = 65536
	}
	if database[18] != 2 || database[19] != 2 || dbPage != page {
		return false
	}
	if page < 512 || page > 65536 || page&(page-1) != 0 || (len(data)-32)%(page+24) != 0 {
		return false
	}
	var order binary.ByteOrder = binary.LittleEndian
	if magic == 0x377f0683 {
		order = binary.BigEndian
	}
	var s0, s1 uint32
	sum := func(b []byte) {
		for len(b) > 0 {
			s0 += order.Uint32(b) + s1
			s1 += order.Uint32(b[4:]) + s0
			b = b[8:]
		}
	}
	sum(data[:24])
	if s0 != binary.BigEndian.Uint32(data[24:]) || s1 != binary.BigEndian.Uint32(data[28:]) {
		return false
	}
	committed := false
	var obsolete []byte
	for offset := 32; offset < len(data); offset += page + 24 {
		frame := data[offset : offset+page+24]
		if binary.BigEndian.Uint32(frame) == 0 {
			return false
		}
		// RESTART increments salt-1 and may leave the previous generation's tail.
		// Require a committed current prefix and one consistent preceding generation.
		if string(frame[8:16]) != string(data[16:24]) {
			if !committed || binary.BigEndian.Uint32(frame[8:]) != binary.BigEndian.Uint32(data[16:])-1 {
				return false
			}
			if obsolete == nil {
				obsolete = frame[8:16]
			}
			if string(frame[8:16]) != string(obsolete) {
				return false
			}
			continue
		}
		if obsolete != nil {
			return false
		}
		sum(frame[:8])
		sum(frame[24:])
		if s0 != binary.BigEndian.Uint32(frame[16:]) || s1 != binary.BigEndian.Uint32(frame[20:]) {
			return false
		}
		committed = binary.BigEndian.Uint32(frame[4:]) != 0
	}
	return true
}

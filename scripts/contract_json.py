"""Byte-span helper shared by the accepted-divergence contracts."""


def value_end(data: bytes, start: int) -> int:
    """Index just past the JSON object/array opening at data[start] (string/escape aware)."""
    depth, in_string, escaped = 0, False, False
    for index in range(start, len(data)):
        byte = data[index : index + 1]
        if in_string:
            if escaped:
                escaped = False
            elif byte == b"\\":
                escaped = True
            elif byte == b'"':
                in_string = False
        elif byte == b'"':
            in_string = True
        elif byte in (b"{", b"["):
            depth += 1
        elif byte in (b"}", b"]"):
            depth -= 1
            if depth == 0:
                return index + 1
    raise AssertionError("unterminated report section")

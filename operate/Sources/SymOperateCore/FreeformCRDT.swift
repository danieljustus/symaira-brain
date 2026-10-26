// Copyright 2026 Soso Song
// Adapted from soso-song/freeform-to-markdown, src/freeform_to_markdown/parsing.py.
// Apache-2.0: https://github.com/soso-song/freeform-to-markdown (see SOURCE_PROVENANCE.md).
// Modified: Swift port, strict CRDT v6 header, bounded traversal and first-line-only text.
import Foundation

/// Defensive parsing of the observed CRDT v6 / protobuf fragments, not a full Freeform schema.
public enum FreeformCRDT {
    public static func readVarint(_ bytes: [UInt8], offset: inout Int) throws -> UInt64 {
        var value: UInt64 = 0
        for index in 0..<10 {
            guard offset < bytes.count else { throw invalid("Truncated varint") }
            let byte = bytes[offset]
            offset += 1
            guard index != 9 || byte <= 1 else { throw invalid("Varint exceeds 64 bits") }
            value |= UInt64(byte & 0x7F) << (index * 7)
            if byte & 0x80 == 0 { return value }
        }
        throw invalid("Varint exceeds 64 bits")
    }

    public static func geometry(_ data: Data) throws -> RectValue? {
        let bytes = try body(data)
        var pairs: [(Double, Double)] = []
        guard bytes.count >= 12 else { return nil }
        for index in 0...(bytes.count - 12) where Array(bytes[index..<(index + 3)]) == [0x22, 0x0A, 0x0D] && bytes[index + 7] == 0x15 {
            func float(at offset: Int) -> Double {
                let bits = (0..<4).reduce(UInt32(0)) { $0 | UInt32(bytes[offset + $1]) << ($1 * 8) }
                return Double(Float(bitPattern: bits))
            }
            pairs.append((float(at: index + 3), float(at: index + 8)))
            if pairs.count == 2 { break }
        }
        guard pairs.count == 2 else { return nil }
        let values = [pairs[0].0, pairs[0].1, pairs[1].0, pairs[1].1]
        guard values.allSatisfy(\.isFinite), values[2] > 0, values[3] > 0 else { return nil }
        return RectValue(x: values[0], y: values[1], width: values[2], height: values[3])
    }

    public static func text(_ data: Data) throws -> String {
        let strings = try leaves(try body(data), depth: 0)
        let styles: Set<String> = [
            "basewritingdirection", "bold", "characterfill", "crdt", "fontname", "fontsize", "italic", "left", "link",
            "listend", "listlevel", "liststart", "liststyle", "listtype", "paragraphalignment", "paragraphlevel", "regular",
            "semibold", "strikethrough", "textcolor", "underline", "verticalalignment",
        ]
        for string in strings {
            // Freeform may append attribute fragments after a newline. Never join them into user prose.
            let value = String(string.prefix { !$0.isNewline }).trimmingCharacters(in: .whitespacesAndNewlines)
            guard !value.isEmpty, !styles.contains(value.lowercased()),
                  !value.hasPrefix("com.apple"), !value.hasPrefix("&com"), !value.hasPrefix("public."),
                  !value.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }),
                  value.unicodeScalars.contains(where: { CharacterSet.alphanumerics.contains($0) }) else { continue }
            return value
        }
        return ""
    }

    private static func body(_ data: Data) throws -> [UInt8] {
        guard data.count <= 16 * 1024 * 1024 else { throw invalid("CRDT blob exceeds limit") }
        let bytes = Array(data)
        if bytes.isEmpty { return [] }
        guard bytes.count >= 8, Array(bytes.prefix(8)) == [0x63, 0x72, 0x64, 0x74, 6, 0, 0, 0] else {
            throw invalid("Unsupported CRDT header; expected crdt + uint32 LE 6")
        }
        return Array(bytes.dropFirst(8))
    }

    private static func leaves(_ bytes: [UInt8], depth: Int) throws -> [String] {
        guard depth <= 8 else { return [] }
        var offset = 0
        var result: [String] = []
        while offset < bytes.count {
            let tag = try readVarint(bytes, offset: &offset)
            guard tag >> 3 > 0 else { throw invalid("Invalid protobuf field") }
            switch tag & 7 {
            case 0: _ = try readVarint(bytes, offset: &offset)
            case 1: try skip(8, bytes: bytes, offset: &offset)
            case 5: try skip(4, bytes: bytes, offset: &offset)
            case 2:
                let length = try readVarint(bytes, offset: &offset)
                guard length <= UInt64(bytes.count - offset) else { throw invalid("Truncated protobuf field") }
                let chunk = Array(bytes[offset..<(offset + Int(length))])
                offset += Int(length)
                if chunk.starts(with: [0x63, 0x72, 0x64, 0x74]) {
                    result += try leaves(body(Data(chunk)), depth: depth + 1)
                } else if !chunk.isEmpty, let nested = try? leaves(chunk, depth: depth + 1) {
                    result += nested
                } else if let string = String(bytes: chunk, encoding: .utf8) {
                    result.append(string)
                }
            default: throw invalid("Unsupported protobuf wire type")
            }
        }
        return result
    }

    private static func skip(_ count: Int, bytes: [UInt8], offset: inout Int) throws {
        guard count <= bytes.count - offset else { throw invalid("Truncated protobuf fixed field") }
        offset += count
    }

    private static func invalid(_ message: String) -> AutomationError { .invalidArgument(message) }
}

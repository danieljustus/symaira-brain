import XCTest
@testable import SymOperateCore

func freeformTestBoard() -> FreeformBoardIR {
    let first = FreeformObject(id: UUID(uuidString: "11111111-1111-1111-1111-111111111111")!, text: "Alpha",
                               x: 10, y: 20, width: 140, height: 70, rgb: [1, 0.5, 0])
    let second = FreeformObject(id: UUID(uuidString: "22222222-2222-2222-2222-222222222222")!, text: "Beta",
                                x: 300, y: 200, width: 120, height: 80, rgb: [0, 0.5, 1])
    return FreeformBoardIR(objects: [first, second], connections: [FreeformConnection(tail: first.id, head: second.id)])
}

final class FreeformContentLanguageTests: XCTestCase {
    func testReferenceShapeSchemaAndRGB() throws {
        let payload = try JSONSerialization.jsonObject(with: FreeformContentLanguage.build(freeformTestBoard())) as! [[String: Any]]
        let shape = payload[0]
        XCTAssertEqual(shape["type_identifier"] as? String, "com.apple.apps.content-language.shape")
        XCTAssertEqual(shape["version"] as? String, "1.0")
        XCTAssertEqual(shape["head"] as? String, "none")
        XCTAssertEqual(shape["tail"] as? String, "none")
        XCTAssertEqual(shape["stroke"] as? String, "empty")
        let geometry = shape["geometry"] as! [String: Any]
        XCTAssertEqual((geometry["position"] as! [String: Any])["x"] as? Double, 10)
        XCTAssertEqual((geometry["size"] as! [String: Any])["height"] as? Double, 70)
        let text = shape["text"] as! [String: Any]
        XCTAssertEqual(text["primary_case"] as? String, "attributed_string")
        let attributed = text["attributed_string"] as! [Any]
        XCTAssertEqual(attributed[0] as? String, "Alpha")
        XCTAssertEqual(Set((attributed[1] as! [String: Any]).keys), ["NSFont", "NSColor", "NSParagraphStyle"])
        let color = (shape["fill"] as! [String: Any])["color"] as! [String: Any]
        let rgba = color["rgba"] as! [String: Any]
        XCTAssertEqual(rgba["version"] as? String, "1.1")
        XCTAssertEqual(rgba["color_space"] as? String, "srgb")
        XCTAssertEqual(rgba["red"] as? Double, 1)
        XCTAssertEqual(rgba["green"] as? Double, 0.5)
        XCTAssertEqual(rgba["blue"] as? Double, 0)
    }

    func testReferenceConnectionSchemaAndConsistentAnchorIDs() throws {
        let board = freeformTestBoard()
        let payload = try JSONSerialization.jsonObject(with: FreeformContentLanguage.build(board)) as! [[String: Any]]
        let line = payload[2]
        XCTAssertEqual(line["type_identifier"] as? String, "com.apple.apps.content-language.connection-line")
        for (key, index) in [("tail", 0), ("head", 1)] {
            let end = line[key] as! [String: Any]
            let anchor = end["anchor"] as! [String: Any]
            XCTAssertEqual(end["line_end"] as? String, "none")
            XCTAssertEqual(anchor["object_id"] as? String, payload[index]["identifier"] as? String)
            XCTAssertEqual(anchor["magnet"] as? String, "center")
            XCTAssertEqual(anchor["version"] as? String, "1.0")
            XCTAssertEqual(anchor["type_identifier"] as? String, "com.apple.apps.content-language.connection-line.end.anchor")
        }
        let tail = (line["tail"] as! [String: Any])["end_point"] as! [String: Any]
        XCTAssertEqual(tail["x"] as? Double, 80)
        XCTAssertEqual(tail["y"] as? Double, 55)
        XCTAssertEqual((line["line_type"] as! [String: Any])["primary_case"] as? String, "curved")
        XCTAssertEqual(try FreeformContentLanguage.build(board), try FreeformContentLanguage.build(board))
    }

    func testStickyNoteAndTextUseTheirActualExportVariants() throws {
        let objects = [FreeformObject(kind: .stickyNote, text: "Note", x: 0, y: 0, width: 200, height: 200, rgb: [1, 1, 0]),
                       FreeformObject(kind: .text, text: "Text", x: 0, y: 0, width: 100, height: 30, rgb: [0, 0, 0])]
        let payload = try JSONSerialization.jsonObject(with: FreeformContentLanguage.build(FreeformBoardIR(objects: objects))) as! [[String: Any]]
        XCTAssertNotNil(payload[0]["shadow"])
        XCTAssertNil(payload[1]["fill"])
        XCTAssertEqual((payload[1]["geometry"] as! [String: Any])["width_valid"] as? Bool, false)
    }

    func testArrowAndAllUnknownConnectionEnumsAreRejected() throws {
        var board = freeformTestBoard()
        for connection in [
            FreeformConnection(tail: board.objects[0].id, head: board.objects[1].id, lineEnd: "arrow"),
            FreeformConnection(tail: board.objects[0].id, head: board.objects[1].id, lineType: "straight"),
            FreeformConnection(tail: board.objects[0].id, head: board.objects[1].id, pattern: "dashed"),
            FreeformConnection(tail: board.objects[0].id, head: board.objects[1].id, magnet: "left"),
        ] {
            board.connections = [connection]
            XCTAssertThrowsError(try FreeformContentLanguage.build(board))
        }
    }

    func testFinalPayloadEnumValidationFailsClosed() {
        for value: [String: Any] in [["line_end": "arrow"], ["primary_case": "unknown"], ["color_space": "p3"],
                                    ["head": "arrow"], ["stroke": "solid"], ["magnet": 4]] {
            XCTAssertThrowsError(try FreeformContentLanguage.validateEnums([value]))
        }
    }

    func testDanglingDuplicateAndSelfAnchorsAreRejected() {
        var board = freeformTestBoard()
        board.connections = [FreeformConnection(tail: board.objects[0].id, head: UUID())]
        XCTAssertThrowsError(try FreeformContentLanguage.build(board))
        board.connections = [FreeformConnection(tail: board.objects[0].id, head: board.objects[0].id)]
        XCTAssertThrowsError(try FreeformContentLanguage.build(board))
        board.objects.append(board.objects[0])
        XCTAssertThrowsError(try FreeformContentLanguage.build(board))
    }

    func testInvalidGeometryRGBAndKindAreRejected() {
        for (width, rgb) in [(0.0, [0.0, 0, 0]), (1, [1.1, 0, 0]), (1, [0]), (Double.nan, [0, 0, 0])] {
            let item = FreeformObject(text: "A", x: 0, y: 0, width: width, height: 1, rgb: rgb)
            XCTAssertThrowsError(try FreeformContentLanguage.build(FreeformBoardIR(objects: [item])))
        }
        XCTAssertThrowsError(try JSONDecoder().decode(FreeformBoardIR.self, from: Data(#"{"objects":[{"kind":"unknown"}]}"#.utf8)))
    }

    private func tempFile(_ bytes: [UInt8]) throws -> String {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try Data(bytes).write(to: url)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url.path
    }

    /// Mirrors the Freeform 5.1 image export: payload references a resource type carrying the raw bytes.
    func testImageTravelsAsIndirectResourceOnTheSameItem() throws {
        let png: [UInt8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3]
        let image = FreeformObject(kind: .image, text: "Logo", x: 5, y: 6, width: 160, height: 90,
                                   imagePath: try tempFile(png))
        let item = try FreeformContentLanguage.pasteboardItem(FreeformBoardIR(objects: [image]))
        let payload = try JSONSerialization.jsonObject(with: item[FreeformContentLanguage.pasteboardType]!) as! [[String: Any]]
        XCTAssertEqual(payload[0]["type_identifier"] as? String, "com.apple.apps.content-language.image")
        XCTAssertEqual(payload[0]["description"] as? String, "Logo")
        XCTAssertNil(payload[0]["fill"])
        let indirect = (payload[0]["resource"] as! [String: Any])["indirect"] as! [String: Any]
        let type = indirect["identifier"] as! String
        XCTAssertTrue(type.hasPrefix("com.apple.apps.content-language.resource-"))
        XCTAssertEqual(indirect["filename"] as? String, "image.png")
        XCTAssertEqual(item[type], Data(png))
        XCTAssertEqual(item.count, 2)
    }

    func testImageRequiresAbsolutePNGOrJPEGPathAndOnlyForImages() throws {
        let gif = try tempFile(Array("GIF89a".utf8))
        for path in [gif, "relative.png", "/nonexistent/x.png"] {
            let image = FreeformObject(kind: .image, text: "", x: 0, y: 0, width: 1, height: 1, imagePath: path)
            XCTAssertThrowsError(try FreeformContentLanguage.build(FreeformBoardIR(objects: [image])))
        }
        let jpeg = try tempFile([0xFF, 0xD8, 0xFF, 0xE0])
        XCTAssertThrowsError(try FreeformContentLanguage.build(FreeformBoardIR(objects: [
            FreeformObject(kind: .image, text: "", x: 0, y: 0, width: 1, height: 1)])))
        XCTAssertThrowsError(try FreeformContentLanguage.build(FreeformBoardIR(objects: [
            FreeformObject(text: "", x: 0, y: 0, width: 1, height: 1, rgb: [0, 0, 0], imagePath: jpeg)])))
        XCTAssertNoThrow(try FreeformContentLanguage.build(FreeformBoardIR(objects: [
            FreeformObject(kind: .image, text: "", x: 0, y: 0, width: 1, height: 1, imagePath: jpeg)])))
    }
}

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
}

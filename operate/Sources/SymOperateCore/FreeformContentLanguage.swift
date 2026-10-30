import Foundation

// swiftlint:disable identifier_name
// Board x/y intentionally match the existing PointValue API and the IR wire schema.

/// Small, explicit board IR. Coordinates are board points; RGB components are 0...1.
public struct FreeformBoardIR: Codable, Sendable {
    public var objects: [FreeformObject]
    public var connections: [FreeformConnection]

    public init(objects: [FreeformObject], connections: [FreeformConnection] = []) {
        self.objects = objects
        self.connections = connections
    }

    enum CodingKeys: String, CodingKey { case objects, connections }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        objects = try values.decode([FreeformObject].self, forKey: .objects)
        connections = try values.decodeIfPresent([FreeformConnection].self, forKey: .connections) ?? []
    }
}

public struct FreeformObject: Codable, Sendable {
    public enum Kind: String, Codable, Sendable { case shape, text, stickyNote = "sticky_note", image }
    public let id: UUID
    public let kind: Kind
    /// For images this becomes the image description, which Freeform exposes as the AX label.
    public let text: String
    public let x: Double
    public let y: Double
    public let width: Double
    public let height: Double
    /// Fill for shapes and notes; ignored for text and images.
    public let rgb: [Double]
    /// Local PNG or JPEG file; required for `image`, rejected for every other kind.
    public let imagePath: String?

    enum CodingKeys: String, CodingKey {
        case id, kind, text, x, y, width, height, rgb
        case imagePath = "image_path"
    }

    public init(id: UUID = UUID(), kind: Kind = .shape, text: String, x: Double, y: Double,
                width: Double, height: Double, rgb: [Double] = [], imagePath: String? = nil) {
        self.id = id
        self.kind = kind
        self.text = text
        self.x = x
        self.y = y
        self.width = width
        self.height = height
        self.rgb = rgb
        self.imagePath = imagePath
    }

    public init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        id = try values.decode(UUID.self, forKey: .id)
        kind = try values.decode(Kind.self, forKey: .kind)
        text = try values.decode(String.self, forKey: .text)
        x = try values.decode(Double.self, forKey: .x)
        y = try values.decode(Double.self, forKey: .y)
        width = try values.decode(Double.self, forKey: .width)
        height = try values.decode(Double.self, forKey: .height)
        rgb = try values.decodeIfPresent([Double].self, forKey: .rgb) ?? []
        imagePath = try values.decodeIfPresent(String.self, forKey: .imagePath)
    }
}

public struct FreeformConnection: Codable, Sendable {
    public let tail: UUID
    public let head: UUID
    public let lineEnd: String?
    public let lineType: String?
    public let pattern: String?
    public let magnet: String?

    enum CodingKeys: String, CodingKey {
        case tail, head, pattern, magnet
        case lineEnd = "line_end", lineType = "line_type"
    }

    public init(tail: UUID, head: UUID, lineEnd: String? = nil, lineType: String? = nil,
                pattern: String? = nil, magnet: String? = nil) {
        self.tail = tail
        self.head = head
        self.lineEnd = lineEnd
        self.lineType = lineType
        self.pattern = pattern
        self.magnet = magnet
    }
}

/// Schema copied from Freeform 5.0 exports ref1/ref-line, with anchors from live-tested s6b.
/// The image template is a Freeform 5.1 export (macOS 27.2): image bytes travel as a separate
/// pasteboard type named by `resource.indirect.identifier` on the same pasteboard item.
/// Templates contain only formatting metadata, never exported board content or identities.
public enum FreeformContentLanguage {
    public static let pasteboardType = "com.apple.apps.content-language.canvas-object-1.0"
    static let prefix = "com.apple.apps.content-language."
    static let maxImageBytes = 25 * 1024 * 1024

    /// Canvas payload only. Image resources are loaded and validated but not returned.
    public static func build(_ board: FreeformBoardIR) throws -> Data {
        try pasteboardItem(board)[pasteboardType]!
    }

    /// All types for one pasteboard item: the canvas payload plus one entry per image resource.
    public static func pasteboardItem(_ board: FreeformBoardIR) throws -> [String: Data] {
        guard !board.objects.isEmpty, board.objects.count <= 500, board.connections.count <= 1000 else {
            throw AutomationError.invalidArgument("Expected 1...500 objects and at most 1000 connections.")
        }
        let ids = Set(board.objects.map(\.id))
        guard ids.count == board.objects.count else {
            throw AutomationError.invalidArgument("Object UUIDs must be unique.")
        }
        var resources: [UUID: (type: String, data: Data, ext: String)] = [:]
        for object in board.objects {
            let isImage = object.kind == .image
            guard [object.x, object.y, object.width, object.height].allSatisfy(\.isFinite),
                  object.width > 0, object.height > 0, isImage || object.rgb.count == 3,
                  object.rgb.allSatisfy({ $0.isFinite && (0...1).contains($0) }),
                  object.text.utf8.count <= 100_000, isImage == (object.imagePath != nil) else {
                throw AutomationError.invalidArgument("Invalid object geometry, text size, RGB fill or image_path.")
            }
            if isImage, let path = object.imagePath {
                let image = try loadImage(path)
                resources[object.id] = (prefix + "resource-" + UUID().uuidString, image.data, image.ext)
            }
        }
        guard resources.values.reduce(0, { $0 + $1.data.count }) <= 4 * maxImageBytes else {
            throw AutomationError.invalidArgument("Images exceed 100 MB in total.")
        }
        for line in board.connections {
            guard ids.contains(line.tail), ids.contains(line.head), line.tail != line.head,
                  line.lineEnd == nil || line.lineEnd == "none",
                  line.lineType == nil || line.lineType == "curved",
                  line.pattern == nil || line.pattern == "solid",
                  line.magnet == nil || line.magnet == "center" else {
                throw AutomationError.invalidArgument("Unknown connection enum or endpoint. Only none/curved/solid/center are evidenced.")
            }
        }
        let templates = try template("freeform-shapes")
        let lineTemplate = try template("freeform-line")[0]
        let imageTemplate = try template("freeform-image")[0]
        var payload = board.objects.map { object -> [String: Any] in
            if let resource = resources[object.id] {
                var image = imageTemplate
                image["identifier"] = object.id.uuidString
                image["description"] = object.text
                image["geometry"] = geometry(image["geometry"], object)
                var wrapper = image["resource"] as? [String: Any] ?? [:]
                var indirect = wrapper["indirect"] as? [String: Any] ?? [:]
                indirect["identifier"] = resource.type
                indirect["filename"] = "image." + resource.ext
                wrapper["indirect"] = indirect
                image["resource"] = wrapper
                return image
            }
            let index = object.kind == .stickyNote ? 1 : (object.kind == .text ? 2 : 0)
            var shape = templates[index]
            shape["identifier"] = object.id.uuidString
            shape["geometry"] = geometry(shape["geometry"], object)
            if object.kind != .text {
                shape["fill"] = typed("fill", ["primary_case": "color", "color": color(object.rgb)])
            }
            var text = shape["text"] as? [String: Any] ?? [:]
            let attributes = (text["attributed_string"] as? [Any])?.last as? [String: Any] ?? [:]
            text["attributed_string"] = [object.text, attributes]
            shape["text"] = text
            return shape
        }
        for connection in board.connections {
            // Endpoint validation above makes these lookups total.
            guard let tail = board.objects.first(where: { $0.id == connection.tail }),
                  let head = board.objects.first(where: { $0.id == connection.head }) else {
                throw AutomationError.invalidArgument("Missing connection endpoint.")
            }
            var line = lineTemplate
            line["tail"] = end(tail)
            line["head"] = end(head)
            line["line_type"] = typed("connection-line.line-type", [
                "primary_case": "curved",
                "curved": typed("position", ["x": (tail.x + tail.width / 2 + head.x + head.width / 2) / 2,
                                               "y": (tail.y + tail.height / 2 + head.y + head.height / 2) / 2]),
            ])
            payload.append(line)
        }
        // Validate the final payload too: an accidental template edit must never reach Freeform.
        try validateEnums(payload)
        var item = [pasteboardType: try JSONSerialization.data(withJSONObject: payload, options: [.sortedKeys])]
        for resource in resources.values { item[resource.type] = resource.data }
        return item
    }

    /// One unanchored line, used as the starting point for connecting objects already on the board.
    public static func freeLine() throws -> [String: Data] {
        var line = try template("freeform-line")[0]
        func end(_ x: Double) -> [String: Any] {
            typed("connection-line.end", ["end_point": typed("position", ["x": x, "y": 0]), "outset": 0, "line_end": "none"])
        }
        line["tail"] = end(0)
        line["head"] = end(100)
        line["line_type"] = typed("connection-line.line-type", [
            "primary_case": "curved", "curved": typed("position", ["x": 50, "y": 0]),
        ])
        try validateEnums([line])
        return [pasteboardType: try JSONSerialization.data(withJSONObject: [line], options: [.sortedKeys])]
    }

    private static func geometry(_ template: Any?, _ object: FreeformObject) -> [String: Any] {
        var geometry = template as? [String: Any] ?? [:]
        geometry["position"] = typed("position", ["x": object.x, "y": object.y])
        geometry["size"] = typed("size", ["width": object.width, "height": object.height])
        return geometry
    }

    /// Only PNG and JPEG, identified by magic bytes; the file extension is not trusted.
    static func loadImage(_ path: String) throws -> (data: Data, ext: String) {
        let expanded = (path as NSString).expandingTildeInPath
        let url = URL(fileURLWithPath: expanded)
        guard (expanded as NSString).isAbsolutePath,
              let size = (try? url.resourceValues(forKeys: [.fileSizeKey, .isRegularFileKey])),
              size.isRegularFile == true, let bytes = size.fileSize, bytes > 0, bytes <= maxImageBytes,
              let data = try? Data(contentsOf: url) else {
            throw AutomationError.invalidArgument("image_path must be an absolute path to a readable file of at most 25 MB: \(path)")
        }
        if data.starts(with: [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) { return (data, "png") }
        if data.starts(with: [0xFF, 0xD8, 0xFF]) { return (data, "jpg") }
        throw AutomationError.invalidArgument("image_path must be a PNG or JPEG file: \(path)")
    }

    private static func template(_ name: String) throws -> [[String: Any]] {
        guard let url = Bundle.module.url(forResource: name, withExtension: "json"),
              let value = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as? [[String: Any]],
              !value.isEmpty else {
            throw AutomationError.unavailable("Missing Freeform reference schema.")
        }
        return value
    }

    private static func typed(_ suffix: String, _ fields: [String: Any]) -> [String: Any] {
        fields.merging(["version": "1.0", "type_identifier": prefix + suffix]) { _, new in new }
    }

    private static func color(_ rgb: [Double]) -> [String: Any] {
        typed("color", ["primary_case": "rgba", "rgba": [
            "type_identifier": prefix + "color.rgba", "version": "1.1", "color_space": "srgb",
            "red": rgb[0], "green": rgb[1], "blue": rgb[2], "alpha": 1, "headroom": 1,
        ]])
    }

    private static func end(_ object: FreeformObject) -> [String: Any] {
        typed("connection-line.end", [
            "end_point": typed("position", ["x": object.x + object.width / 2, "y": object.y + object.height / 2]),
            "outset": 0, "line_end": "none",
            "anchor": typed("connection-line.end.anchor", ["object_id": object.id.uuidString, "magnet": "center"]),
        ])
    }

    static func validateEnums(_ value: Any) throws {
        if let array = value as? [Any] {
            for child in array { try validateEnums(child) }
        } else if let object = value as? [String: Any] {
            let enums: [String: Set<String>] = [
                "line_end": ["none"], "magnet": ["center"], "pattern": ["solid"],
                "color_space": ["srgb"], "vertical_alignment": ["center"],
                "primary_case": ["color", "rgba", "attributed_string", "line", "curved", "curvedShadow",
                                 "indirect", "dropShadow"],
            ]
            for (key, child) in object {
                if let allowed = enums[key] {
                    guard let string = child as? String, allowed.contains(string) else {
                        throw AutomationError.invalidArgument("Unsupported Content-Language enum: \(key).")
                    }
                }
                if (key == "head" || key == "tail" || key == "stroke"), let string = child as? String,
                   string != (key == "stroke" ? "empty" : "none") {
                    throw AutomationError.invalidArgument("Unsupported Content-Language enum: \(key).")
                }
                try validateEnums(child)
            }
        }
    }
}

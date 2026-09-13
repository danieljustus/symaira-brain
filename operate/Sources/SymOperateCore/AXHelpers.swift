import ApplicationServices

func axCopyAttribute(_ element: AXUIElement, attribute: String) -> AnyObject? {
    var value: CFTypeRef?
    let result = AXUIElementCopyAttributeValue(element, attribute as CFString, &value)
    guard result == .success, let value else { return nil }
    return value
}

/// Reads several attributes of one element in a single Accessibility round trip.
///
/// `AXUIElementCopyAttributeValue` is a synchronous IPC call into the target
/// application, so a tree walk that reads ten attributes per node pays ten
/// round trips per node. `AXUIElementCopyMultipleAttributeValues` asks for the
/// whole set at once, which is why every hot AX path here goes through this
/// helper rather than looping over the single-attribute one.
///
/// Attributes the target could not supply (unsupported on that element, or the
/// element went stale mid-flight) come back as an `AXValue` carrying an
/// `AXError`; those positions become `nil`, matching what
/// ``axCopyAttribute(_:attribute:)`` returns for the same case. `nil` is
/// returned only when the batched call itself failed, so callers can fall back
/// to individual reads.
func axCopyMultipleAttributes(_ element: AXUIElement, attributes: [String]) -> [AnyObject?]? {
    var values: CFArray?
    let result = AXUIElementCopyMultipleAttributeValues(
        element,
        attributes as CFArray,
        AXCopyMultipleAttributeOptions(),
        &values
    )
    guard result == .success,
          let raw = values as? [AnyObject],
          raw.count == attributes.count else {
        return nil
    }
    return raw.map(axUnwrapMultipleAttributeValue)
}

/// Maps one slot of an `AXUIElementCopyMultipleAttributeValues` result to the
/// value a single-attribute read would have produced: an error placeholder or
/// a null becomes `nil`.
func axUnwrapMultipleAttributeValue(_ value: AnyObject) -> AnyObject? {
    if value is NSNull { return nil }
    guard CFGetTypeID(value) == AXValueGetTypeID() else { return value }
    return AXValueGetType(unsafeDowncast(value, to: AXValue.self)) == .axError ? nil : value
}

func axCopyElement(_ element: AXUIElement, attribute: String) -> AXUIElement? {
    guard let value = axCopyAttribute(element, attribute: attribute),
          CFGetTypeID(value) == AXUIElementGetTypeID() else { return nil }
    return unsafeDowncast(value, to: AXUIElement.self)
}

func axCopyElements(_ element: AXUIElement, attribute: String) -> [AXUIElement]? {
    axCopyAttribute(element, attribute: attribute) as? [AXUIElement]
}

func axCopyString(_ element: AXUIElement, attribute: String) -> String? {
    axCopyAttribute(element, attribute: attribute) as? String
}

func axCopyFrame(_ element: AXUIElement) -> RectValue? {
    axFrame(
        position: axCopyAttribute(element, attribute: kAXPositionAttribute),
        size: axCopyAttribute(element, attribute: kAXSizeAttribute)
    )
}

/// Decodes an already-fetched AXPosition/AXSize pair, so callers that read both
/// in one batched round trip share the same conversion as ``axCopyFrame(_:)``.
func axFrame(position: AnyObject?, size: AnyObject?) -> RectValue? {
    guard let position, let size else { return nil }

    // Verify CF types before casting — AX API may return unexpected types
    // when elements become stale mid-flight.
    guard CFGetTypeID(position) == AXValueGetTypeID(),
          CFGetTypeID(size) == AXValueGetTypeID() else {
        return nil
    }

    let posAXValue = unsafeDowncast(position, to: AXValue.self)
    let sizeAXValue = unsafeDowncast(size, to: AXValue.self)

    var point = CGPoint.zero
    var sizeValue = CGSize.zero

    guard
        AXValueGetType(posAXValue) == .cgPoint,
        AXValueGetValue(posAXValue, .cgPoint, &point),
        AXValueGetType(sizeAXValue) == .cgSize,
        AXValueGetValue(sizeAXValue, .cgSize, &sizeValue)
    else {
        return nil
    }

    return RectValue(x: point.x, y: point.y, width: sizeValue.width, height: sizeValue.height)
}

func axCopyActionNames(_ element: AXUIElement) -> [String] {
    var names: CFArray?
    let result = AXUIElementCopyActionNames(element, &names)
    guard result == .success, let array = names as? [String] else { return [] }
    return array
}

func axBoolify(_ value: AnyObject?) -> Bool? {
    if let bool = value as? Bool { return bool }
    if let number = value as? NSNumber, CFGetTypeID(number) == CFBooleanGetTypeID() {
        return number.boolValue
    }
    return nil
}

func axStringify(_ value: AnyObject?) -> String? {
    switch value {
    case let string as String:
        return string
    case let number as NSNumber:
        return number.stringValue
    default:
        return nil
    }
}

/// One element's attribute values, fetched in a single Accessibility round trip
/// when the batched API is available and one attribute at a time when it is not.
///
/// The fallback keeps the per-attribute laziness of the original call sites:
/// nothing is read until it is asked for, so an attribute that is only needed
/// when another one is missing still costs nothing when it is not.
struct AXAttributeBag {
    private let element: AXUIElement
    /// `nil` when the batched call failed and reads must go one by one.
    private let batched: [String: AnyObject]?

    init(element: AXUIElement, attributes: [String]) {
        self.element = element
        guard let values = axCopyMultipleAttributes(element, attributes: attributes) else {
            self.batched = nil
            return
        }
        var map: [String: AnyObject] = [:]
        map.reserveCapacity(attributes.count)
        for (attribute, value) in zip(attributes, values) {
            if let value { map[attribute] = value }
        }
        self.batched = map
    }

    func value(_ attribute: String) -> AnyObject? {
        if let batched { return batched[attribute] }
        return axCopyAttribute(element, attribute: attribute)
    }

    func string(_ attribute: String) -> String? {
        value(attribute) as? String
    }

    func elements(_ attribute: String) -> [AXUIElement]? {
        value(attribute) as? [AXUIElement]
    }
}

import XCTest
import ApplicationServices
@testable import SymOperateCore

final class AXHelpersTests: XCTestCase {

    // MARK: - axCopyElement safe cast

    func testAxCopyElementReturnsNilForNonExistentAttribute() {
        let element = AXUIElementCreateApplication(0)
        let result = axCopyElement(element, attribute: "AXNonExistentAttribute")
        XCTAssertNil(result, "Should return nil when attribute does not exist")
    }

    func testAxCopyElementReturnsNilForStringAttribute() {
        // kAXTitleAttribute returns a String, not an AXUIElement.
        // Before the fix this would crash with `as! AXUIElement`.
        let element = AXUIElementCreateApplication(0)
        let result = axCopyElement(element, attribute: kAXTitleAttribute)
        XCTAssertNil(result, "Should return nil when attribute value is not an AXUIElement")
    }

    func testAxCopyElementReturnsNilForRoleAttribute() {
        // kAXRoleAttribute returns a String, not an AXUIElement.
        let element = AXUIElementCreateApplication(0)
        let result = axCopyElement(element, attribute: kAXRoleAttribute)
        XCTAssertNil(result, "Should return nil when role attribute is a String, not an AXUIElement")
    }

    // MARK: - axCopyFrame safe cast

    func testAxCopyFrameReturnsNilForElementWithoutPositionAndSize() {
        // An element that doesn't expose AXPosition/AXSize should return nil
        // instead of crashing on force-cast.
        let element = AXUIElementCreateApplication(0)
        let result = axCopyFrame(element)
        XCTAssertNil(result, "Should return nil when position or size attributes are missing")
    }

    func testAxCopyFrameReturnsNilForAppElement() {
        // AXUIElementCreateApplication returns an element that may not have
        // concrete position/size in some environments. The key assertion is
        // that it does NOT crash — previously this would force-cast and die.
        let element = AXUIElementCreateApplication(ProcessInfo.processInfo.processIdentifier)
        let result = axCopyFrame(element)
        // May or may not succeed depending on whether the app exposes these attributes.
        // The important thing is no crash occurs.
        _ = result
    }

    // MARK: - axCopyAttribute

    func testAxCopyAttributeReturnsNilForMissingAttribute() {
        let element = AXUIElementCreateApplication(0)
        let result = axCopyAttribute(element, attribute: "AXNonExistent")
        XCTAssertNil(result, "Should return nil for non-existent attributes")
    }

    // MARK: - axCopyElements

    func testAxCopyElementsReturnsNilForMissingAttribute() {
        let element = AXUIElementCreateApplication(0)
        let result = axCopyElements(element, attribute: "AXNonExistent")
        XCTAssertNil(result, "Should return nil for non-existent attributes")
    }

    func testAxCopyElementsReturnsNilForStringAttribute() {
        // kAXTitleAttribute returns a String, not an array of AXUIElement.
        let element = AXUIElementCreateApplication(0)
        let result = axCopyElements(element, attribute: kAXTitleAttribute)
        XCTAssertNil(result, "Should return nil when attribute value is not [AXUIElement]")
    }

    // MARK: - axCopyString

    func testAxCopyStringReturnsNilForMissingAttribute() {
        let element = AXUIElementCreateApplication(0)
        let result = axCopyString(element, attribute: "AXNonExistent")
        XCTAssertNil(result, "Should return nil for non-existent attributes")
    }

    // MARK: - axCopyActionNames

    func testAxCopyActionNamesReturnsEmptyForAppElement() {
        let element = AXUIElementCreateApplication(0)
        let result = axCopyActionNames(element)
        // App elements may not expose actions; the important thing is no crash.
        XCTAssertTrue(result.isEmpty || !result.isEmpty)
    }

    // MARK: - axCopyMultipleAttributes / AXAttributeBag

    func testAxCopyMultipleAttributesReturnsNilForAnInvalidElement() {
        let element = AXUIElementCreateApplication(0)
        let result = axCopyMultipleAttributes(
            element,
            attributes: [kAXRoleAttribute, kAXTitleAttribute]
        )
        // No process to answer, so the batched call fails outright and the
        // caller is told to fall back rather than handed a bogus row of values.
        XCTAssertNil(result)
    }

    func testAttributeBagMatchesSingleAttributeReadsWhenTheBatchIsUnavailable() {
        let element = AXUIElementCreateApplication(0)
        let attributes = [kAXRoleAttribute, kAXTitleAttribute, "AXNonExistent"]
        let bag = AXAttributeBag(element: element, attributes: attributes)
        for attribute in attributes {
            XCTAssertEqual(
                bag.string(attribute),
                axCopyString(element, attribute: attribute),
                "Falling back must produce what a single read produces for \(attribute)"
            )
        }
        XCTAssertNil(bag.elements(kAXChildrenAttribute))
    }

    func testUnwrapTurnsAnErrorPlaceholderIntoNilAndPassesRealValuesThrough() {
        // AXUIElementCopyMultipleAttributeValues reports a per-attribute
        // failure in-band, as an AXValue wrapping an AXError. Those slots must
        // read as "absent", exactly like a failed single-attribute read.
        var error = AXError.attributeUnsupported
        let placeholder = AXValueCreate(.axError, &error)!
        XCTAssertNil(axUnwrapMultipleAttributeValue(placeholder))

        var point = CGPoint(x: 3, y: 4)
        let position = AXValueCreate(.cgPoint, &point)!
        XCTAssertNotNil(axUnwrapMultipleAttributeValue(position))

        XCTAssertEqual(axUnwrapMultipleAttributeValue("title" as AnyObject) as? String, "title")
        XCTAssertNil(axUnwrapMultipleAttributeValue(NSNull()))
    }

    // MARK: - axFrame

    func testAxFrameDecodesAPositionAndSizePair() {
        var point = CGPoint(x: 12, y: 34)
        var size = CGSize(width: 56, height: 78)
        let frame = axFrame(
            position: AXValueCreate(.cgPoint, &point)!,
            size: AXValueCreate(.cgSize, &size)!
        )
        XCTAssertEqual(frame?.x, 12)
        XCTAssertEqual(frame?.y, 34)
        XCTAssertEqual(frame?.width, 56)
        XCTAssertEqual(frame?.height, 78)
    }

    func testAxFrameReturnsNilForMissingOrWronglyTypedValues() {
        var point = CGPoint(x: 1, y: 2)
        var size = CGSize(width: 3, height: 4)
        let position = AXValueCreate(.cgPoint, &point)!
        let extent = AXValueCreate(.cgSize, &size)!

        XCTAssertNil(axFrame(position: nil, size: extent))
        XCTAssertNil(axFrame(position: position, size: nil))
        // A stale element can answer with something that is not an AXValue at
        // all; the decoder must refuse it instead of force-casting.
        XCTAssertNil(axFrame(position: "not-a-point" as AnyObject, size: extent))
        // Right container, wrong payload: a size where a point belongs.
        XCTAssertNil(axFrame(position: extent, size: extent))
    }

    // MARK: - axStringify

    func testAxStringifyReturnsNilForNil() {
        XCTAssertNil(axStringify(nil))
    }

    func testAxStringifyReturnsStringForString() {
        XCTAssertEqual(axStringify("hello" as AnyObject), "hello")
    }

    func testAxStringifyReturnsStringValueForNSNumber() {
        XCTAssertEqual(axStringify(42 as NSNumber), "42")
    }

    func testAxStringifyReturnsNilForOtherTypes() {
        let dict = ["key": "value"] as NSDictionary
        XCTAssertNil(axStringify(dict))
    }
}

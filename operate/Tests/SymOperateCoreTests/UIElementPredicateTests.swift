import XCTest
@testable import SymOperateCore

final class UIElementPredicateTests: XCTestCase {
    private let buttonNode = UINode(
        id: "btn",
        role: "AXButton",
        subrole: "AXPushButton",
        title: "Save",
        label: "save-button",
        value: nil,
        nodeDescription: nil,
        frame: nil,
        actions: ["AXPress"],
        children: []
    )

    private let textFieldNode = UINode(
        id: "tf",
        role: "AXTextField",
        subrole: nil,
        title: nil,
        label: "Email",
        value: "hello@example.com",
        nodeDescription: nil,
        frame: nil,
        actions: [],
        children: []
    )

    // MARK: - Field matching

    func testMatchesRole() {
        let predicate = UIElementPredicate(role: "AXButton")
        XCTAssertTrue(predicate.matches(node: buttonNode))
        XCTAssertFalse(predicate.matches(node: textFieldNode))
    }

    func testMatchesTitle() {
        let predicate = UIElementPredicate(title: "Save")
        XCTAssertTrue(predicate.matches(node: buttonNode))
        XCTAssertFalse(predicate.matches(node: textFieldNode))
    }

    func testMatchesLabelCaseInsensitive() {
        let predicate = UIElementPredicate(label: "EMAIL")
        XCTAssertTrue(predicate.matches(node: textFieldNode))
    }

    func testMatchesValue() {
        let predicate = UIElementPredicate(value: "hello@example.com")
        XCTAssertTrue(predicate.matches(node: textFieldNode))
    }

    func testMatchesSubrole() {
        let predicate = UIElementPredicate(subrole: "AXPushButton")
        XCTAssertTrue(predicate.matches(node: buttonNode))
        XCTAssertFalse(predicate.matches(node: textFieldNode))
    }

    func testMatchesActions() {
        let predicate = UIElementPredicate(actions: ["AXPress"])
        XCTAssertTrue(predicate.matches(node: buttonNode))
        XCTAssertFalse(predicate.matches(node: textFieldNode))
    }

    func testMatchesMultipleFields() {
        let predicate = UIElementPredicate(role: "AXButton", title: "Save", actions: ["AXPress"])
        XCTAssertTrue(predicate.matches(node: buttonNode))
    }

    func testMissWhenOneFieldDoesNotMatch() {
        let predicate = UIElementPredicate(role: "AXButton", title: "Cancel")
        XCTAssertFalse(predicate.matches(node: buttonNode))
    }

    func testNilFieldIsIgnored() {
        let predicate = UIElementPredicate(role: nil, title: "Save")
        XCTAssertTrue(predicate.matches(node: buttonNode))
    }

    // MARK: - Regex matching

    func testRegexPatternMatches() {
        let predicate = UIElementPredicate(title: "/Sa.*e/")
        XCTAssertTrue(predicate.matches(node: buttonNode))
    }

    func testRegexPatternDoesNotMatch() {
        let predicate = UIElementPredicate(title: "/Cancel/")
        XCTAssertFalse(predicate.matches(node: buttonNode))
    }

    func testEmptyRegexFallsBackToLiteral() {
        let predicate = UIElementPredicate(title: "//")
        XCTAssertFalse(predicate.matches(node: buttonNode))
    }

    func testOversizedRegexFallsBackToLiteral() {
        let longPattern = String(repeating: "a", count: 201)
        let predicate = UIElementPredicate(title: "/\(longPattern)/")
        XCTAssertFalse(predicate.matches(node: buttonNode))
    }

    func testUnsafeNestedQuantifierRegexFallsBackToLiteral() {
        // `(a+)+` would be rejected by isSafeRegex; fallback checks if field contains literal "(a+)"
        let predicate = UIElementPredicate(title: "/(a+)+/")
        XCTAssertFalse(predicate.matches(node: buttonNode))
    }

    func testBackreferenceRegexFallsBackToLiteral() {
        let predicate = UIElementPredicate(title: "/(foo)\\1/")
        XCTAssertFalse(predicate.matches(node: buttonNode))
    }

    // MARK: - Query service

    func testFindNodesRecursively() {
        let child = UINode(
            id: "child",
            role: "AXButton",
            subrole: nil,
            title: "Child Save",
            label: nil,
            value: nil,
            nodeDescription: nil,
            frame: nil,
            actions: [],
            children: []
        )
        let parent = UINode(
            id: "parent",
            role: "AXGroup",
            subrole: nil,
            title: nil,
            label: nil,
            value: nil,
            nodeDescription: nil,
            frame: nil,
            actions: [],
            children: [child]
        )
        let service = UIQueryService()
        let predicate = UIElementPredicate(role: "AXButton")
        let results = service.findNodes(in: [parent], predicate: predicate)
        XCTAssertEqual(results.map(\.id), ["child"])
    }

    func testFindFirstNodeReturnsFirstMatch() {
        let node1 = UINode(id: "1", role: "AXButton", subrole: nil, title: nil, label: nil, value: nil, nodeDescription: nil, frame: nil, actions: [], children: [])
        let node2 = UINode(id: "2", role: "AXButton", subrole: nil, title: nil, label: nil, value: nil, nodeDescription: nil, frame: nil, actions: [], children: [])
        let service = UIQueryService()
        let predicate = UIElementPredicate(role: "AXButton")
        XCTAssertEqual(service.findFirstNode(in: [node1, node2], predicate: predicate)?.id, "1")
    }

    func testFindFirstNodeReturnsNilWhenNoMatch() {
        let service = UIQueryService()
        let predicate = UIElementPredicate(role: "AXNonExistent")
        XCTAssertNil(service.findFirstNode(in: [buttonNode], predicate: predicate))
    }
}

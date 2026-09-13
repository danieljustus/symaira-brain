import XCTest
@testable import SymOperateCore

private final class RecordingFinderAdapter: FinderSemanticAdapterProtocol, @unchecked Sendable {
    let capabilityMetadata: FinderCapabilityMetadata
    var performed: [FinderSemanticDescriptor] = []
    var performError: Error?

    init(capabilityMetadata: FinderCapabilityMetadata) {
        self.capabilityMetadata = capabilityMetadata
    }

    func descriptor(for action: FinderSemanticAction, target: FinderSemanticTarget) throws -> FinderSemanticDescriptor {
        try FinderSemanticDescriptor(action: action, target: target, capability: capabilityMetadata)
    }

    func perform(_ descriptor: FinderSemanticDescriptor) throws -> FinderActionResult {
        performed.append(descriptor)
        if let performError { throw performError }
        return FinderActionResult(action: descriptor.action, route: .semantic, message: "semantic")
    }
}

private final class RecordingAXFallback: FinderAXFallbackProtocol, @unchecked Sendable {
    var descriptors: [FinderSemanticDescriptor] = []

    func performAXFallback(for descriptor: FinderSemanticDescriptor) throws -> FinderActionResult {
        descriptors.append(descriptor)
        return FinderActionResult(action: descriptor.action, route: .axFallback, message: "ax")
    }
}

final class FinderSemanticAdapterTests: XCTestCase {
    func testFinderMetadataAdvertisesCommonActionsAndAXFallback() {
        let metadata = FinderCapabilityMetadata.finder

        XCTAssertEqual(metadata.appFamily, "finder")
        XCTAssertEqual(metadata.bundleIdentifier, "com.apple.finder")
        XCTAssertTrue(metadata.supportsOpen)
        XCTAssertTrue(metadata.supportsSelect)
        XCTAssertTrue(metadata.supportsReveal)
        XCTAssertTrue(metadata.supportsSearch)
        XCTAssertTrue(metadata.supportsAXFallback)
        XCTAssertEqual(metadata.capabilities, Set(["open", "select", "reveal", "search"]))
    }

    func testDescriptorValidatesItemAndSearchTargets() throws {
        let item = try FinderSemanticDescriptor(action: .reveal, target: .item(path: "/tmp/example.txt"))
        XCTAssertEqual(item.action, .reveal)
        XCTAssertEqual(item.target, .item(path: "/tmp/example.txt"))

        let search = try FinderSemanticDescriptor(action: .search, target: .search(query: "invoice"))
        XCTAssertEqual(search.action, .search)
        XCTAssertEqual(search.target, .search(query: "invoice"))
    }

    func testDescriptorRejectsMismatchedOrEmptyTargetsWithoutAutomation() {
        XCTAssertThrowsError(try FinderSemanticDescriptor(action: .search, target: .item(path: "/tmp/example.txt")))
        XCTAssertThrowsError(try FinderSemanticDescriptor(action: .open, target: .search(query: "invoice")))
        XCTAssertThrowsError(try FinderSemanticDescriptor(action: .open, target: .item(path: "  ")))
        XCTAssertThrowsError(try FinderSemanticDescriptor(action: .search, target: .search(query: "  ")))
    }

    func testRouterUsesSemanticAdapterWhenCapabilityIsAvailable() throws {
        let adapter = RecordingFinderAdapter(capabilityMetadata: FinderCapabilityMetadata(supportedActions: [.open]))
        let fallback = RecordingAXFallback()
        let router = FinderActionRouter(adapter: adapter, axFallback: fallback)

        let result = try router.perform(.open, target: .item(path: "/tmp/example.txt"))

        XCTAssertEqual(result.route, .semantic)
        XCTAssertEqual(adapter.performed.count, 1)
        XCTAssertTrue(fallback.descriptors.isEmpty)
    }

    func testRouterFallsBackToAXWhenSemanticCapabilityIsMissing() throws {
        let adapter = RecordingFinderAdapter(capabilityMetadata: FinderCapabilityMetadata(supportedActions: [.open]))
        let fallback = RecordingAXFallback()
        let router = FinderActionRouter(adapter: adapter, axFallback: fallback)

        let result = try router.perform(.search, target: .search(query: "invoice"))

        XCTAssertEqual(result.route, .axFallback)
        XCTAssertTrue(adapter.performed.isEmpty)
        XCTAssertEqual(fallback.descriptors.map(\.action), [.search])
    }

    func testRouterFallsBackOnlyForUnavailableSemanticAdapter() throws {
        let adapter = RecordingFinderAdapter(capabilityMetadata: .finder)
        adapter.performError = FinderSemanticAdapterError.unavailable("semantic route unavailable")
        let fallback = RecordingAXFallback()
        let router = FinderActionRouter(adapter: adapter, axFallback: fallback)

        let result = try router.perform(.reveal, target: .item(path: "/tmp/example.txt"))

        XCTAssertEqual(result.route, .axFallback)
        XCTAssertEqual(fallback.descriptors.count, 1)
    }

    func testRouterDoesNotMaskSemanticExecutionFailureWithAX() {
        let adapter = RecordingFinderAdapter(capabilityMetadata: .finder)
        adapter.performError = FinderSemanticAdapterError.operationFailed("Finder rejected the operation")
        let fallback = RecordingAXFallback()
        let router = FinderActionRouter(adapter: adapter, axFallback: fallback)

        XCTAssertThrowsError(try router.perform(.open, target: .item(path: "/tmp/example.txt"))) { error in
            XCTAssertEqual(error as? FinderSemanticAdapterError, .operationFailed("Finder rejected the operation"))
        }
        XCTAssertTrue(fallback.descriptors.isEmpty)
    }

    func testCapabilityMetadataCanDisableFallback() {
        let adapter = RecordingFinderAdapter(capabilityMetadata: FinderCapabilityMetadata(
            supportedActions: [.open],
            supportsAXFallback: false
        ))
        let fallback = RecordingAXFallback()
        let router = FinderActionRouter(adapter: adapter, axFallback: fallback)

        XCTAssertThrowsError(try router.perform(.search, target: .search(query: "invoice"))) { error in
            XCTAssertEqual(error as? FinderSemanticAdapterError, .unsupportedAction(.search))
        }
        XCTAssertTrue(fallback.descriptors.isEmpty)
    }
}

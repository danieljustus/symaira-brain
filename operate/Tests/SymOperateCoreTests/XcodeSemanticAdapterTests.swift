import XCTest
@testable import SymOperateCore

private struct StubXcodeAdapter: XcodeSemanticAdapterProtocol {
    let descriptor: XcodeDescriptor
    let error: AutomationError?

    func perform(_ request: XcodeActionRequest) throws -> XcodeActionResult {
        if let error { throw error }
        return XcodeActionResult(action: request.action, route: .semantic, message: "semantic")
    }
}

private struct StubXcodeOperations: XcodeSemanticOperationsProtocol {
    let result: XcodeActionResult

    func perform(_ request: XcodeActionRequest) throws -> XcodeActionResult {
        result
    }
}

private final class RecordingXcodeAXFallback: XcodeAXFallbackProtocol, @unchecked Sendable {
    private(set) var requests: [XcodeActionRequest] = []

    func performAX(_ request: XcodeActionRequest) throws -> XcodeActionResult {
        requests.append(request)
        return XcodeActionResult(action: request.action, route: .ax, message: "ax")
    }
}

final class XcodeSemanticAdapterTests: XCTestCase {
    func testDescriptorAndRequestAreCodableWithAllCapabilities() throws {
        let descriptor = XcodeDescriptor.xcode
        let request = XcodeActionRequest(
            action: .test,
            projectPath: "/tmp/Example.xcodeproj",
            documentPath: "/tmp/Example/Sources/App.swift",
            scheme: "Example",
            testName: "ExampleTests/testLaunch",
            destination: "platform=macOS",
            navigationTarget: "ExampleTests/testLaunch",
            snapshotID: "snapshot-1",
            elementID: "element-1"
        )
        let descriptorData = try JSONEncoder().encode(descriptor)
        let requestData = try JSONEncoder().encode(request)

        XCTAssertEqual(try JSONDecoder().decode(XcodeDescriptor.self, from: descriptorData), descriptor)
        XCTAssertEqual(try JSONDecoder().decode(XcodeActionRequest.self, from: requestData), request)
        XCTAssertEqual(descriptor.bundleIdentifier, "com.apple.dt.Xcode")
        XCTAssertEqual(descriptor.capabilityMetadata.appFamily, "xcode")
        XCTAssertEqual(descriptor.capabilities.capabilityNames, ["project", "document", "build", "test", "navigation"])
        XCTAssertTrue(descriptor.capabilities.supportsAXFallback)
        XCTAssertTrue(request.hasAXReference)
        XCTAssertEqual(request.workspacePath, request.projectPath)
    }

    func testSemanticRouteUsesInjectedOperationsWithoutAX() throws {
        let operations = StubXcodeOperations(
            result: XcodeActionResult(action: .build, route: .semantic, message: "built")
        )
        let adapter = XcodeSemanticAdapter(operations: operations)
        let fallback = RecordingXcodeAXFallback()
        let router = XcodeActionRouter(semantic: adapter, axFallback: fallback)

        let result = try router.perform(XcodeActionRequest(
            action: .build,
            projectPath: "/tmp/Example.xcodeproj",
            scheme: "Example"
        ))

        XCTAssertEqual(result.route, .semantic)
        XCTAssertEqual(result.message, "built")
        XCTAssertTrue(fallback.requests.isEmpty)
    }

    func testFallbackRequiresObservedElementAndDoesNotMaskOperationFailure() {
        let limitedDescriptor = XcodeDescriptor(
            capabilityMetadata: XcodeCapabilityDescriptor(
                capabilities: [.project, .document],
                semanticActions: [.openProject, .openDocument],
                supportsAXFallback: true
            )
        )
        let fallback = RecordingXcodeAXFallback()
        let limitedRouter = XcodeActionRouter(
            semantic: StubXcodeAdapter(descriptor: limitedDescriptor, error: nil),
            axFallback: fallback
        )

        XCTAssertThrowsError(try limitedRouter.perform(XcodeActionRequest(action: .test))) { error in
            guard case .invalidArgument = error as? AutomationError else {
                return XCTFail("Expected an invalid argument for an unreferenced AX fallback")
            }
        }
        XCTAssertTrue(fallback.requests.isEmpty)

        let failingRouter = XcodeActionRouter(
            semantic: StubXcodeAdapter(
                descriptor: .xcode,
                error: .operationFailed("Xcode rejected the build")
            ),
            axFallback: fallback
        )
        XCTAssertThrowsError(try failingRouter.perform(XcodeActionRequest(
            action: .build,
            scheme: "Example",
            snapshotID: "snapshot-1",
            elementID: "element-1"
        ))) { error in
            guard case .operationFailed = error as? AutomationError else {
                return XCTFail("Expected the semantic operation failure to propagate")
            }
        }
        XCTAssertTrue(fallback.requests.isEmpty)
    }
}

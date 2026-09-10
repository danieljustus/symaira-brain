import XCTest
@testable import SymOperateCore

private struct StubSemanticAdapter: TerminalSemanticAdapterProtocol {
    enum Behavior: Sendable {
        case success
        case unsupported
        case failed
    }

    let descriptor: TerminalDescriptor
    let behavior: Behavior

    func perform(_ request: TerminalActionRequest) throws -> TerminalActionResult {
        switch behavior {
        case .success:
            return TerminalActionResult(action: request.action, route: .semantic, message: "semantic")
        case .unsupported:
            throw AutomationError.unsupported("stub")
        case .failed:
            throw AutomationError.operationFailed("stub")
        }
    }
}

private final class StubAXFallback: TerminalAXFallbackProtocol, @unchecked Sendable {
    private(set) var calls = 0

    func performAX(_ request: TerminalActionRequest) throws -> TerminalActionResult {
        calls += 1
        return TerminalActionResult(action: request.action, route: .ax, message: "ax")
    }
}

final class TerminalAdapterTests: XCTestCase {
    func testAppleTerminalDescriptorRoundTripsCapabilitiesAsCodableMetadata() throws {
        let descriptor = TerminalDescriptor.appleTerminal
        let data = try JSONEncoder().encode(descriptor)
        let decoded = try JSONDecoder().decode(TerminalDescriptor.self, from: data)

        XCTAssertEqual(decoded, descriptor)
        XCTAssertEqual(decoded.bundleIdentifier, "com.apple.Terminal")
        XCTAssertTrue(decoded.capabilities.capabilities.isSuperset(of: [.shell, .sessions, .tabs]))
        XCTAssertTrue(decoded.capabilities.supports(.openShell))
        XCTAssertTrue(decoded.capabilities.supports(.createSession))
        XCTAssertTrue(decoded.capabilities.supports(.createTab))
        XCTAssertTrue(decoded.capabilities.supportsAXFallback)
    }

    func testRouterPrefersSemanticRouteAndFallsBackForUnsupportedSemanticAction() throws {
        let ax = StubAXFallback()
        let request = TerminalActionRequest(
            action: .selectTab,
            identifier: "tab-1",
            snapshotID: "snapshot-1",
            elementID: "element-1"
        )

        let semanticRouter = TerminalActionRouter(
            semantic: StubSemanticAdapter(descriptor: .appleTerminal, behavior: .success),
            axFallback: ax
        )
        XCTAssertEqual(try semanticRouter.perform(request).route, .semantic)
        XCTAssertEqual(ax.calls, 0)

        let unsupportedRouter = TerminalActionRouter(
            semantic: StubSemanticAdapter(
                descriptor: TerminalDescriptor(
                    bundleIdentifier: "com.example.terminal",
                    applicationName: "Example Terminal",
                    capabilityMetadata: TerminalCapabilityDescriptor(
                        capabilities: [.tabs],
                        semanticActions: [],
                        supportsAXFallback: true
                    )
                ),
                behavior: .success
            ),
            axFallback: ax
        )
        XCTAssertEqual(try unsupportedRouter.perform(request).route, .ax)
        XCTAssertEqual(ax.calls, 1)
    }

    func testRouterFailsClosedWithoutAXReferenceAndDoesNotFallbackOperationFailures() {
        let ax = StubAXFallback()
        let noReferenceRequest = TerminalActionRequest(action: .createTab)
        let router = TerminalActionRouter(
            semantic: StubSemanticAdapter(
                descriptor: TerminalDescriptor(
                    bundleIdentifier: "com.example.terminal",
                    applicationName: "Example Terminal",
                    capabilityMetadata: TerminalCapabilityDescriptor(
                        capabilities: [.tabs],
                        semanticActions: [],
                        supportsAXFallback: true
                    )
                ),
                behavior: .success
            ),
            axFallback: ax
        )

        XCTAssertThrowsError(try router.perform(noReferenceRequest)) { error in
            guard case .invalidArgument = error as? AutomationError else {
                return XCTFail("Expected an invalid argument for an unreferenced AX fallback")
            }
        }
        XCTAssertEqual(ax.calls, 0)

        let failingRouter = TerminalActionRouter(
            semantic: StubSemanticAdapter(descriptor: .appleTerminal, behavior: .failed),
            axFallback: ax
        )
        XCTAssertThrowsError(try failingRouter.perform(TerminalActionRequest(
            action: .runCommand,
            snapshotID: "snapshot-1",
            elementID: "element-1"
        ))) { error in
            guard case .operationFailed = error as? AutomationError else {
                return XCTFail("Expected semantic operation failure to propagate")
            }
        }
        XCTAssertEqual(ax.calls, 0)
    }
}

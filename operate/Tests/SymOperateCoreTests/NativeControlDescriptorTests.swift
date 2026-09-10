import XCTest
@testable import SymOperateCore

private struct NativeSemanticSpy: NativeControlSemanticAdapterProtocol {
    let result: NativeControlActionResult?
    let error: AutomationError?

    func perform(_ request: NativeControlActionRequest) throws -> NativeControlActionResult {
        if let error { throw error }
        return result ?? NativeControlActionResult(kind: request.descriptor.kind, route: .semantic, message: "native")
    }
}

private final class NativeAXSpy: NativeControlAXFallbackProtocol, @unchecked Sendable {
    private(set) var requests: [NativeControlActionRequest] = []

    func performAX(_ request: NativeControlActionRequest) throws -> NativeControlActionResult {
        requests.append(request)
        return NativeControlActionResult(kind: request.descriptor.kind, route: .axFallback, message: "ax")
    }
}

final class NativeControlDescriptorTests: XCTestCase {
    func testToolbarDescriptorMatchesToolbarAndRequiredControlFromPureSnapshot() {
        let save = NativeControlDescriptor.control(role: "AXButton", label: "Save")
        let toolbar = UINode(
            id: "toolbar",
            role: "AXToolbar",
            subrole: nil,
            title: "Main Toolbar",
            label: nil,
            value: nil,
            nodeDescription: nil,
            frame: nil,
            actions: [],
            children: [UINode(
                id: "save",
                role: "AXButton",
                subrole: "AXPushButton",
                title: nil,
                label: "Save",
                value: nil,
                nodeDescription: nil,
                frame: nil,
                actions: ["AXPress"],
                children: []
            )]
        )

        XCTAssertTrue(NativeControlDescriptor.toolbar(title: "Main", requiredControls: [save]).matches(toolbar))
        XCTAssertFalse(NativeControlDescriptor.toolbar(title: "Inspector").matches(toolbar))
    }

    func testSheetDescriptorMatchesOnlySheetRoleAndControlDescriptorUsesAXFields() {
        let sheet = UINode(
            id: "sheet",
            role: "AXSheet",
            subrole: nil,
            title: "Export",
            label: nil,
            value: nil,
            nodeDescription: nil,
            frame: nil,
            actions: [],
            children: []
        )
        let window = UINode(
            id: "window",
            role: "AXWindow",
            subrole: nil,
            title: "Export",
            label: nil,
            value: nil,
            nodeDescription: nil,
            frame: nil,
            actions: [],
            children: []
        )

        XCTAssertTrue(NativeControlDescriptor.sheet(title: "Export").matches(sheet))
        XCTAssertFalse(NativeControlDescriptor.sheet(title: "Export").matches(window))
        XCTAssertTrue(NativeControlDescriptor.control(role: "AXButton", identifier: "export", enabled: true)
            .matches(UINode(
                id: "button",
                role: "AXButton",
                subrole: nil,
                title: "Export",
                label: "export",
                value: nil,
                nodeDescription: nil,
                frame: nil,
                actions: ["AXPress"],
                enabled: true,
                children: []
            )))
    }

    func testRouterFallsBackOnlyWithExplicitAXReferenceAndDoesNotRetryFailures() throws {
        let descriptor = NativeControlDescriptor.control(role: "AXButton", label: "Save")
        let request = NativeControlActionRequest(descriptor: descriptor, snapshotID: "snapshot", elementID: "element")
        let ax = NativeAXSpy()
        let router = NativeControlActionRouter(
            semantic: NativeSemanticSpy(result: nil, error: .unavailable("native route unavailable")),
            axFallback: ax
        )

        XCTAssertEqual(try router.perform(request).route, .axFallback)
        XCTAssertEqual(ax.requests.count, 1)

        let failingRouter = NativeControlActionRouter(
            semantic: NativeSemanticSpy(result: nil, error: .operationFailed("native action failed")),
            axFallback: ax
        )
        XCTAssertThrowsError(try failingRouter.perform(request)) { error in
            guard case .operationFailed = error as? AutomationError else {
                return XCTFail("Expected the native operation failure to propagate")
            }
        }
        XCTAssertEqual(ax.requests.count, 1)

        let missingReference = NativeControlActionRequest(descriptor: descriptor)
        let unavailableRouter = NativeControlActionRouter(semantic: nil, axFallback: ax)
        XCTAssertThrowsError(try unavailableRouter.perform(missingReference)) { error in
            guard case .invalidArgument = error as? AutomationError else {
                return XCTFail("Expected AX fallback to require a snapshot and element identity")
            }
        }
        XCTAssertEqual(ax.requests.count, 1)
    }
}

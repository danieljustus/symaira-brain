import XCTest
@testable import SymOperateCore

final class DockStatusBarAdapterTests: XCTestCase {
    func testDescriptorsIdentifyEachSurfaceAndRoundTripAsMetadata() throws {
        let dock = DockSemanticDescriptor(
            bundleIdentifier: "com.apple.Safari",
            applicationName: "Safari",
            itemIdentifier: "com.apple.Safari",
            itemTitle: "Safari"
        )
        let menuBar = MenuBarSemanticDescriptor(
            bundleIdentifier: "com.apple.Safari",
            applicationName: "Safari",
            menuPath: ["File", "New Window"],
            itemTitle: "New Window"
        )
        let statusBar = StatusBarSemanticDescriptor(
            bundleIdentifier: "com.example.agent",
            itemIdentifier: "agent-status",
            itemTitle: "Agent Status"
        )

        XCTAssertEqual(dock.surface, .dock)
        XCTAssertEqual(menuBar.surface, .menuBar)
        XCTAssertEqual(statusBar.surface, .statusBar)
        XCTAssertTrue(dock.supports(.activate))
        XCTAssertTrue(menuBar.supports(.selectItem))
        XCTAssertTrue(statusBar.supports(.activate))

        let data = try JSONEncoder().encode(menuBar)
        XCTAssertEqual(try JSONDecoder().decode(MenuBarSemanticDescriptor.self, from: data), menuBar)
    }

    func testSemanticOnlyRoutingFailsClosedWithoutUnsafeFallback() {
        let dock = DockSemanticDescriptor(supportedActions: [.activate])
        let menuBar = MenuBarSemanticDescriptor(supportedActions: [.openMenu])

        let dockDecision = dock.routeDecision(
            for: .activate,
            semanticAvailable: false,
            accessibilityFallbackAvailable: true,
            hasFreshObservation: true
        )
        XCTAssertEqual(dockDecision.route, .refused)
        XCTAssertFalse(dockDecision.isExecutable)
        XCTAssertFalse(dockDecision.requiresFreshObservation)

        let menuDecision = menuBar.routeDecision(
            for: .selectItem,
            semanticAvailable: true
        )
        XCTAssertEqual(menuDecision.route, .refused)
        XCTAssertTrue(menuDecision.reason.contains("not advertised"))
    }

    func testAccessibilityFallbackIsExplicitAndRequiresFreshObservation() {
        let descriptor = StatusBarSemanticDescriptor(
            supportedActions: [.openMenu],
            routing: .semanticThenObservedAccessibility
        )

        let stale = descriptor.routeDecision(
            for: .openMenu,
            semanticAvailable: false,
            accessibilityFallbackAvailable: true,
            hasFreshObservation: false
        )
        XCTAssertEqual(stale.route, .refused)
        XCTAssertTrue(stale.requiresFreshObservation)

        let observed = descriptor.routeDecision(
            for: .openMenu,
            semanticAvailable: false,
            accessibilityFallbackAvailable: true,
            hasFreshObservation: true
        )
        XCTAssertEqual(observed.route, .accessibilityFallback)
        XCTAssertTrue(observed.isExecutable)
    }
}

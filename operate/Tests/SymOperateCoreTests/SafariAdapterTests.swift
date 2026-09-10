import Foundation
import XCTest
@testable import SymOperateCore

private final class SafariAdapterSpy: SafariSemanticOperationsProtocol, SafariAXFallbackProtocol, @unchecked Sendable {
    var observation: SafariObservation
    var performCount = 0
    var lastAction: SafariSemanticAction?

    init(observation: SafariObservation) {
        self.observation = observation
    }

    func observe(target: TargetIdentity) throws -> SafariObservation {
        observation
    }

    func perform(_ action: SafariSemanticAction, target: TargetIdentity) throws -> SafariObservation {
        performCount += 1
        lastAction = action
        return observation
    }
}

final class SafariAdapterTests: XCTestCase {
    func testSafariDescriptorIsCodableAndAdvertisesCapabilities() throws {
        let descriptor = SafariSemanticDescriptor.safari
        let encoded = try JSONEncoder().encode(descriptor)
        let decoded = try JSONDecoder().decode(SafariSemanticDescriptor.self, from: encoded)

        XCTAssertEqual(decoded, descriptor)
        XCTAssertEqual(descriptor.bundleIdentifier, "com.apple.Safari")
        XCTAssertTrue(descriptor.supports(.tabIdentity))
        XCTAssertTrue(descriptor.supports(.tabSelection))
        XCTAssertTrue(descriptor.supports(.addressFieldFocus))
        XCTAssertTrue(descriptor.supports(.navigation))
        XCTAssertTrue(descriptor.supports(.loadingState))
        XCTAssertEqual(descriptor.capabilityProfile.appFamily, "safari")
    }

    func testSemanticRouteVerifiesTabSelectionWithoutAXFallback() throws {
        let tab = SafariTabState(id: "tab-2", index: 1, title: "Example", isSelected: true)
        let observation = SafariObservation(
            processID: 42,
            windows: [SafariWindowState(windowID: 7, title: "Example", isFocused: true, tabs: [tab])],
            focusedWindowID: 7,
            observedAt: "2026-08-28T00:00:00Z"
        )
        let semantic = SafariAdapterSpy(observation: observation)
        let fallback = SafariAdapterSpy(observation: observation)
        let adapter = SafariSemanticAdapter(semantic: semantic, axFallback: fallback)

        let result = try adapter.selectTab(
            id: "tab-2",
            windowID: 7,
            target: TargetIdentity(bundleID: SafariSemanticDescriptor.bundleID, windowID: 7)
        )

        XCTAssertEqual(result.route, .semantic)
        XCTAssertEqual(result.status, .completed)
        XCTAssertEqual(result.observation.selectedTab?.id, "tab-2")
        XCTAssertEqual(semantic.performCount, 1)
        XCTAssertEqual(fallback.performCount, 0)
    }

    func testNavigationFallsBackToAXReportsTimeoutAndRejectsCredentials() throws {
        let loadingTab = SafariTabState(id: "tab-1", index: 0, url: "https://example.com", isSelected: true, isLoading: true)
        let fallback = SafariAdapterSpy(observation: SafariObservation(
            processID: 42,
            windows: [SafariWindowState(windowID: 7, isFocused: true, tabs: [loadingTab])],
            focusedWindowID: 7,
            observedAt: "2026-08-28T00:00:00Z"
        ))
        let descriptor = SafariSemanticDescriptor(capabilities: [.tabIdentity, .tabSelection, .addressFieldFocus, .loadingState])
        let adapter = SafariSemanticAdapter(descriptor: descriptor, semantic: nil, axFallback: fallback)

        let result = try adapter.navigate(
            to: "https://example.com",
            target: TargetIdentity(bundleID: SafariSemanticDescriptor.bundleID, windowID: 7),
            timeout: 0.1
        )

        XCTAssertEqual(result.route, .accessibilityFallback)
        XCTAssertEqual(result.status, .timedOut)
        XCTAssertEqual(fallback.performCount, 1)

        XCTAssertThrowsError(try adapter.navigate(
            to: "https://user:password@example.com",
            target: TargetIdentity(bundleID: SafariSemanticDescriptor.bundleID),
            timeout: 1
        )) { error in
            guard case AutomationError.invalidArgument = error else {
                return XCTFail("Expected embedded credentials to be rejected, got \(error)")
            }
        }
    }
}

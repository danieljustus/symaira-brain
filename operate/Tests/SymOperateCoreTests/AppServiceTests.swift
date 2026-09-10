import XCTest
@testable import SymOperateCore

final class AppServiceTests: XCTestCase {
    // MARK: - read-only enumeration (no side effects)

    func testListAppsReturnsRunningRegularApps() {
        let service = AppService()
        let apps = service.listApps()
        // Only assert shape/invariants — the actual running-app set is host-dependent.
        XCTAssertTrue(apps.allSatisfy { !$0.localizedName.isEmpty })
        let sortedNames = apps.map(\.localizedName)
        XCTAssertEqual(sortedNames, sortedNames.sorted(), "listApps must return apps sorted by localized name")
    }

    func testListWindowsReturnsWellFormedEntries() {
        let service = AppService()
        let windows = service.listWindows()
        for window in windows {
            XCTAssertFalse(window.ownerName.isEmpty)
            XCTAssertGreaterThanOrEqual(window.bounds.width, 0)
            XCTAssertGreaterThanOrEqual(window.bounds.height, 0)
        }
    }

    func testFrontmostAppReturnsActiveFlagWhenPresent() {
        let service = AppService()
        if let app = service.frontmostApp() {
            XCTAssertTrue(app.isActive)
            XCTAssertFalse(app.localizedName.isEmpty)
        }
        // On a headless CI runner there may be no frontmost app — absence is valid too.
    }

    // MARK: - launchApp failure paths

    func testLaunchAppMissingArgumentsThrowsInvalidArgument() {
        let service = AppService()
        XCTAssertThrowsError(try service.launchApp(bundleID: nil, appName: nil)) { error in
            guard let automationError = error as? AutomationError, case .invalidArgument = automationError else {
                return XCTFail("expected invalidArgument, got \(error)")
            }
        }
    }

    func testLaunchAppEmptyStringArgumentsThrowsInvalidArgument() {
        let service = AppService()
        XCTAssertThrowsError(try service.launchApp(bundleID: "", appName: "")) { error in
            guard let automationError = error as? AutomationError, case .invalidArgument = automationError else {
                return XCTFail("expected invalidArgument, got \(error)")
            }
        }
    }

    func testLaunchAppUnknownBundleIDThrowsNotFound() {
        let service = AppService()
        XCTAssertThrowsError(
            try service.launchApp(bundleID: "com.symaira.does-not-exist.\(UUID().uuidString)", appName: nil)
        ) { error in
            guard let automationError = error as? AutomationError, case .notFound = automationError else {
                return XCTFail("expected notFound, got \(error)")
            }
        }
    }

    func testLaunchAppUnknownAppNameThrowsNotFound() {
        let service = AppService()
        XCTAssertThrowsError(
            try service.launchApp(bundleID: nil, appName: "DoesNotExist-\(UUID().uuidString)")
        ) { error in
            guard let automationError = error as? AutomationError, case .notFound = automationError else {
                return XCTFail("expected notFound, got \(error)")
            }
        }
    }

    func testLaunchAppBundleIDTakesPrecedenceOverAppName() {
        // Both provided, bundleID invalid — must fail on the bundleID branch
        // (checked first) rather than falling through to the appName branch.
        let service = AppService()
        XCTAssertThrowsError(
            try service.launchApp(bundleID: "com.symaira.does-not-exist.\(UUID().uuidString)", appName: "Finder")
        ) { error in
            guard let automationError = error as? AutomationError, case .notFound = automationError else {
                return XCTFail("expected notFound, got \(error)")
            }
        }
    }

    // MARK: - focusWindow failure path (no activation side effect)

    func testFocusWindowUnknownAppThrowsNotFound() {
        let service = AppService()
        XCTAssertThrowsError(
            try service.focusWindow(bundleID: "com.symaira.does-not-exist.\(UUID().uuidString)", appName: nil, title: nil)
        ) { error in
            guard let automationError = error as? AutomationError, case .notFound = automationError else {
                return XCTFail("expected notFound, got \(error)")
            }
        }
    }
}

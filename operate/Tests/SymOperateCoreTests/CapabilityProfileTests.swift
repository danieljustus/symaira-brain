import Foundation
import XCTest
@testable import SymOperateCore

final class CapabilityProfileTests: XCTestCase {
    func testKnownBundleProfileWinsOverGenericFallback() throws {
        let data = Data(#"""
            {
                "version": 1,
                "profiles": {
                    "com.example.editor": {
                        "version": 1,
                        "app_family": "editor",
                        "capabilities": ["capture", "input"]
                    }
                }
            }
            """#.utf8)
        let store = try JSONDecoder().decode(CapabilityProfileStore.self, from: data)

        let profile = store.profile(forBundleID: "com.example.editor")
        XCTAssertEqual(profile.appFamily, "editor")
        XCTAssertEqual(profile.capabilities, ["capture", "input"])
    }

    func testFamilyProfileIsUsedWhenBundleIsUnknown() throws {
        let profile = CapabilityProfile(appFamily: "browser", capabilities: ["capture"])
        let store = CapabilityProfileStore(profiles: ["browser": profile])

        XCTAssertEqual(store.profile(forBundleID: "com.example.browser", appFamily: "browser"), profile)
    }

    func testUnknownApplicationUsesConservativeGenericFallback() {
        let profile = CapabilityProfileStore.empty.profile(forBundleID: "com.example.unknown", appFamily: "unknown")

        XCTAssertEqual(profile, .generic)
        XCTAssertEqual(profile.appFamily, "generic")
        XCTAssertTrue(profile.capabilities.isEmpty)
    }

    func testMalformedProfileVersionIsRejected() {
        let data = Data(#"""
            {
                "version": 1,
                "profiles": {
                    "com.example.editor": {
                        "version": 99,
                        "app_family": "editor",
                        "capabilities": ["capture"]
                    }
                }
            }
            """#.utf8)

        XCTAssertThrowsError(try JSONDecoder().decode(CapabilityProfileStore.self, from: data)) { error in
            guard case .unsupportedVersion(99) = error as? CapabilityProfileError else {
                return XCTFail("Expected unsupported profile version, got \(error)")
            }
        }
    }

    func testMalformedProfileDocumentIsRejected() {
        let data = Data(#"{"version":1,"profiles":[]}"#.utf8)

        XCTAssertThrowsError(try JSONDecoder().decode(CapabilityProfileStore.self, from: data))
    }
}

import Foundation
import Testing
@testable import SymBrainCore

struct AppVersionInfoTests {
    @Test func combinesShortVersionAndBuildNumber() {
        #expect(AppVersionInfo.displayString(short: "0.6.0", build: "1") == "0.6.0 (1)")
    }

    @Test func omitsBuildNumberWhenMissing() {
        #expect(AppVersionInfo.displayString(short: "0.6.0", build: nil) == "0.6.0")
        #expect(AppVersionInfo.displayString(short: "0.6.0", build: "") == "0.6.0")
        #expect(AppVersionInfo.displayString(short: "0.6.0", build: "   ") == "0.6.0")
    }

    @Test func fallsBackToBuildNumberWhenShortVersionMissing() {
        #expect(AppVersionInfo.displayString(short: nil, build: "42") == "build 42")
    }

    @Test func reportsUnknownWhenBothMissing() {
        #expect(AppVersionInfo.displayString(short: nil, build: nil) == "unknown")
        #expect(AppVersionInfo.displayString(short: "", build: "  ") == "unknown")
    }

    @Test func readsValuesFromAnInfoDictionary() {
        let bundle = Bundle(for: TestAnchor.self)
        // The test bundle carries its own versions; the helper must not crash
        // and must return a non-empty string for any real bundle.
        #expect(!AppVersionInfo.current(bundle: bundle).isEmpty)
    }
}

private final class TestAnchor {}

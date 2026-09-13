import XCTest
@testable import SymOperateCore
import SymairaUpdateCheck

// MARK: - Stub HTTP Client

private struct StubHTTPClient: UpdateHTTPClient {
    let body: String
    let status: Int

    func data(for request: URLRequest) async throws -> (Data, URLResponse) {
        let response = HTTPURLResponse(
            url: request.url!,
            statusCode: status,
            httpVersion: nil,
            headerFields: nil
        )!
        return (Data(body.utf8), response)
    }
}

// MARK: - Tests

final class UpdateCheckerTests: XCTestCase {
    /// A temporary directory unique per test for the disk cache.
    private var cacheDir: URL!
    /// Isolated UserDefaults suite so skip-state never leaks between tests.
    private var defaults: UserDefaults!

    override func setUpWithError() throws {
        cacheDir = FileManager.default.temporaryDirectory
            .appendingPathComponent("symoperate-updatecheck-\(UUID().uuidString)")
        defaults = UserDefaults(suiteName: "com.symaira.operate.tests.\(UUID().uuidString)")
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: cacheDir)
    }

    /// Create an UpdateChecker with a stubbed HTTP client that returns the given latest tag.
    private func makeChecker(
        latestTag: String = "v1.1.0",
        status: Int = 200,
        currentVersion: String = "1.0.0"
    ) -> SymOperateCore.UpdateChecker {
        let body = #"{"tag_name":"\#(latestTag)","html_url":"https://github.com/danieljustus/symaira-cockpit/releases/tag/\#(latestTag)"}"#
        return SymOperateCore.UpdateChecker(
            currentVersion: currentVersion,
            repoOwner: "danieljustus",
            repoName: "symaira-cockpit",
            client: StubHTTPClient(body: body, status: status),
            cacheDirectory: cacheDir,
            userDefaults: defaults
        )
    }

    // MARK: - Update Detection

    func testReportsAvailableUpdate() async {
        let checker = makeChecker(latestTag: "v1.1.0", currentVersion: "1.0.0")
        let info = await checker.checkForUpdate()
        XCTAssertTrue(info.updateAvailable)
        XCTAssertEqual(info.latestVersion, "1.1.0")
        XCTAssertNotNil(info.releaseURL)
        XCTAssertNil(info.error)
    }

    func testUpToDateReportsNoUpdate() async {
        let checker = makeChecker(latestTag: "v1.0.0", currentVersion: "1.0.0")
        let info = await checker.checkForUpdate()
        XCTAssertFalse(info.updateAvailable)
        XCTAssertNil(info.latestVersion)
    }

    func testNewerLocalVersionReturnsNoUpdate() async {
        let checker = makeChecker(latestTag: "v1.0.0", currentVersion: "1.2.0")
        let info = await checker.checkForUpdate()
        XCTAssertFalse(info.updateAvailable)
        XCTAssertNil(info.latestVersion)
    }

    // MARK: - Error Handling

    func testHTTPErrorReturnsError() async {
        let checker = makeChecker(latestTag: "v1.1.0", status: 500, currentVersion: "1.0.0")
        let info = await checker.checkForUpdate()
        XCTAssertFalse(info.updateAvailable)
        XCTAssertNotNil(info.error)
    }

    // MARK: - Dev Version

    func testDevVersionSkipsCheck() async {
        let checker = makeChecker(latestTag: "v9.9.9", currentVersion: "1.0.0-dev")
        let info = await checker.checkForUpdate()
        XCTAssertFalse(info.updateAvailable)
        XCTAssertNil(info.error)
    }

    // MARK: - Skip Version

    func testSkippedVersionIsNotOffered() async {
        let checker = makeChecker(latestTag: "v1.1.0", currentVersion: "1.0.0")
        // First call: update is available
        let info1 = await checker.checkForUpdate()
        XCTAssertTrue(info1.updateAvailable)

        // Skip version 1.1.0
        checker.skipVersion("1.1.0")
        XCTAssertEqual(checker.skippedVersion, "1.1.0")

        // Second call: should no longer be offered
        let info2 = await checker.checkForUpdate()
        XCTAssertFalse(info2.updateAvailable)
    }

    func testForceCheckBypassesSkipGate() async {
        let checker = makeChecker(latestTag: "v1.1.0", currentVersion: "1.0.0")
        checker.skipVersion("1.1.0")
        XCTAssertEqual(checker.skippedVersion, "1.1.0")

        // With force=true, the skipped version should still be reported
        let info = await checker.checkForUpdate(force: true)
        XCTAssertTrue(info.updateAvailable)
    }

    func testClearSkippedVersion() async {
        let checker = makeChecker(latestTag: "v1.1.0", currentVersion: "1.0.0")
        checker.skipVersion("1.1.0")
        XCTAssertEqual(checker.skippedVersion, "1.1.0")

        checker.clearSkippedVersion()
        XCTAssertNil(checker.skippedVersion)
    }

    func testSkipWithVPrefix() async {
        let checker = makeChecker(latestTag: "v1.1.0", currentVersion: "1.0.0")
        checker.skipVersion("v1.1.0")
        XCTAssertEqual(checker.skippedVersion, "1.1.0")

        // Should be skipped
        let info = await checker.checkForUpdate()
        XCTAssertFalse(info.updateAvailable)
    }

    // MARK: - UpdateInfo Codable

    func testUpdateInfoCodable() throws {
        let info = UpdateChecker.UpdateInfo(
            updateAvailable: true,
            latestVersion: "1.1.0",
            currentVersion: "1.0.0",
            releaseURL: "https://github.com/danieljustus/symaira-operate/releases/tag/v1.1.0",
            error: nil
        )
        let data = try JSONEncoder().encode(info)
        let decoded = try JSONDecoder().decode(UpdateChecker.UpdateInfo.self, from: data)
        XCTAssertTrue(decoded.updateAvailable)
        XCTAssertEqual(decoded.latestVersion, "1.1.0")
        XCTAssertNil(decoded.error)
    }

    func testUpdateInfoErrorFieldRoundTrips() throws {
        let info = UpdateChecker.UpdateInfo(
            updateAvailable: false,
            latestVersion: nil,
            currentVersion: "1.0.0",
            releaseURL: nil,
            error: "Network error"
        )
        let data = try JSONEncoder().encode(info)
        let decoded = try JSONDecoder().decode(UpdateChecker.UpdateInfo.self, from: data)
        XCTAssertEqual(decoded.error, "Network error")
        XCTAssertFalse(decoded.updateAvailable)
    }
}

import Foundation
import XCTest
@testable import SymScopeCore

final class CacheServiceTests: XCTestCase {
    private var roots: [URL] = []

    override func tearDown() {
        for root in roots {
            try? FileManager.default.removeItem(at: root)
        }
        roots.removeAll()
        super.tearDown()
    }

    func testLocationUsesXDGCacheHomeOverride() throws {
        let home = try makeTemporaryRoot()
        let xdgCacheHome = home.appendingPathComponent("xdg-cache", isDirectory: true)
        let location = CacheService.location(
            environment: ["XDG_CACHE_HOME": xdgCacheHome.path],
            homeDirectory: home
        )

        XCTAssertEqual(location.directory.path, xdgCacheHome.appendingPathComponent("symscope").path)
        XCTAssertEqual(location.file.path, xdgCacheHome.appendingPathComponent("symscope/snapshot.json").path)
    }

    func testLocationUsesDefaultCacheHomeWhenXDGCacheHomeIsUnset() throws {
        let home = try makeTemporaryRoot()
        let location = CacheService.location(environment: [:], homeDirectory: home)

        XCTAssertEqual(location.directory.path, home.appendingPathComponent(".cache/symscope").path)
        XCTAssertEqual(location.file.path, home.appendingPathComponent(".cache/symscope/snapshot.json").path)
    }

    func testSaveCreatesPrivateCacheDirectoryAndLoadsSnapshot() throws {
        let home = try makeTemporaryRoot()
        let location = CacheService.location(environment: [:], homeDirectory: home)
        let snapshot = Snapshot(
            generatedAt: "2026-09-02T12:00:00Z",
            ports: [Port(port: 8080, protocol_: "tcp", address: "127.0.0.1", pid: 1, process: "x")]
        )

        XCTAssertFalse(FileManager.default.fileExists(atPath: location.directory.path))
        XCTAssertNil(CacheService.save(snapshot, at: location))

        let attributes = try FileManager.default.attributesOfItem(atPath: location.directory.path)
        let permissions = try XCTUnwrap((attributes[.posixPermissions] as? NSNumber)?.intValue)
        XCTAssertEqual(permissions, 0o700)
        let loaded = try XCTUnwrap(CacheService.load(from: location))
        XCTAssertEqual(loaded.generatedAt, snapshot.generatedAt)
        XCTAssertEqual(loaded.ports, snapshot.ports)
    }

    func testLoadReadsExistingLegacySnapshotWhenNewCacheIsAbsent() throws {
        let home = try makeTemporaryRoot()
        let location = CacheService.location(environment: [:], homeDirectory: home)
        let snapshot = Snapshot(
            generatedAt: "2026-09-02T12:01:00Z",
            containers: [Container(id: "abc", name: "postgres", image: "postgres:17")]
        )
        let legacyDirectory = location.legacyFile.deletingLastPathComponent()
        try FileManager.default.createDirectory(at: legacyDirectory, withIntermediateDirectories: true)
        try JSONEncoder().encode(snapshot).write(to: location.legacyFile)

        XCTAssertFalse(FileManager.default.fileExists(atPath: location.file.path))
        let loaded = try XCTUnwrap(CacheService.load(from: location))
        XCTAssertEqual(loaded.generatedAt, snapshot.generatedAt)
        XCTAssertEqual(loaded.containers, snapshot.containers)
        XCTAssertFalse(FileManager.default.fileExists(atPath: location.file.path))
    }

    func testStatsAndClearUseInjectedCacheLocation() throws {
        let home = try makeTemporaryRoot()
        let location = CacheService.location(environment: [:], homeDirectory: home)
        XCTAssertEqual(CacheService.stats(at: location).path, location.file.path)
        XCTAssertFalse(CacheService.stats(at: location).exists)

        XCTAssertNil(CacheService.save(Snapshot(ports: []), at: location))
        XCTAssertTrue(CacheService.stats(at: location).exists)
        XCTAssertNil(CacheService.clear(at: location))
        XCTAssertFalse(CacheService.stats(at: location).exists)
    }

    private func makeTemporaryRoot() throws -> URL {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("symscope-cache-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: false)
        roots.append(root)
        return root
    }
}

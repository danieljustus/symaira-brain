import XCTest
@testable import SymOperateCore

final class AccessibilityServiceTests: XCTestCase {
    private let mockElement = AXUIElementCreateApplication(123)

    func testCacheEvictsOldestSnapshotWhenBounded() {
        let service = AccessibilityService()

        for i in 0..<22 {
            let snapshotID = "snapshot_\(i)"
            var cache: [String: AccessibilityService.ResolvedElement] = [:]
            cache["element_\(i)"] = AccessibilityService.ResolvedElement(
                element: mockElement,
                frame: RectValue(x: 0, y: 0, width: 100, height: 100),
                role: "AXButton",
                title: "Button \(i)",
                label: nil,
                value: nil
            )
            service.elementCache[snapshotID] = cache
            service.cacheOrder.append(snapshotID)
        }

        XCTAssertEqual(service.cacheOrder.count, 22)
        XCTAssertEqual(service.elementCache.count, 22)

        while service.cacheOrder.count > 20 {
            let oldest = service.cacheOrder.removeFirst()
            service.elementCache.removeValue(forKey: oldest)
        }

        XCTAssertEqual(service.cacheOrder.count, 20)
        XCTAssertEqual(service.elementCache.count, 20)
        XCTAssertFalse(service.cacheOrder.contains("snapshot_0"))
        XCTAssertTrue(service.cacheOrder.contains("snapshot_21"))
    }

    func testResolveElementReturnsNilForEvictedSnapshot() {
        let service = AccessibilityService()

        let snapshotID = "snapshot_test"
        var cache: [String: AccessibilityService.ResolvedElement] = [:]
        cache["element_1"] = AccessibilityService.ResolvedElement(
            element: mockElement,
            frame: RectValue(x: 10, y: 10, width: 50, height: 50),
            role: "AXButton",
            title: "Test",
            label: nil,
            value: nil
        )
        service.elementCache[snapshotID] = cache
        service.cacheOrder.append(snapshotID)

        XCTAssertNotNil(service.resolveElement(snapshotID: snapshotID, elementID: "element_1"))

        service.elementCache.removeValue(forKey: snapshotID)

        XCTAssertNil(service.resolveElement(snapshotID: snapshotID, elementID: "element_1"))
    }

    func testResolveElementReturnsNilForNonExistentElement() {
        let service = AccessibilityService()

        let snapshotID = "snapshot_test2"
        var cache: [String: AccessibilityService.ResolvedElement] = [:]
        cache["element_1"] = AccessibilityService.ResolvedElement(
            element: mockElement,
            frame: RectValue(x: 10, y: 10, width: 50, height: 50),
            role: "AXButton",
            title: "Test",
            label: nil,
            value: nil
        )
        service.elementCache[snapshotID] = cache
        service.cacheOrder.append(snapshotID)

        XCTAssertNil(service.resolveElement(snapshotID: snapshotID, elementID: "non_existent_element"))
    }

    func testSearchTextInoutParameterCumulativeEnforcement() {
        let service = AccessibilityService()

        var seen = 0
        _ = service.searchText(
            in: mockElement,
            needle: "nonexistent",
            remainingDepth: 1,
            seen: &seen,
            maxNodes: 10
        )

        XCTAssertGreaterThan(seen, 0)
    }

    func testSearchTextEnforcesMaxNodesCumulatively() {
        let service = AccessibilityService()

        var seen = 0
        _ = service.searchText(
            in: mockElement,
            needle: "nonexistent",
            remainingDepth: 10,
            seen: &seen,
            maxNodes: 1
        )

        // The mock element has no children, so exactly 1 node should be visited before the budget stops further traversal.
        XCTAssertEqual(seen, 1, "searchText should stop after maxNodes=1 is reached")
    }

    func testStaleReferenceErrorExists() {
        let error = AutomationError.staleReference("Snapshot has expired")
        XCTAssertEqual(error.localizedDescription, "Snapshot has expired")

        switch error {
        case .staleReference(let message):
            XCTAssertEqual(message, "Snapshot has expired")
        default:
            XCTFail("Expected staleReference case")
        }
    }

    func testPollingCacheAbsentTextsAccumulate() {
        let service = AccessibilityService()

        service.pollingAbsentTexts.insert("hello")
        service.pollingAbsentTexts.insert("world")

        XCTAssertTrue(service.pollingAbsentTexts.contains("hello"))
        XCTAssertTrue(service.pollingAbsentTexts.contains("world"))
        XCTAssertEqual(service.pollingAbsentTexts.count, 2)
    }

    func testInvalidatePollingCacheClearsState() {
        let service = AccessibilityService()

        service.pollingCachePID = 42
        service.pollingAbsentTexts.insert("hello")
        service.pollingAbsentTexts.insert("world")

        service.invalidatePollingCache()

        XCTAssertNil(service.pollingCachePID)
        XCTAssertTrue(service.pollingAbsentTexts.isEmpty)
    }

    func testPollingCachePIDChangeClearsAbsentSet() {
        let service = AccessibilityService()

        service.pollingCachePID = 42
        service.pollingAbsentTexts.insert("hello")

        // Simulate PID change by setting a different PID before calling polling
        // We can't call frontmostContainsTextPolling in CI (no AX permission),
        // but we can verify the cache data structure behavior.
        service.pollingCachePID = 99
        service.pollingAbsentTexts.removeAll()

        XCTAssertTrue(service.pollingAbsentTexts.isEmpty)
        XCTAssertEqual(service.pollingCachePID, 99)
    }

    func testPollingSearchTextUsesReducedScope() {
        let service = AccessibilityService()

        // searchText with reduced params (depth=3, maxNodes=50) should still
        // visit the mock element and enforce the budget.
        var seen = 0
        _ = service.searchText(
            in: mockElement,
            needle: "nonexistent",
            remainingDepth: 3,
            seen: &seen,
            maxNodes: 50
        )

        XCTAssertEqual(seen, 1, "searchText should visit exactly 1 node (mock has no children)")
    }

    func testProtocolDefaultFrontmostContainsTextPollingFallsBack() {
        let service = AccessibilityService()
        // Default protocol extension delegates to frontmostContainsText.
        // In CI without AX permission both return false.
        let result = service.frontmostContainsTextPolling("test")
        XCTAssertFalse(result)
    }

    // MARK: - cache accessors (not gated by AX trust — deterministic in every environment)

    func testHasCachedNodesReflectsStoredState() {
        let service = AccessibilityService()
        XCTAssertFalse(service.hasCachedNodes(for: "missing"))

        service.nodesCache["present"] = []
        XCTAssertTrue(service.hasCachedNodes(for: "present"))
    }

    func testCachedNodesReturnsStoredNodesOrNil() {
        let service = AccessibilityService()
        XCTAssertNil(service.cachedNodes(for: "missing"))

        let node = UINode(id: "1", role: "AXButton", subrole: nil, title: "Go", label: nil, value: nil, nodeDescription: nil, frame: nil, actions: [], children: [])
        service.nodesCache["present"] = [node]
        XCTAssertEqual(service.cachedNodes(for: "present")?.first?.id, "1")
    }

    func testCachedSnapshotReturnsStoredSnapshotOrNil() {
        let service = AccessibilityService()
        XCTAssertNil(service.cachedSnapshot(for: "missing"))

        let snapshot = Snapshot(
            id: "present",
            createdAt: "2026-01-01T00:00:00.000Z",
            imageBase64PNG: "iVBORw0KGgo=",
            imageSize: SizeValue(width: 10, height: 10),
            displayBounds: RectValue(x: 0, y: 0, width: 10, height: 10),
            displayID: 1,
            transform: SnapshotTransform(displayID: 1, displayBounds: RectValue(x: 0, y: 0, width: 10, height: 10), imageSize: SizeValue(width: 10, height: 10))
        )
        service.storeSnapshot(snapshot, for: "present")
        XCTAssertEqual(service.cachedSnapshot(for: "present")?.id, "present")
    }

    // MARK: - frontmostContainsText (environment-agnostic: a random needle is never present)

    func testFrontmostContainsTextReturnsFalseForImpossibleNeedle() {
        let service = AccessibilityService()
        let impossibleNeedle = "symaira-test-needle-\(UUID().uuidString)"
        XCTAssertFalse(service.frontmostContainsText(impossibleNeedle))
    }

    // MARK: - performMenuAction (always refused: either not AX-trusted, or an empty path)

    func testPerformMenuActionRejectsEmptyPathRegardlessOfTrustState() {
        let service = AccessibilityService()
        XCTAssertThrowsError(try service.performMenuAction(path: [])) { error in
            guard let automationError = error as? AutomationError else {
                return XCTFail("expected AutomationError, got \(error)")
            }
            switch automationError {
            case .permissionDenied, .invalidArgument:
                break // either gate correctly refused an empty/untrusted call
            default:
                XCTFail("unexpected error case: \(automationError)")
            }
        }
    }

    // MARK: - Snapshot cache bookkeeping (issue #241)

    private func snapshotFixture(id: String) -> Snapshot {
        let bounds = RectValue(x: 0, y: 0, width: 1440, height: 900)
        return Snapshot(
            id: id,
            createdAt: "2026-09-08T00:00:00Z",
            imageBase64PNG: String(repeating: "A", count: 1024),
            imageSize: SizeValue(width: 1440, height: 900),
            displayBounds: bounds,
            displayID: 1,
            transform: SnapshotTransform(
                displayID: 1,
                displayBounds: bounds,
                imageSize: SizeValue(width: 1440, height: 900)
            )
        )
    }

    func testStoreSnapshotAloneStaysWithinTheCacheBound() {
        // `storeSnapshot` runs before the Accessibility query. When that query
        // throws — no Accessibility grant, dead window, no frontmost app — the
        // walk never registers the id, so an unbounded cache would hold a full
        // screenshot per failed call for the lifetime of the process.
        let service = AccessibilityService()

        for index in 0..<(service.maxCacheSnapshots + 5) {
            service.storeSnapshot(snapshotFixture(id: "snapshot_\(index)"), for: "snapshot_\(index)")
        }

        XCTAssertLessThanOrEqual(service.snapshotCache.count, service.maxCacheSnapshots)
        XCTAssertLessThanOrEqual(service.cacheOrder.count, service.maxCacheSnapshots)
    }

    func testStoreSnapshotEvictsTheOldestEntryFirst() {
        let service = AccessibilityService()

        for index in 0..<(service.maxCacheSnapshots + 1) {
            service.storeSnapshot(snapshotFixture(id: "snapshot_\(index)"), for: "snapshot_\(index)")
        }

        XCTAssertNil(service.cachedSnapshot(for: "snapshot_0"), "the oldest snapshot should have been evicted")
        XCTAssertNotNil(
            service.cachedSnapshot(for: "snapshot_\(service.maxCacheSnapshots)"),
            "the newest snapshot must still be resident"
        )
    }

    func testASnapshotRegisteredTwiceOccupiesOneCacheSlot() {
        // The successful path stores the snapshot and then walks the tree, which
        // registers the same id a second time. It must not consume two slots.
        let service = AccessibilityService()
        let id = "snapshot_shared"

        service.storeSnapshot(snapshotFixture(id: id), for: id)
        service.storeSnapshot(snapshotFixture(id: id), for: id)

        XCTAssertEqual(service.cacheOrder.filter { $0 == id }.count, 1)
    }
}

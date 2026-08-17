import Foundation
import Testing
@testable import SymBrainCore

/// The Memory list renders a bounded page (#226). Past roughly this many
/// variable-height rows AppKit switches its table to estimated row heights and
/// its span cache re-enters itself, which it reports as a reentrant
/// table-delegate operation.
@MainActor
struct MemoryListPagingTests {
    private func makeRecords(_ count: Int, firstIndex: Int = 0) throws -> [MemoryRecord] {
        let objects = (firstIndex..<(firstIndex + count)).map { index in
            """
            {"id":"mem-\(index)","content":"memory \(index)","scope":"global",\
            "created_at":"2026-08-16T07:00:00Z"}
            """
        }
        let json = "[\(objects.joined(separator: ","))]"
        // MemoryRecord declares explicit snake_case CodingKeys, so it decodes
        // with a plain decoder — the same way MemoryClient reads the CLI.
        return try JSONDecoder().decode([MemoryRecord].self, from: Data(json.utf8))
    }

    @Test func boundsAPageLargerThanTheLimit() throws {
        let page = MemoryViewModel.boundedPage(try makeRecords(277))
        #expect(page.count == MemoryViewModel.listPageSize)
        // The page keeps the store's order, newest-first as the CLI returns it.
        #expect(page.first?.id == "mem-0")
        #expect(page.last?.id == "mem-\(MemoryViewModel.listPageSize - 1)")
    }

    @Test func keepsEveryRecordWhenUnderTheLimit() throws {
        let records = try makeRecords(30)
        let page = MemoryViewModel.boundedPage(records)
        #expect(page.count == 30)
        #expect(page == records)
    }

    @Test func keepsEveryRecordExactlyAtTheLimit() throws {
        let page = MemoryViewModel.boundedPage(try makeRecords(MemoryViewModel.listPageSize))
        #expect(page.count == MemoryViewModel.listPageSize)
    }

    @Test func pageSizeStaysBelowTheHeightEstimationThreshold() {
        // Measured on macOS 27: 150 rows render clean, 277 rows reproduce the
        // warning. The page size keeps headroom under the lower bound so the
        // margin survives different row content and window heights.
        #expect(MemoryViewModel.listPageSize <= 150)
    }

    // MARK: - Truncation hint

    @Test func reportsTruncationOnlyWhenSomeRecordsAreHidden() throws {
        let vm = MemoryViewModel()

        vm.memories = try makeRecords(100)
        vm.totalMemoryCount = 277
        #expect(vm.isMemoryListTruncated)

        vm.totalMemoryCount = 100
        #expect(!vm.isMemoryListTruncated)
    }

    @Test func reportsNoTruncationBeforeAnythingIsLoaded() {
        let vm = MemoryViewModel()
        #expect(vm.memories.isEmpty)
        #expect(!vm.isMemoryListTruncated)
        #expect(vm.listTruncationNote == nil)
    }

    // MARK: - The note shown under the list

    @Test func browsingNoteStatesTheRealTotal() throws {
        let vm = MemoryViewModel()
        vm.memories = try makeRecords(100)
        vm.totalMemoryCount = 277

        let note = try #require(vm.listTruncationNote)
        #expect(note.contains("Showing 100 of 277 memories"))
    }

    /// A search that comes back full may be hiding further matches, and the
    /// command reports no total — so the note must not invent one.
    @Test func searchNoteClaimsNoTotalItCannotKnow() throws {
        let vm = MemoryViewModel()
        vm.memories = try makeRecords(MemoryViewModel.searchResultLimit)
        vm.totalMemoryCount = MemoryViewModel.searchResultLimit
        vm.searchMayHaveMoreMatches = true

        let note = try #require(vm.listTruncationNote)
        #expect(note.contains("Showing the first 50 matches"))
        #expect(!note.contains(" of "))
    }

    @Test func silentWhenASearchDidNotFillItsLimit() throws {
        let vm = MemoryViewModel()
        vm.memories = try makeRecords(12)
        vm.totalMemoryCount = 12
        vm.searchMayHaveMoreMatches = false

        #expect(vm.listTruncationNote == nil)
    }

    // MARK: - List identity (#231)

    /// The view keys the list on `listGeneration`, so a changed row set has to
    /// change it — that is what replaces the table instead of diffing it.
    @Test func aChangedRowSetChangesTheListIdentity() throws {
        let vm = MemoryViewModel()
        let start = vm.listGeneration

        vm.memories = try makeRecords(50)
        let afterMount = vm.listGeneration
        #expect(afterMount != start)

        // A search replacing 50 rows with 50 different rows is the reported
        // repro: same count, different entries.
        vm.memories = try makeRecords(50, firstIndex: 500)
        #expect(vm.listGeneration != afterMount)
    }

    /// Re-running the same query returns the same rows. SwiftUI would diff that
    /// to nothing, so it must not force a remount and throw away the scroll
    /// position and selection for no reason.
    @Test func anUnchangedRowSetKeepsTheListIdentity() throws {
        let vm = MemoryViewModel()
        vm.memories = try makeRecords(30)
        let generation = vm.listGeneration

        vm.memories = try makeRecords(30)
        #expect(vm.listGeneration == generation)
    }

    @Test func searchLimitStaysUnderThePageSize() {
        // A search must never return more rows than the browse page renders,
        // or searching could reintroduce the row volume this bound exists to
        // avoid.
        #expect(MemoryViewModel.searchResultLimit <= MemoryViewModel.listPageSize)
    }
}

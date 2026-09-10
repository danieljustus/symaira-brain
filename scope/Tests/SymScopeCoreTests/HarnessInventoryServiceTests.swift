import XCTest
@testable import SymScopeCore

final class HarnessInventoryServiceTests: XCTestCase {
    private func resolve(
        environment: [String: String],
        executablePaths: Set<String> = [],
        pathResult: String? = nil
    ) -> String? {
        SymBrainHarnessService.resolveSymbrain(
            environment: environment,
            isExecutableFile: { executablePaths.contains($0) },
            pathLookup: { _ in pathResult }
        )
    }

    func testExplicitSymairaBinWinsOverEveryOtherLocation() {
        let result = resolve(
            environment: [
                "SYMAIRA_BIN": "/custom/bin",
                "HOME": "/Users/example",
            ],
            executablePaths: [
                "/custom/bin/symbrain",
                "/Users/example/.symaira/bin/symbrain",
                "/opt/homebrew/bin/symbrain",
            ],
            pathResult: "/path/symbrain"
        )

        XCTAssertEqual(result, "/custom/bin/symbrain")
    }

    func testManagedUserBinIsUsedBeforePATHAndHomebrew() {
        let result = resolve(
            environment: ["HOME": "/Users/example"],
            executablePaths: [
                "/Users/example/.symaira/bin/symbrain",
                "/opt/homebrew/bin/symbrain",
            ],
            pathResult: "/path/symbrain"
        )

        XCTAssertEqual(result, "/Users/example/.symaira/bin/symbrain")
    }

    func testPATHLookupIsUsedBeforeHomebrewFallback() {
        let result = resolve(
            environment: ["PATH": "/custom/bin"],
            executablePaths: ["/opt/homebrew/bin/symbrain"],
            pathResult: "/custom/bin/symbrain"
        )

        XCTAssertEqual(result, "/custom/bin/symbrain")
    }

    func testHomebrewFallbackFindsAppleSiliconLocationAfterOtherSearchesFail() {
        let result = resolve(
            environment: ["PATH": "/empty", "HOME": "/Users/example"],
            executablePaths: ["/opt/homebrew/bin/symbrain"],
            pathResult: nil
        )

        XCTAssertEqual(result, "/opt/homebrew/bin/symbrain")
    }

    func testHomebrewFallbackUsesIntelLocationWhenAppleSiliconIsAbsent() {
        let result = resolve(
            environment: ["PATH": "/empty", "HOME": "/Users/example"],
            executablePaths: ["/usr/local/bin/symbrain"],
            pathResult: nil
        )

        XCTAssertEqual(result, "/usr/local/bin/symbrain")
    }

    func testResolutionReturnsNilWhenNoCandidateIsExecutable() {
        XCTAssertNil(
            resolve(
                environment: ["PATH": "/empty", "HOME": "/Users/example"],
                pathResult: nil
            )
        )
    }
}

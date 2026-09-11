#if os(macOS)
import Foundation
import Testing
import SymairaCLIRunner
@testable import SymBrainCore

struct CLIErrorFormatterTests {
    @Test func formatsEveryCLIRunnerErrorWithSafeDefaultAndDetail() {
        let secret = "super-secret-token-1234567890"
        let cases: [(CLIRunnerError, String, String?)] = [
            (.binaryNotFound(tool: "symvault"), "symvault", nil),
            (.executionFailed(code: 7, fullStderr: "vault locked: \(secret)"), "locked", nil),
            (.timeout(seconds: 5.5), "did not complete", "Timed out"),
            (.invalidJSON(description: "/Users/daniel/private/secret.json"), "unexpected response", nil),
            (.schemaMismatch(expected: 2, actual: 1), "out of date", "Schema mismatch"),
            (.outputTruncated(size: 1024), "too much output", "1024"),
        ]

        for (error, expectedMessage, expectedDetail) in cases {
            let formatted = formatError(error)
            #expect(formatted.message.localizedCaseInsensitiveContains(expectedMessage))
            #expect((formatted.detail != nil) == (expectedDetail != nil))
            if let expectedDetail {
                #expect(formatted.detail?.localizedCaseInsensitiveContains(expectedDetail) == true)
            }
            #expect(!formatted.message.contains(secret))
            #expect(!formatted.message.contains("/Users/"))
            #expect(!formatted.message.contains("Exit code"))
        }
    }

    @Test func redactsExecutionDiagnosticsFromBothMessageAndDetail() {
        let stderr = "cannot read entry /Users/daniel/.config/symvault/prod: token=super-secret-token"
        let formatted = formatError(CLIRunnerError.executionFailed(code: 23, fullStderr: stderr))

        #expect(formatted.message == "This entry appears to be corrupted or unreadable. Try restoring it from a backup, or check the CLI logs for details.")
        #expect(formatted.detail == nil)
        #expect(!formatted.message.contains("/Users/"))
        #expect(!formatted.message.contains("super-secret-token"))
        #expect(!formatted.message.contains("23"))
    }

    @Test func redactsArbitraryErrorsAtTheFormattingBoundary() {
        struct ArbitraryError: LocalizedError {
            var errorDescription: String? {
                "failed at /Users/daniel/private/token.json: token=super-secret-token"
            }
        }

        let formatted = formatError(ArbitraryError())

        #expect(formatted.message == "Something went wrong. Please try again or check the CLI logs for details.")
        #expect(formatted.detail == nil)
        #expect(!formatted.message.contains("/Users/"))
        #expect(!formatted.message.contains("super-secret-token"))
    }

    @Test func classifiesKnownVaultAndProfileStderrPatterns() {
        let cases: [(String, String)] = [
            ("vault locked", "vault is locked"),
            ("Vault is locked; unlock required", "vault is locked"),
            ("decryption failed: key mismatch", "Incorrect passphrase"),
            ("failed to decrypt entry", "Incorrect passphrase"),
            ("cannot read entry personal/github", "corrupted or unreadable"),
            ("corrupted entry data", "corrupted or unreadable"),
            ("unreadable entry metadata", "corrupted or unreadable"),
            ("vault not initialized", "hasn't been initialized"),
            ("profile personal not found", "selected profile"),
            ("profile \"personal\" not found", "selected profile"),
            ("binary not found: symvault", "required component"),
            ("profile \"personal\" already exists", "profile named"),
            ("some unrelated failure", "Something went wrong"),
        ]

        for (stderr, expectedMessage) in cases {
            let formatted = formatError(CLIRunnerError.executionFailed(code: 1, fullStderr: stderr))
            #expect(formatted.message.localizedCaseInsensitiveContains(expectedMessage))
            #expect(!formatted.message.contains(stderr))
            #expect(formatted.detail == nil)
        }
    }
}
#endif

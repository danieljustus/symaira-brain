// CLIErrorFormatter — maps raw CLI errors to user-friendly messages.
//
// Every view that catches a SymBrainClient or CLIRunner error should route
// through `formatError(_:)` so users see plain language instead of exit codes,
// absolute paths, and raw stderr.  The original error detail is preserved
// in `detail` for "Show Details" expansion.

#if os(macOS)
import Foundation
import SymairaCLIRunner

/// A two-tier error representation: a friendly summary for the user and an
/// optional raw detail string for troubleshooting.
public struct FriendlyCLIError: Sendable {
    /// Plain-language message suitable for a notice or alert.
    public let message: String
    /// Raw error detail (exit code, stderr, path) for expandable disclosure.
    public let detail: String?
}

/// Map any error (typically CLIRunnerError) into a user-friendly message while
/// preserving raw detail for debugging.
///
/// Usage:
/// ```swift
/// let friendly = formatError(error)
/// errorMessage = friendly.message          // shown to the user
/// errorDetail  = friendly.detail           // hidden behind "Show Details"
/// ```
public func formatError(_ error: Error) -> FriendlyCLIError {
    if let cliError = error as? CLIRunnerError {
        return formatCLIError(cliError)
    }
    return FriendlyCLIError(
        message: error.localizedDescription,
        detail: nil
    )
}

// MARK: - Private

private func formatCLIError(_ error: CLIRunnerError) -> FriendlyCLIError {
    switch error {
    case .binaryNotFound(let tool):
        return FriendlyCLIError(
            message: "The “\(tool)” command could not be found. Make sure it is installed "
                + "and available on your PATH, or set a custom path in Settings.",
            detail: nil
        )

    case .executionFailed(let code, let stderr):
        return FriendlyCLIError(
            message: friendlyMessage(from: stderr),
            detail: "Exit code \(code): \(stderr)"
        )

    case .timeout(let seconds):
        return FriendlyCLIError(
            message: "The command did not complete within \(Int(seconds)) seconds. "
                + "Please try again.",
            detail: "Timed out after \(seconds)s"
        )

    case .invalidJSON(let description):
        return FriendlyCLIError(
            message: "Received an unexpected response from the CLI. "
                + "Try restarting the app or running `brew upgrade`.",
            detail: description
        )

    case .schemaMismatch(let expected, let actual):
        return FriendlyCLIError(
            message: "The CLI version is out of date (schema \(actual), expected \(expected)). "
                + "Run `brew update && brew upgrade` to fix this.",
            detail: "Schema mismatch: expected \(expected), got \(actual)"
        )
    }
}

/// Pattern-match known stderr strings into plain-language messages.
private func friendlyMessage(from stderr: String) -> String {
    let trimmed = stderr.trimmingCharacters(in: .whitespacesAndNewlines)

    // Profile already exists — extract the name for a precise message.
    if let name = extractProfileName(from: trimmed) {
        return "A profile named “\(name)” already exists. Choose a different name."
    }

    let lower = trimmed.lowercased()

    if lower.contains("no --profile given") || lower.contains("no default_profile") {
        return "Select a profile before running this action, or configure a default profile in Settings."
    }

    if lower.contains("binary not found") || lower.contains("not found") {
        return "A required component could not be found on your system. Please check the installation."
    }

    if lower.contains("already exists") {
        return "This item already exists. Choose a different name or remove the existing one first."
    }

    if lower.contains("invalid profile") || lower.contains("profile.*not found") {
        return "The selected profile does not exist or is invalid. Please choose another one."
    }

    // Fallback — still more friendly than a raw exit code line.
    return "Something went wrong. Please try again or check the CLI logs for details."
}

private func extractProfileName(from stderr: String) -> String? {
    // Match: profile "NAME" already exists (/path/to/NAME.toml)
    // or:     profile "NAME" already exists
    let patterns = [
        try? NSRegularExpression(pattern: "profile \"([^\"]+)\" already exists"),
    ]
    for pattern in patterns.compactMap({ $0 }) {
        if let match = pattern.firstMatch(in: stderr, range: NSRange(stderr.startIndex..., in: stderr)) {
            if let range = Range(match.range(at: 1), in: stderr) {
                return String(stderr[range])
            }
        }
    }
    return nil
}
#endif

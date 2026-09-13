// CLIErrorFormatter — maps raw CLI errors to user-friendly messages.
//
// Every view that catches a SymBrainClient or CLIRunner error should route
// through `formatError(_:)` so users see plain language instead of exit codes,
// absolute paths, and raw stderr.  Diagnostics are intentionally not retained
// in the view-facing representation because module views render its fields.

#if os(macOS)
import Foundation
import SymairaCLIRunner

/// A two-tier error representation: a friendly summary for the user and an
/// optional safe detail string for troubleshooting.
public struct FriendlyCLIError: Sendable {
    /// Plain-language message suitable for a notice or alert.
    public let message: String
    /// Safe, non-sensitive detail for display alongside the message.
    public let detail: String?
}

/// Map any error (typically CLIRunnerError) into a user-friendly message while
/// without exposing subprocess diagnostics or arbitrary error descriptions.
///
/// Usage:
/// ```swift
/// let friendly = formatError(error)
/// errorMessage = friendly.message          // shown to the user
/// errorDetail  = friendly.detail           // safe display-only context
/// ```
public func formatError(_ error: Error) -> FriendlyCLIError {
    if let cliError = error as? CLIRunnerError {
        return formatCLIError(cliError)
    }
    return FriendlyCLIError(
        message: "Something went wrong. Please try again or check the CLI logs for details.",
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

    case .executionFailed(_, let stderr):
        return FriendlyCLIError(
            message: friendlyMessage(from: stderr),
            detail: nil
        )

    case .timeout(let seconds):
        return FriendlyCLIError(
            message: "The command did not complete within \(Int(seconds)) seconds. "
                + "Please try again.",
            detail: "Timed out after \(seconds)s"
        )

    case .invalidJSON:
        return FriendlyCLIError(
            message: "Received an unexpected response from the CLI. "
                + "Try restarting the app or running `brew upgrade`.",
            detail: nil
        )

    case .schemaMismatch(let expected, let actual):
        return FriendlyCLIError(
            message: "The CLI version is out of date (schema \(actual), expected \(expected)). "
                + "Run `brew update && brew upgrade` to fix this.",
            detail: "Schema mismatch"
        )

    case .outputTruncated(let size):
        return FriendlyCLIError(
            message: "The command produced too much output (over \(size) bytes). "
                + "Try narrowing the scope of the operation.",
            detail: "Output truncated at \(size) bytes"
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

    // Keep profile errors ahead of the general "not found" classifier. The
    // previous literal `profile.*not found` check never matched real output.
    if lower.contains("profile ") && lower.contains(" not found") {
        return "The selected profile does not exist or is invalid. Please choose another one."
    }

    if lower.contains("binary not found") || lower.contains("not found") {
        return "A required component could not be found on your system. Please check the installation."
    }

    if lower.contains("already exists") {
        return "This item already exists. Choose a different name or remove the existing one first."
    }

    if lower.contains("invalid profile") {
        return "The selected profile does not exist or is invalid. Please choose another one."
    }

    // A single entry's ciphertext or metadata could not be read back — checked
    // before "decryption failed" below, since a corrupted entry's underlying
    // cause is often itself a decryption failure, but the "cannot read entry"
    // wrapping means the vault unlocked fine and only this one entry is bad.
    if lower.contains("cannot read entry")
        || lower.contains("corrupt entry")
        || lower.contains("corrupted entry")
        || lower.contains("unreadable entry") {
        return "This entry appears to be corrupted or unreadable. Try restoring it from a backup, "
            + "or check the CLI logs for details."
    }

    // Wrong passphrase (or any other failure to decrypt with the vault's
    // identity) while unlocking the vault itself.
    if lower.contains("decryption failed") || lower.contains("failed to decrypt") {
        return "Incorrect passphrase. Please check your passphrase and try again."
    }

    if lower.contains("vault locked") || lower.contains("vault is locked") {
        return "The vault is locked. Run `symvault unlock` to unlock it before trying again."
    }

    if lower.contains("vault not initialized") {
        return "This vault hasn't been initialized yet. Run `symvault init` to set one up."
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

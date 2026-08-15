import Foundation

/// The version of the SymBrain **app bundle** itself.
///
/// This is deliberately distinct from `VersionInfo`, which reports the version
/// of the `symbrain` **CLI** the app talks to. The two are released separately
/// and routinely differ, so any surface showing a version must say which of the
/// two it means.
public enum AppVersionInfo {
    /// Formats a marketing version and build number for display,
    /// e.g. `"0.6.0 (1)"`.
    ///
    /// Missing or blank components are dropped rather than rendered as empty
    /// parentheses; when nothing usable is present the result is `"unknown"`.
    public static func displayString(short: String?, build: String?) -> String {
        let shortVersion = short?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let buildVersion = build?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""

        switch (shortVersion.isEmpty, buildVersion.isEmpty) {
        case (true, true):
            return "unknown"
        case (false, true):
            return shortVersion
        case (true, false):
            return "build \(buildVersion)"
        case (false, false):
            return "\(shortVersion) (\(buildVersion))"
        }
    }

    /// The running app bundle's version, formatted for display.
    public static func current(bundle: Bundle = .main) -> String {
        displayString(
            short: bundle.infoDictionary?["CFBundleShortVersionString"] as? String,
            build: bundle.infoDictionary?["CFBundleVersion"] as? String
        )
    }
}

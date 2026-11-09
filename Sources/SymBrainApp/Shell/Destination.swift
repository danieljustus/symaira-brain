import SymairaTheme

/// The seven top-level destinations of the workspace sidebar, in menu order.
///
/// Settings is deliberately absent: it lives in its own native window (⌘,).
/// Broker activity for every state core is one destination instead of a tab
/// on each core screen, and the optional Operate/Scope modules are reached
/// from the Overview, which keeps the sidebar within seven entries.
enum Destination: String, CaseIterable, Identifiable, Hashable {
    case overview
    case harnesses
    case profiles
    case memory
    case skills
    case credentials
    case activity

    var id: String { rawValue }

    var title: String {
        switch self {
        case .overview: "Overview"
        case .harnesses: "Harnesses"
        case .profiles: "Profiles"
        case .memory: "Memory"
        case .skills: "Skills"
        case .credentials: "Credentials"
        case .activity: "Activity"
        }
    }

    var icon: SymairaIcon {
        switch self {
        case .overview: .dashboard
        case .harnesses: .terminal
        case .profiles: .shieldCheck
        case .memory: .productBrain
        case .skills: .library
        case .credentials: .productVault
        case .activity: .history
        }
    }

    /// Ordered entries for `SymairaCommands`, which binds ⌘1 to ⌘7.
    static var commandEntries: [(value: Destination, label: String)] {
        allCases.map { (value: $0, label: $0.title) }
    }
}

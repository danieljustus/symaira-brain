import Foundation

public struct UIElementPredicate: Codable, Sendable, Equatable {
    public let role: String?
    public let title: String?
    public let label: String?
    public let value: String?
    public let subrole: String?
    public let actions: [String]?
    public let text: String?
    public let app: String?
    public let window: String?
    public let enabled: Bool?

    public init(
        role: String? = nil,
        title: String? = nil,
        label: String? = nil,
        value: String? = nil,
        subrole: String? = nil,
        actions: [String]? = nil,
        text: String? = nil,
        app: String? = nil,
        window: String? = nil,
        enabled: Bool? = nil
    ) {
        self.role = role
        self.title = title
        self.label = label
        self.value = value
        self.subrole = subrole
        self.actions = actions
        self.text = text
        self.app = app
        self.window = window
        self.enabled = enabled
    }

    /// Matches the node-scoped portion of a predicate. `app` and `window`
    /// are evaluated by `ActionConditionEvaluator` against the observation's
    /// app/window metadata, not against individual nodes.
    public func matches(node: UINode) -> Bool {
        if let role, !matchField(node.role, pattern: role) { return false }
        if let title, !matchField(node.title, pattern: title) { return false }
        if let label, !matchField(node.label, pattern: label) { return false }
        if let value, !matchField(node.value, pattern: value) { return false }
        if let subrole, !matchField(node.subrole, pattern: subrole) { return false }
        if let actions {
            let nodeActions = Set(node.actions)
            for action in actions where !nodeActions.contains(action) { return false }
        }
        if let text {
            let fields = [node.title, node.label, node.value, node.nodeDescription]
            guard fields.compactMap({ $0 }).contains(where: { matchField($0, pattern: text) }) else {
                return false
            }
        }
        if let enabled, node.enabled != enabled { return false }
        return true
    }

    /// Maximum regex pattern length to prevent catastrophic backtracking (ReDoS).
    private static let maxRegexPatternLength = 200

    func matchField(_ field: String?, pattern: String) -> Bool {
        guard let field else { return false }
        if pattern.hasPrefix("/") && pattern.hasSuffix("/") {
            let regex = String(pattern.dropFirst().dropLast())
            guard !regex.isEmpty,
                  regex.count <= Self.maxRegexPatternLength,
                  Self.isSafeRegex(regex)
            else {
                // Fallback to literal substring match for invalid/unsafe patterns
                return field.localizedCaseInsensitiveContains(regex)
            }
            return field.range(of: regex, options: .regularExpression) != nil
        }
        return field.localizedCaseInsensitiveContains(pattern)
    }

    /// Rejects patterns with nested quantifiers (e.g., `(a+)+`) that cause ReDoS.
    private static func isSafeRegex(_ pattern: String) -> Bool {
        // Check for nested quantifiers: quantifier inside a group followed by another quantifier
        let nestedQuantifierPattern = #"(\([^)]*[+*][^)]*\))[+*?{]"#
        if pattern.range(of: nestedQuantifierPattern, options: .regularExpression) != nil {
            return false
        }
        // Check for backreferences or alternation with quantifiers
        let backreferencePattern = #"\\[1-9]"#
        if pattern.range(of: backreferencePattern, options: .regularExpression) != nil {
            return false
        }
        return true
    }
}

public struct UIQueryService: UIQueryServiceProtocol {
    public init() {}

    public func findNodes(in nodes: [UINode], predicate: UIElementPredicate) -> [UINode] {
        var results: [UINode] = []
        for node in nodes {
            if predicate.matches(node: node) {
                results.append(node)
            }
            results.append(contentsOf: findNodes(in: node.children, predicate: predicate))
        }
        return results
    }

    public func findFirstNode(in nodes: [UINode], predicate: UIElementPredicate) -> UINode? {
        for node in nodes {
            if predicate.matches(node: node) { return node }
            if let found = findFirstNode(in: node.children, predicate: predicate) { return found }
        }
        return nil
    }
}

import Foundation

/// Redacts credential-shaped substrings from text and JSON history values.
public enum SecretRedactor {
    /// The redaction marker replacing every match.
    public static let placeholder = "<redacted>"

    /// Credential-shaped patterns shared by all history consumers.
    /// Compiled with `try?` + `compactMap`: a broken pattern must never crash
    /// the service; it simply stops matching.
    private static let patterns: [NSRegularExpression] = [
        // Redaction markers already applied elsewhere.
        #"«redacted:[^»]*»"#,
        // sk-... (OpenAI/OpenRouter-style), ghp_/github_pat_ (GitHub),
        // glpat- (GitLab), xox[baprs]- (Slack), AIza... (Google)
        #"(sk-[A-Za-z0-9_-]{8,}|ghp_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|glpat-[A-Za-z0-9_-]{8,}|xox[baprs]-[A-Za-z0-9-]{8,}|AIza[0-9A-Za-z_-]{20,})"#,
        // Authorization / Bearer headers and quoted secrets. Both the
        // `Bearer <token>` (space-separated) and `key=<token>` shapes.
        #"(?i)(authorization|bearer|api[_-]?key|token)\s*[:=]\s*("[^"]+"|'[^']+'|[A-Za-z0-9._-]{12,})"#,
        #"(?i)\bbearer\s+[A-Za-z0-9._-]{12,}"#,
        // JWT-shaped segments
        #"eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}"#,
    ].compactMap { try? NSRegularExpression(pattern: $0) }

    /// Replaces every credential-shaped substring with `placeholder`.
    public static func redact(_ text: String) -> String {
        var result = text
        for pattern in patterns {
            result = pattern.stringByReplacingMatches(
                in: result,
                range: NSRange(result.startIndex..., in: result),
                withTemplate: placeholder
            )
        }
        return result
    }

    /// Recursively redacts strings while preserving non-string scalar values.
    public static func redact(_ value: HistoryJSONValue) -> HistoryJSONValue {
        switch value {
        case .string(let text):
            return .string(redact(text))
        case .object(let object):
            return .object(object.mapValues { redact($0) })
        case .array(let array):
            return .array(array.map { redact($0) })
        case .number, .integer, .bool, .null:
            return value
        }
    }
}

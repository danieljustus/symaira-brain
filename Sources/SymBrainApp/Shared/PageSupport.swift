import AppKit
import SwiftUI
import SymairaTheme

/// Product-side glue between the view models and the shared page scaffold.
/// Everything visual comes from `SymairaTheme`; these helpers only compose it.
enum PageLoad {
    /// Maps a view model's loading flags to the shared page state.
    ///
    /// `hasLoaded` stays false until the first load finished, so the page
    /// shows its loading text instead of an empty state before any data
    /// arrived, and a failed load never renders as an empty list (STATE-8).
    static func state<Value>(
        hasLoaded: Bool, error: String?, value: Value
    ) -> SymairaPageLoadState<Value> {
        if let error { return .failed(error) }
        if !hasLoaded { return .loading }
        return .loaded(value)
    }
}

/// Wide content budget for dashboards and tables (LAYOUT-2).
enum PageWidth {
    static let dashboard: CGFloat = 1080
    static let table: CGFloat = 1280
}

/// A shell command the user is asked to run, with a copy control.
struct CommandLineView: View {
    let command: String

    init(_ command: String) {
        self.command = command
    }

    var body: some View {
        HStack(spacing: SymairaSpacing.small) {
            Text(verbatim: command)
                .symairaText(.mono)
                .textSelection(.enabled)
                .lineLimit(2)
                .frame(maxWidth: .infinity, alignment: .leading)
            SymairaIconButton("Copy Command", icon: .copy, size: 16) {
                Pasteboard.copy(command)
            }
        }
        .padding(.horizontal, SymairaSpacing.medium)
        .padding(.vertical, SymairaSpacing.small)
        .background(SymairaTokens.raised, in: RoundedRectangle(cornerRadius: SymairaRadius.control, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: SymairaRadius.control, style: .continuous)
                .stroke(SymairaTokens.line, lineWidth: 1)
        )
    }
}

/// One label/value line in a detail card.
struct DetailRow: View {
    let label: String
    let value: String
    var monospaced = false

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: SymairaSpacing.medium) {
            Text(label)
                .symairaText(.secondary)
                .frame(width: 120, alignment: .leading)
            Text(verbatim: value)
                .symairaText(monospaced ? .monoSmall : .body)
                .foregroundStyle(SymairaTokens.ink)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityElement(children: .combine)
    }
}

/// A dashboard summary tile: the value in the stat role, a label beneath.
struct StatTile: View {
    let value: String
    let label: String
    var detail: String?

    var body: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
            Text(verbatim: value)
                .font(SymairaTypography.stat)
                .foregroundStyle(SymairaTokens.ink)
            Text(label)
                .symairaText(.secondary)
            Text(detail ?? " ")
                .symairaText(.caption)
                .lineLimit(1)
                .accessibilityHidden(detail == nil)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .symairaCard()
        .topoProtected(hill: true)
        .accessibilityElement(children: .combine)
    }
}

/// A titled card section for read-only content that is not a form.
struct CardSection<Content: View, Accessory: View>: View {
    let title: String
    let accessory: Accessory
    let content: Content

    init(_ title: String, @ViewBuilder accessory: () -> Accessory, @ViewBuilder content: () -> Content) {
        self.title = title
        self.accessory = accessory()
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.small) {
            HStack(alignment: .firstTextBaseline) {
                Text(title.uppercased())
                    .symairaText(.sectionLabel)
                    .accessibilityAddTraits(.isHeader)
                    .topoProtected()
                Spacer()
                accessory
            }
            VStack(alignment: .leading, spacing: SymairaSpacing.small) { content }
                .frame(maxWidth: .infinity, alignment: .leading)
                .symairaCard()
                .topoProtected(hill: true)
        }
    }
}

extension CardSection where Accessory == EmptyView {
    init(_ title: String, @ViewBuilder content: () -> Content) {
        self.init(title, accessory: { EmptyView() }, content: content)
    }
}

/// Text output from a CLI run, shown verbatim on a raised surface.
struct OutputBlock: View {
    let text: String

    var body: some View {
        Text(verbatim: text)
            .symairaText(.monoSmall)
            .foregroundStyle(SymairaTokens.ink)
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(SymairaSpacing.medium)
            .background(SymairaTokens.raised, in: RoundedRectangle(cornerRadius: SymairaRadius.control, style: .continuous))
    }
}

enum Pasteboard {
    static func copy(_ value: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(value, forType: .string)
    }
}

/// Status words from the CLI mapped to shared tones; the word stays visible.
enum StatusTone {
    static func call(_ status: String) -> SymairaTone {
        switch status.lowercased() {
        case "ok": .positive
        case "error", "denied": .critical
        case "timeout": .warning
        default: .neutral
        }
    }

    static func write(_ status: String) -> SymairaTone {
        switch status.lowercased() {
        case "created", "updated", "in-sync", "verified", "ok": .positive
        case "skipped", "stale", "harness-changed": .warning
        case "error", "conflict", "orphaned": .critical
        default: .neutral
        }
    }
}

/// Horizontal flow of badges that wraps onto new lines.
struct FlowLayout: Layout {
    var spacing: CGFloat = SymairaSpacing.xSmall

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        arrange(width: proposal.width ?? .infinity, subviews: subviews).size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let result = arrange(width: bounds.width, subviews: subviews)
        for (index, origin) in result.origins.enumerated() {
            subviews[index].place(
                at: CGPoint(x: bounds.minX + origin.x, y: bounds.minY + origin.y), proposal: .unspecified)
        }
    }

    private func arrange(width: CGFloat, subviews: Subviews) -> (size: CGSize, origins: [CGPoint]) {
        var origins: [CGPoint] = []
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0
        var usedWidth: CGFloat = 0
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            if x + size.width > width, x > 0 {
                x = 0
                y += rowHeight + spacing
                rowHeight = 0
            }
            origins.append(CGPoint(x: x, y: y))
            rowHeight = max(rowHeight, size.height)
            x += size.width + spacing
            usedWidth = max(usedWidth, x - spacing)
        }
        return (CGSize(width: usedWidth, height: y + rowHeight), origins)
    }
}

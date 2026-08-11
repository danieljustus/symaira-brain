import SwiftUI
import SymairaTheme

/// An inline notice for the Memory and Vault module screens.
///
/// This deliberately does not use `SymairaNotice` from symaira-appkit: that
/// component lays its message out as `Text(...).fixedSize(horizontal: false,
/// vertical: true)` next to a `Spacer(minLength: 0)`, and on macOS 26 a
/// message long enough to wrap inside the NavigationSplitView detail column
/// leaves the window blank and unresponsive.  Error text wraps by nature, so
/// the module screens render notices themselves, with the message given an
/// explicit width to lay out in.
struct ModuleNotice: View {
    let title: String?
    let message: String
    let tone: SymairaTone

    init(title: String? = nil, message: String, tone: SymairaTone = .informative) {
        self.title = title
        self.message = message
        self.tone = tone
    }

    var body: some View {
        HStack(alignment: .top, spacing: SymairaSpacing.medium) {
            Image(systemName: tone.systemImage)
                .foregroundStyle(tone.foreground)
                .imageScale(.large)
                .accessibilityHidden(true)

            VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
                if let title {
                    Text(title)
                        .font(.callout.weight(.semibold))
                        .foregroundStyle(SymairaTheme.textPrimary)
                }
                Text(message)
                    .font(.callout)
                    .foregroundStyle(SymairaTheme.textSecondary)
                    .multilineTextAlignment(.leading)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .textSelection(.enabled)
            }
        }
        .padding(SymairaSpacing.medium)
        .background(
            tone.foreground.opacity(0.10),
            in: RoundedRectangle(cornerRadius: SymairaRadius.control, style: .continuous)
        )
        .overlay(
            RoundedRectangle(cornerRadius: SymairaRadius.control, style: .continuous)
                .stroke(tone.foreground.opacity(0.30), lineWidth: 1)
        )
        .accessibilityElement(children: .combine)
    }
}

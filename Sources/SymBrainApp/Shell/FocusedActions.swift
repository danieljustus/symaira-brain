import SwiftUI

/// A page action that the app menu can trigger while the page is visible.
///
/// Pages publish their refresh and their single primary action through
/// focused scene values, so ⌘R and ⌘N always act on the page in front
/// without the menu knowing any page type.
struct PageAction {
    let title: String
    let isEnabled: Bool
    let perform: () -> Void

    init(_ title: String, isEnabled: Bool = true, perform: @escaping () -> Void) {
        self.title = title
        self.isEnabled = isEnabled
        self.perform = perform
    }
}

private struct DestinationKey: FocusedValueKey {
    typealias Value = Binding<Destination>
}

private struct RefreshActionKey: FocusedValueKey {
    typealias Value = PageAction
}

private struct PrimaryActionKey: FocusedValueKey {
    typealias Value = PageAction
}

extension FocusedValues {
    var brainDestination: Binding<Destination>? {
        get { self[DestinationKey.self] }
        set { self[DestinationKey.self] = newValue }
    }

    var brainRefresh: PageAction? {
        get { self[RefreshActionKey.self] }
        set { self[RefreshActionKey.self] = newValue }
    }

    var brainPrimary: PageAction? {
        get { self[PrimaryActionKey.self] }
        set { self[PrimaryActionKey.self] = newValue }
    }
}

extension View {
    /// Publishes the page's refresh for ⌘R.
    func pageRefresh(_ perform: @escaping () -> Void) -> some View {
        focusedSceneValue(\.brainRefresh, PageAction("Refresh", perform: perform))
    }

    /// Publishes the page's primary action for ⌘N. Pass `nil` when the page
    /// currently has no primary action, so the menu item disables itself.
    func pagePrimaryAction(_ action: PageAction?) -> some View {
        focusedSceneValue(\.brainPrimary, action)
    }
}

/// Menu commands for the page in front: refresh and its primary action.
///
/// ⌘N belongs to the page's primary action (New Profile, New Memory, …), so
/// the default New Window item is replaced and moves to ⌘⇧N.
struct PageCommands: Commands {
    static let windowID = "main"

    @FocusedValue(\.brainRefresh) private var refresh
    @FocusedValue(\.brainPrimary) private var primary
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button(primary?.title ?? "New Item") { primary?.perform() }
                .keyboardShortcut("n", modifiers: .command)
                .disabled(primary == nil || primary?.isEnabled == false)
            Button("New Window") { openWindow(id: Self.windowID) }
                .keyboardShortcut("n", modifiers: [.command, .shift])
        }
        CommandGroup(after: .toolbar) {
            Button("Refresh") { refresh?.perform() }
                .keyboardShortcut("r", modifiers: .command)
                .disabled(refresh == nil)
        }
    }
}

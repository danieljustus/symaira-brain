#!/usr/bin/env swift

// Prints the accessible name of every button in a running app, the way
// VoiceOver reads it.
//
// Why this exists (#227): SwiftUI publishes a button's name in
// AXDescription / AXAttributedDescription, not in AXTitle. AppleScript and
// System Events cannot marshal an attributed string, so they report every
// SwiftUI button as an unnamed "button" — including buttons that are labelled
// correctly and that VoiceOver announces correctly. Measuring accessibility
// that way produces false defects. #227 was filed, investigated and blocked on
// exactly that artefact, and it proposed dropping the system Liquid Glass
// button styles across every Symaira app to fix a problem that was not there.
//
// This tool asks through the real accessibility API, which is the same source
// VoiceOver reads, so its answer is the one that counts.
//
// Usage:
//   swift scripts/ax-button-names.swift <pid>
//   swift scripts/ax-button-names.swift "$(pgrep -f 'SymBrain.app/Contents/MacOS/SymBrain' | head -1)"
//
// The target app's window has to be active while this runs. SwiftUI builds its
// accessibility tree on demand and lets it collapse again, so querying a
// backgrounded window reads nothing at all — which the tool reports as a
// failure rather than as a clean result.
//
// The calling process needs Accessibility permission
// (System Settings → Privacy & Security → Accessibility). Run it from a
// terminal that has it; a sandboxed shell reports NOT_TRUSTED and exits 2.
//
// Buttons that legitimately have no name of their own are system chrome —
// scrollbar arrows and the window's close/minimize/zoom controls. They carry a
// subrole instead, which is printed, and VoiceOver names them from it.

import ApplicationServices
import Foundation

guard CommandLine.arguments.count > 1, let pid = Int32(CommandLine.arguments[1]) else {
    FileHandle.standardError.write(Data("usage: ax-button-names.swift <pid>\n".utf8))
    exit(64)
}

guard AXIsProcessTrusted() else {
    FileHandle.standardError.write(Data(
        "NOT_TRUSTED: grant this terminal Accessibility permission first.\n".utf8
    ))
    exit(2)
}

func attribute(_ element: AXUIElement, _ name: String) -> CFTypeRef? {
    var value: CFTypeRef?
    return AXUIElementCopyAttributeValue(element, name as CFString, &value) == .success
        ? value
        : nil
}

/// The name VoiceOver announces, checked in the order SwiftUI populates it.
func accessibleName(of element: AXUIElement) -> String? {
    if let description = attribute(element, kAXDescriptionAttribute) as? String,
       !description.isEmpty {
        return description
    }
    if let attributed = attribute(element, "AXAttributedDescription") as? NSAttributedString,
       !attributed.string.isEmpty {
        return attributed.string
    }
    if let title = attribute(element, kAXTitleAttribute) as? String, !title.isEmpty {
        return title
    }
    return nil
}

var unnamed = 0
var buttons = 0

func walk(_ element: AXUIElement, depth: Int) {
    guard depth < 40 else { return }

    if attribute(element, kAXRoleAttribute) as? String == kAXButtonRole {
        buttons += 1
        let subrole = attribute(element, kAXSubroleAttribute) as? String
        if let name = accessibleName(of: element) {
            print("named   \(name)")
        } else {
            print("UNNAMED subrole=\(subrole ?? "none")")
            // System chrome has no name of its own by design.
            if subrole == nil { unnamed += 1 }
        }
    }

    for child in (attribute(element, kAXChildrenAttribute) as? [AXUIElement] ?? []) {
        walk(child, depth: depth + 1)
    }
}

walk(AXUIElementCreateApplication(pid), depth: 0)

// Finding nothing is never good news: it means the queries were refused, not
// that the app is clean. AXIsProcessTrusted() can pass while the individual
// attribute reads still fail — notably when this file is run through the
// `swift` interpreter, whose process is not the one holding the permission.
// Compile it instead: `swiftc scripts/ax-button-names.swift -o /tmp/axnames`.
guard buttons > 0 else {
    let message = "NO_BUTTONS_READ: the accessibility queries returned nothing. "
        + "This is a permission or tooling failure, not a passing result.\n"
    FileHandle.standardError.write(Data(message.utf8))
    exit(3)
}

if unnamed > 0 {
    print("\n\(unnamed) button(s) without a name and without a subrole — these are real gaps.")
    exit(1)
}
print("\nEvery app-authored button exposes a name.")

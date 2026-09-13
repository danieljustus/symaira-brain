import AppKit
import CoreGraphics
import Foundation

public final class AutomationController {
    private let permissions: any PermissionServiceProtocol
    private let screen: any ScreenServiceProtocol
    private let apps: any AppServiceProtocol
    let accessibility: any AccessibilityServiceProtocol
    private let input: any InputServiceProtocol
    private let ocr: any OCRServiceProtocol
    private let queryService: any UIQueryServiceProtocol
    public var actionPolicy: ActionPolicy
    private let history: any HistoryServiceProtocol

    public init(
        permissions: any PermissionServiceProtocol = PermissionService(),
        screen: any ScreenServiceProtocol = ScreenService(),
        apps: any AppServiceProtocol = AppService(),
        accessibility: any AccessibilityServiceProtocol = AccessibilityService(),
        input: any InputServiceProtocol = InputService(),
        ocr: any OCRServiceProtocol = OCRService(),
        queryService: any UIQueryServiceProtocol = UIQueryService(),
        actionPolicy: ActionPolicy = ActionPolicy(),
        history: any HistoryServiceProtocol = HistoryService()
    ) {
        self.permissions = permissions
        self.screen = screen
        self.apps = apps
        self.accessibility = accessibility
        self.input = input
        self.ocr = ocr
        self.queryService = queryService
        self.actionPolicy = actionPolicy
        self.history = history
    }

    public func permissionsStatus() -> PermissionSnapshot {
        permissions.status()
    }

    @discardableResult
    public func requestAccessibilityPermission() -> Bool {
        permissions.requestAccessibilityPermission()
    }

    @discardableResult
    public func requestScreenRecordingPermission() -> Bool {
        permissions.requestScreenRecordingPermission()
    }

    public func listApps() -> [AppInfo] {
        apps.listApps()
    }

    public func listWindows() -> [WindowInfo] {
        apps.listWindows()
    }

    public func listDisplays() -> [DisplayInfo] {
        screen.listDisplays()
    }

    public func snapshot(displayID: UInt32? = nil, windowID: Int? = nil, target: TargetIdentity? = nil) throws -> Snapshot {
        try requirePermission(.capture, for: "snapshot")
        if let resolved = try resolveWindowTarget(target, windowID: windowID) {
            return try screen.captureWindow(windowID: resolved.window!.windowID)
        }
        if let displayID {
            return try screen.captureDisplay(displayID: displayID)
        }
        return try screen.captureMainDisplay()
    }

    public func queryUI(maxDepth: Int = 4, maxNodes: Int = 200, displayID: UInt32? = nil, windowID: Int? = nil, target: TargetIdentity? = nil) throws -> UIQueryResult {
        let resolved = try resolveWindowTarget(target, windowID: windowID)
        let snapshot = try self.snapshot(displayID: displayID, windowID: resolved?.window?.windowID)
        accessibility.storeSnapshot(snapshot, for: snapshot.id)
        let nodes: [UINode]
        let app: AppInfo?
        if let resolved, let window = resolved.window {
            nodes = try accessibility.queryUI(
                snapshotID: snapshot.id,
                processID: resolved.app.processIdentifier,
                windowID: window.windowID,
                windowBounds: window.bounds,
                maxDepth: maxDepth,
                maxNodes: maxNodes
            )
            app = resolved.app
        } else {
            nodes = try accessibility.queryFrontmostUI(snapshotID: snapshot.id, maxDepth: maxDepth, maxNodes: maxNodes)
            app = apps.frontmostApp()
        }
        return UIQueryResult(snapshot: snapshot, app: app, nodes: nodes)
    }

    public func queryUIWithOCR(maxDepth: Int = 4, maxNodes: Int = 200, displayID: UInt32? = nil, windowID: Int? = nil, target: TargetIdentity? = nil) throws -> UIQueryResultWithOCR {
        let resolved = try resolveWindowTarget(target, windowID: windowID)
        let snapshot = try self.snapshot(displayID: displayID, windowID: resolved?.window?.windowID)
        accessibility.storeSnapshot(snapshot, for: snapshot.id)
        let nodes: [UINode]
        let app: AppInfo?
        if let resolved, let window = resolved.window {
            nodes = try accessibility.queryUI(
                snapshotID: snapshot.id,
                processID: resolved.app.processIdentifier,
                windowID: window.windowID,
                windowBounds: window.bounds,
                maxDepth: maxDepth,
                maxNodes: maxNodes
            )
            app = resolved.app
        } else {
            nodes = try accessibility.queryFrontmostUI(snapshotID: snapshot.id, maxDepth: maxDepth, maxNodes: maxNodes)
            app = apps.frontmostApp()
        }
        let isWeak = ocr.isAXTreeWeak(nodeCount: countNodes(nodes))

        var ocrResult: OCRResult?
        if isWeak, let imageData = Data(base64Encoded: snapshot.imageBase64PNG) {
            if let bitmapRep = NSBitmapImageRep(data: imageData),
               let cgImage = bitmapRep.cgImage {
                ocrResult = ocr.recognizeText(in: cgImage)
            }
        }

        return UIQueryResultWithOCR(
            snapshot: snapshot,
            app: app,
            nodes: nodes,
            ocrResult: ocrResult,
            axTreeWeak: isWeak
        )
    }

    private func countNodes(_ nodes: [UINode]) -> Int {
        var count = 0
        var stack = nodes
        while !stack.isEmpty {
            let node = stack.removeLast()
            count += 1
            stack.append(contentsOf: node.children)
        }
        return count
    }

    public func findUI(
        predicate: UIElementPredicate,
        snapshotID: String? = nil,
        maxDepth: Int = 4,
        maxNodes: Int = 200,
        displayID: UInt32? = nil,
        windowID: Int? = nil,
        target: TargetIdentity? = nil
    ) throws -> UIQueryResult {
        try requirePermission(.capture, for: "find_ui")
        let resolvedTarget = try resolveWindowTarget(target, windowID: windowID)
        let target = resolvedTarget?.window
        let queryResult: UIQueryResult
        if let snapshotID, accessibility.hasCachedNodes(for: snapshotID),
           let cachedNodes = accessibility.cachedNodes(for: snapshotID),
           let cachedSnapshot = accessibility.cachedSnapshot(for: snapshotID),
           cachedSnapshotMatchesTarget(cachedSnapshot, target: target) {
            // Reuse only a snapshot captured for the same target scope. A
            // target window snapshot must carry both its window and owner PID;
            // an unscoped/frontmost snapshot is never valid for a window query.
            let matched = queryService.findNodes(in: cachedNodes, predicate: predicate)
            return UIQueryResult(snapshot: cachedSnapshot, app: resolvedTarget?.app ?? target.map(appInfo(for:)), nodes: matched)
        } else {
            queryResult = try queryUI(
                maxDepth: maxDepth,
                maxNodes: maxNodes,
                displayID: displayID,
                windowID: resolvedTarget?.window?.windowID
            )
        }
        let matched = queryService.findNodes(in: queryResult.nodes, predicate: predicate)
        return UIQueryResult(snapshot: queryResult.snapshot, app: queryResult.app, nodes: matched)
    }

    private func cachedSnapshotMatchesTarget(_ snapshot: Snapshot, target: WindowInfo?) -> Bool {
        guard let target else {
            return snapshot.windowID == nil && snapshot.windowOwnerPID == nil
        }
        return snapshot.windowID == target.windowID && snapshot.windowOwnerPID == target.ownerPID
    }

    private func windowInfo(for windowID: Int) throws -> WindowInfo {
        guard let window = apps.listWindows().first(where: { $0.windowID == windowID }) else {
            throw AutomationError.notFound("Window \(windowID) was not found or is no longer available.")
        }
        guard window.ownerPID > 0 else {
            throw AutomationError.unavailable("Window \(windowID) has no resolvable owning process.")
        }
        return window
    }

    private func resolveWindowTarget(_ target: TargetIdentity?, windowID: Int?) throws -> ResolvedTarget? {
        var requested = target ?? TargetIdentity()
        if let windowID {
            if let requestedWindowID = requested.windowID, requestedWindowID != windowID {
                throw AutomationError.targetMismatch("Conflicting window identities were provided (\(requestedWindowID) and \(windowID)).")
            }
            requested = TargetIdentity(
                processID: requested.processID,
                bundleID: requested.bundleID,
                appName: requested.appName,
                windowID: windowID,
                windowTitle: requested.windowTitle,
                displayID: requested.displayID,
                spaceID: requested.spaceID
            )
        }
        guard requested.isExplicit else { return nil }
        let resolved = try apps.resolveTarget(requested)
        guard resolved.window != nil else {
            throw AutomationError.notFound("The requested target has no resolvable window.")
        }
        return resolved
    }

    private func resolveActionTarget(_ target: TargetIdentity?, snapshotID: String? = nil) throws -> ResolvedTarget? {
        guard let target, target.isExplicit else { return nil }
        let resolved = try apps.resolveTarget(target)
        if let snapshotID {
            guard let snapshot = accessibility.cachedSnapshot(for: snapshotID) else {
                throw AutomationError.staleReference("The referenced snapshot has expired or is unavailable for the requested target.")
            }
            if let window = resolved.window,
               (snapshot.windowID != window.windowID || snapshot.windowOwnerPID != resolved.app.processIdentifier) {
                throw AutomationError.targetMismatch("The requested target does not match snapshot '\(snapshotID)'.")
            }
        }
        return resolved
    }

    private func actionTarget(for resolved: ResolvedTarget?) -> ActionTarget? {
        guard let resolved else { return nil }
        return ActionTarget(
            requestedPID: resolved.identity.processID,
            requestedBundleID: resolved.identity.bundleID,
            requestedAppName: resolved.identity.appName,
            requestedWindowID: resolved.identity.windowID,
            requestedWindowTitle: resolved.identity.windowTitle,
            requestedDisplayID: resolved.identity.displayID,
            requestedSpaceID: resolved.identity.spaceID
        )
    }

    private func appInfo(for window: WindowInfo) -> AppInfo {
        apps.listApps().first(where: { $0.processIdentifier == window.ownerPID })
            ?? AppInfo(
                localizedName: window.ownerName,
                bundleIdentifier: nil,
                processIdentifier: window.ownerPID,
                isActive: false
            )
    }

    private struct VerificationDecision {
        let ok: Bool
        let effect: EffectState
        let verification: ActionVerification
        let target: ActionTarget?

        static func submitted(snapshot: Snapshot?, target: ActionTarget?) -> VerificationDecision {
            VerificationDecision(
                ok: true,
                effect: .submitted,
                verification: ActionVerification(
                    status: .unverifiable,
                    strategy: "post_action_snapshot",
                    reason: snapshot == nil
                        ? "The event was submitted, but no post-action snapshot was available."
                        : "The event was submitted; a snapshot was captured but its semantic effect was not verified.",
                    snapshotID: snapshot?.id
                ),
                target: target
            )
        }
    }

    private func defaultRouteDiagnostics(executionPath: String, deliveryMode: DeliveryMode) -> ActionRouteDiagnostics {
        let route: ActionRoute
        if deliveryMode == .foreground {
            route = .foreground
        } else if executionPath == "cg_event" {
            route = .coordinate
        } else {
            route = .native
        }
        return ActionRouteDiagnostics(
            route: route,
            attempts: [ActionRouteAttempt(route: route, status: .selected, reason: "Selected by the bounded route policy.")]
        )
    }

    private func diagnosticTrace(
        traceID: String,
        action: String,
        startedAt: String,
        request: [String: String],
        target: ActionTarget?,
        before: Snapshot?,
        after: Snapshot?,
        route: ActionRouteDiagnostics?,
        policy: String,
        outcome: DiagnosticTraceOutcome
    ) -> DiagnosticTraceRecord {
        DiagnosticTraceRecord(
            traceID: traceID,
            action: action,
            startedAt: startedAt,
            finishedAt: DateFormats.iso8601String(from: Date()),
            request: request,
            target: target,
            before: before.map { DiagnosticTraceObservation(snapshotID: $0.id) },
            after: after.map { DiagnosticTraceObservation(snapshotID: $0.id) },
            route: route,
            policy: DiagnosticTracePolicy(decision: policy),
            outcome: outcome
        )
    }

    private func executeAction(
        name: String,
        targets: [String: String] = [:],
        executionPath: String = "unknown",
        target: ActionTarget? = nil,
        verification: (() -> VerificationDecision)? = nil,
        conditions: ActionConditions? = nil,
        snapshotID: String? = nil,
        windowID: Int? = nil,
        deliveryMode: DeliveryMode = .automatic,
        routeDiagnostics: ActionRouteDiagnostics? = nil,
        action: () throws -> String
    ) throws -> ActionResult {
        let traceID = UUID().uuidString
        let startedAt = DateFormats.iso8601String(from: Date())
        let beforeSnapshot = snapshotID.flatMap { accessibility.cachedSnapshot(for: $0) }
        do {
            var preconditionEvaluation: PredicateEvaluation?
        if let predicate = conditions?.precondition {
            let evaluation: PredicateEvaluation
            do {
                evaluation = try evaluate(
                    predicate: predicate,
                    snapshotID: snapshotID,
                    windowID: windowID
                )
            } catch {
                let reason = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
                evaluation = PredicateEvaluation(
                    status: .unverifiable,
                    predicate: predicate,
                    observation: nil,
                    reason: reason
                )
            }
            guard evaluation.status == .satisfied else {
                throw AutomationError.preconditionFailed(evaluation)
            }
            preconditionEvaluation = evaluation
        }

            let message = try action()
            let actionSnapshot = try? screen.captureMainDisplay()
            let postconditionEvaluation: PredicateEvaluation
            if let predicate = conditions?.postcondition {
                do {
                    postconditionEvaluation = try evaluate(
                        predicate: predicate,
                        snapshotID: nil,
                        windowID: windowID
                    )
                } catch {
                    let reason = (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
                    postconditionEvaluation = PredicateEvaluation(
                        status: .unverifiable,
                        predicate: predicate,
                        observation: nil,
                        reason: reason
                    )
                }
            } else {
                postconditionEvaluation = PredicateEvaluation(
                    status: .unverifiable,
                    predicate: nil,
                    observation: nil,
                    reason: "No postcondition was requested."
                )
            }
            let conditionResult = conditions.map { _ in
                ActionConditionsResult(
                    precondition: preconditionEvaluation,
                    postcondition: postconditionEvaluation
                )
            }
            let decision = verification?() ?? VerificationDecision(
                ok: true,
                effect: conditions?.postcondition != nil && postconditionEvaluation.status == .satisfied ? .confirmed : .submitted,
                verification: ActionVerification(
                    status: conditions?.postcondition != nil && postconditionEvaluation.status == .satisfied ? .confirmed : .unverifiable,
                    strategy: conditions?.postcondition != nil ? "postcondition" : "event_submission",
                    reason: conditions?.postcondition != nil ? postconditionEvaluation.reason : "The event was submitted; its semantic effect was not verified.",
                    snapshotID: actionSnapshot?.id
                ),
                target: target
            )
            let finalRouteDiagnostics = routeDiagnostics ?? defaultRouteDiagnostics(executionPath: executionPath, deliveryMode: deliveryMode)
            let result = ActionResult(
                ok: decision.ok,
                message: message,
                snapshot: actionSnapshot,
                contractVersion: EffectContract.currentVersion,
                effect: decision.effect,
                verification: decision.verification,
                executionPath: executionPath,
                deliveryMode: deliveryMode,
                target: decision.target ?? target,
                routeDiagnostics: finalRouteDiagnostics,
                conditions: conditionResult
            )
            let trace = diagnosticTrace(
                traceID: traceID,
                action: name,
                startedAt: startedAt,
                request: targets,
                target: decision.target ?? target,
                before: beforeSnapshot,
                after: actionSnapshot,
                route: finalRouteDiagnostics,
                policy: "allowed",
                outcome: DiagnosticTraceOutcome(
                    success: decision.ok,
                    effect: decision.effect,
                    verification: decision.verification,
                    message: message
                )
            )
            try? history.record(HistoryEvent(
                action: name,
                targets: targets,
                success: decision.ok,
                message: message,
                contractVersion: EffectContract.currentVersion,
                effect: decision.effect,
                verification: decision.verification,
                executionPath: executionPath,
                target: decision.target ?? target,
                diagnosticTrace: trace
            ))
            return result
        } catch {
            let message = error.localizedDescription
            let verification = ActionVerification(
                status: .notAttempted,
                strategy: "precondition",
                reason: message
            )
            let finalRouteDiagnostics = routeDiagnostics ?? defaultRouteDiagnostics(executionPath: executionPath, deliveryMode: deliveryMode)
            let trace = diagnosticTrace(
                traceID: traceID,
                action: name,
                startedAt: startedAt,
                request: targets,
                target: target,
                before: beforeSnapshot,
                after: nil,
                route: finalRouteDiagnostics,
                policy: "refused",
                outcome: DiagnosticTraceOutcome(
                    success: false,
                    effect: .refused,
                    verification: verification,
                    message: message,
                    errorCode: (error as? AutomationError)?.code ?? "operation_failed"
                )
            )
            try? history.record(HistoryEvent(
                action: name,
                targets: targets,
                success: false,
                message: message,
                contractVersion: EffectContract.currentVersion,
                effect: .refused,
                verification: verification,
                executionPath: executionPath,
                target: target,
                diagnosticTrace: trace
            ))
            throw error
        }
    }

    private func evaluate(
        predicate: UIElementPredicate,
        snapshotID: String?,
        windowID: Int?
    ) throws -> PredicateEvaluation {
        let observation = try findUI(
            predicate: predicate,
            snapshotID: snapshotID,
            maxDepth: 4,
            maxNodes: 200,
            windowID: windowID
        )
        let appMatches = predicate.app.map { pattern in
            guard let app = observation.app else { return false }
            return predicate.matchField(app.localizedName, pattern: pattern)
                || predicate.matchField(app.bundleIdentifier, pattern: pattern)
        } ?? true
        let windowMatches = predicate.window.map { pattern in
            let candidate = apps.listWindows().first {
                $0.windowID == observation.snapshot.windowID
            } ?? windowID.flatMap { id in apps.listWindows().first { $0.windowID == id } }
            guard let candidate else { return false }
            return predicate.matchField(candidate.ownerName, pattern: pattern)
                || predicate.matchField(candidate.title, pattern: pattern)
        } ?? true
        let nodeCriteria = predicate.hasNodeCriteria
        let nodeMatches = !nodeCriteria || !observation.nodes.isEmpty
        let status: PredicateEvaluationStatus = appMatches && windowMatches && nodeMatches ? .satisfied : .failed
        let reason: String?
        if status == .satisfied {
            reason = nil
        } else {
            reason = "The predicate did not match the current UI observation."
        }
        return PredicateEvaluation(status: status, predicate: predicate, observation: observation, reason: reason)
    }

    public func click(
        snapshotID: String? = nil,
        elementID: String? = nil,
        x: Double? = nil,
        y: Double? = nil,
        button: String = "left",
        doubleClick: Bool = false,
        conditions: ActionConditions? = nil,
        target: TargetIdentity? = nil,
        deliveryMode: DeliveryMode = .automatic
    ) throws -> ActionResult {
        try requirePermission(.input, for: "click")
        var targets: [String: String] = ["button": button, "doubleClick": String(doubleClick)]
        if let snapshotID { targets["snapshotID"] = snapshotID }
        if let elementID { targets["elementID"] = elementID }
        if let x { targets["x"] = String(x) }
        if let y { targets["y"] = String(y) }
        let resolvedTarget = try resolveActionTarget(target, snapshotID: snapshotID)

        let semanticElement: AccessibilityService.ResolvedElement? = if let snapshotID, let elementID {
            accessibility.resolveElement(snapshotID: snapshotID, elementID: elementID)
        } else {
            nil
        }
        let semanticAvailable = semanticElement?.enabled != false
            && semanticElement?.actions.contains("AXPress") == true
        let selectedRoute: ActionRoute
        if deliveryMode == .foreground {
            selectedRoute = .foreground
        } else if semanticAvailable {
            selectedRoute = .semantic
        } else {
            selectedRoute = .coordinate
        }
        var attempts: [ActionRouteAttempt] = []
        if selectedRoute == .semantic {
            attempts.append(ActionRouteAttempt(route: .semantic, status: .selected, reason: "The observed element exposes AXPress and is enabled."))
        } else if deliveryMode == .foreground {
            attempts.append(ActionRouteAttempt(route: .semantic, status: .skipped, reason: "Foreground delivery was explicitly requested."))
            attempts.append(ActionRouteAttempt(route: .coordinate, status: .skipped, reason: "Foreground delivery supersedes coordinate delivery."))
        } else if semanticElement?.enabled == false {
            attempts.append(ActionRouteAttempt(route: .semantic, status: .refused, reason: "The observed element is disabled."))
        } else {
            attempts.append(ActionRouteAttempt(route: .semantic, status: .skipped, reason: "The observed element does not expose AXPress; no semantic retry is made."))
        }
        if selectedRoute != .semantic {
            attempts.append(ActionRouteAttempt(route: selectedRoute, status: .selected, reason: "Selected by the bounded route policy."))
        }
        let diagnostics = ActionRouteDiagnostics(route: selectedRoute, attempts: attempts)

        return try executeAction(name: "click", targets: targets, executionPath: "cg_event", target: actionTarget(for: resolvedTarget), conditions: conditions, snapshotID: snapshotID, windowID: resolvedTarget?.window?.windowID, deliveryMode: deliveryMode, routeDiagnostics: diagnostics) {
            if selectedRoute == .semantic {
                guard let snapshotID, let elementID else {
                    throw AutomationError.staleReference("Semantic delivery requires the observed snapshot and element identity.")
                }
                try self.guardAgainstResolvedElement(semanticElement)
                try self.accessibility.performElementAction(snapshotID: snapshotID, elementID: elementID, action: "AXPress")
                return "Semantic AXPress action performed."
            }
            let target = try self.resolvePoint(snapshotID: snapshotID, elementID: elementID, x: x, y: y)
            try self.input.click(at: target, button: button, doubleClick: doubleClick, deliveryMode: deliveryMode, targetProcessID: resolvedTarget?.app.processIdentifier)
            return "Click event posted at (\(Int(target.x)), \(Int(target.y)))."
        }
    }

    public func typeText(_ text: String, conditions: ActionConditions? = nil, target: TargetIdentity? = nil, deliveryMode: DeliveryMode = .automatic) throws -> ActionResult {
        try requirePermission(.input, for: "type_text")
        let redacted = "<redacted: \(text.count) chars>"
        let resolvedTarget = try resolveActionTarget(target)
        return try executeAction(name: "type_text", targets: ["text": redacted], executionPath: "cg_event", target: actionTarget(for: resolvedTarget), conditions: conditions, windowID: resolvedTarget?.window?.windowID, deliveryMode: deliveryMode) {
            if let target = resolvedTarget?.identity { _ = try self.apps.focusTarget(target) }
            guard !text.isEmpty else {
                throw AutomationError.invalidArgument("type_text requires a non-empty text argument.")
            }
            try guardAgainstSecureField()
            try input.typeText(text, deliveryMode: deliveryMode, targetProcessID: resolvedTarget?.app.processIdentifier)
            return "\(text.count) keystroke events posted."
        }
    }

    public func pressKeys(_ keys: [String], conditions: ActionConditions? = nil, target: TargetIdentity? = nil, deliveryMode: DeliveryMode = .automatic) throws -> ActionResult {
        try requirePermission(.input, for: "press_keys")
        let resolvedTarget = try resolveActionTarget(target)
        return try executeAction(name: "press_keys", targets: ["keys": keys.joined(separator: "+")], executionPath: "cg_event", target: actionTarget(for: resolvedTarget), conditions: conditions, windowID: resolvedTarget?.window?.windowID, deliveryMode: deliveryMode) {
            if let target = resolvedTarget?.identity { _ = try self.apps.focusTarget(target) }
            try guardAgainstSecureField()
            try input.pressKeys(keys, deliveryMode: deliveryMode, targetProcessID: resolvedTarget?.app.processIdentifier)
            return "Key events posted: \(keys.joined(separator: "+"))."
        }
    }

    public func scroll(deltaX: Double = 0, deltaY: Double, conditions: ActionConditions? = nil, target: TargetIdentity? = nil, deliveryMode: DeliveryMode = .automatic) throws -> ActionResult {
        try requirePermission(.input, for: "scroll")
        let resolvedTarget = try resolveActionTarget(target)
        return try executeAction(name: "scroll", targets: ["deltaX": String(deltaX), "deltaY": String(deltaY)], executionPath: "cg_event", target: actionTarget(for: resolvedTarget), conditions: conditions, windowID: resolvedTarget?.window?.windowID, deliveryMode: deliveryMode) {
            if let target = resolvedTarget?.identity { _ = try self.apps.focusTarget(target) }
            try input.scroll(deltaX: deltaX, deltaY: deltaY, deliveryMode: deliveryMode, targetProcessID: resolvedTarget?.app.processIdentifier)
            return "Scroll event posted with delta (\(deltaX), \(deltaY))."
        }
    }

    public func drag(
        snapshotID: String? = nil,
        fromElementID: String? = nil,
        toElementID: String? = nil,
        fromX: Double? = nil,
        fromY: Double? = nil,
        toX: Double? = nil,
        toY: Double? = nil,
        conditions: ActionConditions? = nil,
        target: TargetIdentity? = nil,
        deliveryMode: DeliveryMode = .automatic
    ) throws -> ActionResult {
        try requirePermission(.input, for: "drag")
        var targets: [String: String] = [:]
        if let snapshotID { targets["snapshotID"] = snapshotID }
        if let fromElementID { targets["fromElementID"] = fromElementID }
        if let toElementID { targets["toElementID"] = toElementID }
        if let fromX { targets["fromX"] = String(fromX) }
        if let fromY { targets["fromY"] = String(fromY) }
        if let toX { targets["toX"] = String(toX) }
        if let toY { targets["toY"] = String(toY) }
        let resolvedTarget = try resolveActionTarget(target, snapshotID: snapshotID)

        return try executeAction(name: "drag", targets: targets, target: actionTarget(for: resolvedTarget), conditions: conditions, snapshotID: snapshotID, windowID: resolvedTarget?.window?.windowID, deliveryMode: deliveryMode) {
            let start = try resolvePoint(snapshotID: snapshotID, elementID: fromElementID, x: fromX, y: fromY)
            let end = try resolvePoint(snapshotID: snapshotID, elementID: toElementID, x: toX, y: toY)
            try input.drag(from: start, to: end, steps: 24, deliveryMode: deliveryMode, targetProcessID: resolvedTarget?.app.processIdentifier)
            return "Drag event posted from (\(Int(start.x)), \(Int(start.y))) to (\(Int(end.x)), \(Int(end.y)))."
        }
    }

    public func launchApp(bundleID: String? = nil, appName: String? = nil, conditions: ActionConditions? = nil) throws -> ActionResult {
        try requirePermission(.appControl, for: "launch_app")
        var targets: [String: String] = [:]
        if let bundleID { targets["bundleID"] = bundleID }
        if let appName { targets["appName"] = appName }

        return try executeAction(name: "launch_app", targets: targets, executionPath: "cg_event", conditions: conditions) {
            try apps.launchApp(bundleID: bundleID, appName: appName)
            return "Application launch requested."
        }
    }

    public func focusWindow(
        bundleID: String? = nil,
        appName: String? = nil,
        title: String? = nil,
        conditions: ActionConditions? = nil,
        target: TargetIdentity? = nil,
        deliveryMode: DeliveryMode = .automatic
    ) throws -> ActionResult {
        try requirePermission(.appControl, for: "focus_window")
        let requested = target ?? TargetIdentity(bundleID: bundleID, appName: appName, windowTitle: title)
        let resolved = try requested.isExplicit ? apps.resolveTarget(requested) : nil
        let requestedTarget = actionTarget(for: resolved) ?? ActionTarget(
            requestedBundleID: bundleID,
            requestedAppName: appName,
            requestedWindowTitle: title
        )
        let requestedBundleID = resolved?.app.bundleIdentifier ?? bundleID
        let requestedAppName = resolved?.app.localizedName ?? appName
        let requestedTitle = resolved?.window?.title ?? title
        return try executeAction(
            name: "focus_window",
            targets: [
                "bundleID": requestedBundleID ?? "",
                "appName": requestedAppName ?? "",
                "title": requestedTitle ?? ""
            ],
            executionPath: "nsworkspace_ax",
            target: requestedTarget,
            verification: {
                guard let frontmost = self.apps.frontmostApp() else {
                    return VerificationDecision(
                        ok: true,
                        effect: .submitted,
                        verification: ActionVerification(status: .unverifiable, strategy: "focus_readback", reason: "The focus request was submitted, but macOS exposed no frontmost application."),
                        target: requestedTarget
                    )
                }
                let appMatches = (requestedBundleID == nil || frontmost.bundleIdentifier == requestedBundleID)
                    && (requestedAppName == nil || frontmost.localizedName.localizedCaseInsensitiveCompare(requestedAppName!) == .orderedSame)
                let observedWindow = self.apps.frontmostWindow(ownerPID: frontmost.processIdentifier, title: requestedTitle)
                let windowMatches = requested.windowID == nil
                    ? (requestedTitle?.isEmpty != false || observedWindow != nil)
                    : observedWindow?.windowID == requested.windowID
                let observedTarget = ActionTarget(
                    requestedPID: requested.processID ?? resolved?.app.processIdentifier,
                    requestedBundleID: requested.bundleID ?? requestedBundleID,
                    requestedAppName: requested.appName ?? requestedAppName,
                    requestedWindowID: requested.windowID,
                    requestedWindowTitle: requested.windowTitle ?? requestedTitle,
                    frontmostPID: frontmost.processIdentifier,
                    frontmostWindowID: observedWindow?.windowID
                )
                if appMatches && windowMatches {
                    return VerificationDecision(
                        ok: true,
                        effect: .confirmed,
                        verification: ActionVerification(status: .confirmed, strategy: "focus_readback", reason: "The requested application and window matched the frontmost read-back."),
                        target: observedTarget
                    )
                }
                return VerificationDecision(
                    ok: false,
                    effect: .suspectedNoop,
                    verification: ActionVerification(status: .suspectedNoop, strategy: "focus_readback", reason: "The frontmost application or requested window did not match after focus was submitted."),
                    target: observedTarget
                )
            },
            conditions: conditions,
            windowID: resolved?.window?.windowID
        ) {
            if requested.isExplicit {
                _ = try self.apps.focusTarget(requested)
            } else {
                try self.apps.focusWindow(bundleID: bundleID, appName: appName, title: title)
            }
            return "Window-focus request submitted."
        }
    }

    public func menuAction(path: [String], conditions: ActionConditions? = nil, target: TargetIdentity? = nil, deliveryMode: DeliveryMode = .automatic) throws -> ActionResult {
        try requirePermission(.menuAction, for: "menu_action")
        guard deliveryMode != .background else {
            throw AutomationError.unsupported("Background delivery for menu_action is unsupported because menu actions are scoped to the frontmost application.")
        }
        let resolvedTarget = try resolveActionTarget(target)
        return try executeAction(name: "menu_action", targets: ["path": path.joined(separator: " > ")], executionPath: "cg_event", target: actionTarget(for: resolvedTarget), conditions: conditions, windowID: resolvedTarget?.window?.windowID, deliveryMode: deliveryMode) {
            if let processID = resolvedTarget?.app.processIdentifier {
                try self.accessibility.performMenuAction(path: path, processID: processID)
            } else {
                try self.accessibility.performMenuAction(path: path)
            }
            return "Menu action posted: \(path.joined(separator: " > "))."
        }
    }

    public func waitFor(text: String?, app: String?, timeoutSeconds: Double = 10, target: TargetIdentity? = nil) async throws -> ActionResult {
        let resolvedTarget = try target.flatMap { $0.isExplicit ? try apps.resolveTarget($0) : nil }
        accessibility.invalidatePollingCache()

        let observer = NSWorkspace.shared.notificationCenter.addObserver(
            forName: NSWorkspace.didActivateApplicationNotification,
            object: nil,
            queue: .main
        ) { [accessibility] _ in
            accessibility.invalidatePollingCache()
        }
        defer { NSWorkspace.shared.notificationCenter.removeObserver(observer) }

        let deadline = Date().addingTimeInterval(timeoutSeconds)
        while Date() < deadline {
            if let app, !app.isEmpty {
                let exists: Bool
                if let resolvedTarget {
                    exists = resolvedTarget.app.localizedName.localizedCaseInsensitiveContains(app)
                        || (resolvedTarget.app.bundleIdentifier?.localizedCaseInsensitiveContains(app) ?? false)
                } else {
                    exists = apps.listApps().contains {
                        $0.localizedName.localizedCaseInsensitiveContains(app) || ($0.bundleIdentifier?.localizedCaseInsensitiveContains(app) ?? false)
                    }
                }
                if exists {
                    return ActionResult(ok: true, message: "Observed app '\(app)'.", snapshot: try? screen.captureMainDisplay())
                }
            }

            if let text, !text.isEmpty {
                let found = resolvedTarget.map { accessibility.containsText(text, processID: $0.app.processIdentifier) }
                    ?? accessibility.frontmostContainsTextPolling(text)
                if found {
                    return ActionResult(ok: true, message: "Observed text '\(text)'.", snapshot: try? screen.captureMainDisplay())
                }
            }

            try await Task.sleep(nanoseconds: 400_000_000) // 0.4 seconds
        }

        throw AutomationError.operationFailed("Condition was not met within \(timeoutSeconds) seconds.")
    }

    private func guardAgainstResolvedElement(_ resolved: AccessibilityService.ResolvedElement?) throws {
        guard let resolved else {
            throw AutomationError.staleReference("The referenced snapshot has expired or the element no longer exists.")
        }
        if resolved.role == "AXSecureTextField" {
            throw AutomationError.permissionDenied("Refusing to target a secure text field.")
        }
        if actionPolicy.isDestructive(role: resolved.role, title: resolved.title, label: resolved.label, value: resolved.value) {
            throw AutomationError.permissionDenied("Refusing to target a potentially destructive UI element.")
        }
        if resolved.enabled == false {
            throw AutomationError.unsupported("The target UI element is disabled.")
        }
    }

    private func guardAgainstSecureField() throws {
        if let role = accessibility.frontmostFocusedElementRole(), role == "AXSecureTextField" {
            throw AutomationError.permissionDenied("Refusing to type into a secure text field.")
        }
    }

    /// Throws a classified `.permissionDenied` refusal when `required` is not
    /// present in `actionPolicy.grantedPermissions`. The destructive-keyword
    /// guard inside `firstViolation` always takes precedence and is unchanged;
    /// this gate is ADDITIONAL to it.
    private func requirePermission(_ required: PermissionFlags, for action: String) throws {
        if let violation = actionPolicy.firstViolation(requiredPermission: required) {
            let names = violation.flagNames.joined(separator: ", ")
            throw AutomationError.permissionDenied(
                "Permission denied: the '\(names)' permission is required for \(action)."
            )
        }
    }

    private func resolvePoint(snapshotID: String?, elementID: String?, x: Double?, y: Double?) throws -> PointValue {
        if let snapshotID, let elementID {
            if let resolved = accessibility.resolveElement(snapshotID: snapshotID, elementID: elementID) {
                try guardAgainstResolvedElement(resolved)

                if let frame = resolved.frame {
                    return frame.center
                }
                throw AutomationError.unavailable("The target element does not expose a clickable frame.")
            }
            throw AutomationError.staleReference("The referenced snapshot has expired or the element no longer exists.")
        }

        if let x, let y {
            if let resolved = accessibility.resolveElementAtPoint(x: x, y: y) {
                try guardAgainstResolvedElement(resolved)
                return PointValue(x: x, y: y)
            }
            throw AutomationError.permissionDenied("Cannot identify the element at the given coordinates. Use snapshot_id + element_id instead, or take a snapshot first to enable coordinate-based targeting.")
        }

        throw AutomationError.invalidArgument("Provide either x/y coordinates or snapshot_id + element_id.")
    }
}

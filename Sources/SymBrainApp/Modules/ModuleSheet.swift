import AppKit
import SwiftUI
import SymairaTheme
import SymBrainCore

/// Detail sheet for one optional module: config switch, managed binary,
/// profile exposure, health probe and the build-from-source setup.
///
/// The health probes (Operate doctor, Scope scan, Operate version check)
/// have side effects, so they run only from their buttons, never on open.
struct ModuleSheet: View {
    let module: ManagedModule
    let client: SymBrainClient
    let close: () -> Void

    var body: some View {
        switch module {
        case .operate: OperateModuleSheet(client: client, close: close)
        case .scope: ScopeModuleSheet(client: client, close: close)
        }
    }
}

private struct OperateModuleSheet: View {
    let close: () -> Void
    @StateObject private var vm: OperateViewModel

    init(client: SymBrainClient, close: @escaping () -> Void) {
        self.close = close
        _vm = StateObject(wrappedValue: OperateViewModel(symbrainClient: client))
    }

    var body: some View {
        ModuleSheetBody(module: .operate, vm: vm, close: close) {
            CardSection("Health") {
                Button("Run Doctor") { Task { await vm.runDoctor() } }
                    .symairaButtonStyle(.secondary)
                    .disabled(vm.isRunningDoctor)
            } content: {
                Text("Takes a real screenshot and reads the frontmost app's interface to prove both permissions work.")
                    .symairaText(.caption)
                if vm.isRunningDoctor {
                    SymairaLoadingState("Probing screen capture and accessibility…")
                } else if let doctor = vm.doctorReport {
                    SymairaStatusLabel(doctor.ok ? "Ready" : "Needs Attention", tone: doctor.ok ? .positive : .warning)
                    DetailRow(label: "Accessibility", value: doctor.permissions.accessibilityGranted ? "Granted" : "Missing")
                    DetailRow(label: "Screen Recording", value: doctor.permissions.screenRecordingGranted ? "Granted" : "Missing")
                    DetailRow(
                        label: "Environment",
                        value: "macOS \(doctor.environment.macOSVersion), \(doctor.environment.displaysCount) displays, \(doctor.environment.appsCount) apps"
                    )
                    ForEach(doctor.recommendations, id: \.self) { note in
                        Text(note).symairaText(.secondary)
                    }
                }
            }
            CardSection("Version") {
                Button("Check for Updates") { Task { await vm.checkVersion() } }
                    .symairaButtonStyle(.secondary)
                    .disabled(vm.isCheckingVersion)
            } content: {
                Text("Contacts GitHub to look for a newer symoperate release.")
                    .symairaText(.caption)
                if vm.isCheckingVersion {
                    SymairaLoadingState("Checking for updates…")
                } else if let version = vm.versionReport {
                    DetailRow(label: "Installed", value: version.version)
                    if version.updateAvailable, let latest = version.latestVersion {
                        DetailRow(label: "Available", value: latest)
                    }
                }
            }
        }
    }
}

private struct ScopeModuleSheet: View {
    let close: () -> Void
    @StateObject private var vm: ScopeViewModel

    init(client: SymBrainClient, close: @escaping () -> Void) {
        self.close = close
        _vm = StateObject(wrappedValue: ScopeViewModel(symbrainClient: client))
    }

    var body: some View {
        ModuleSheetBody(module: .scope, vm: vm, close: close) {
            CardSection("Inventory") {
                Button("Scan Now") { Task { await vm.runScan() } }
                    .symairaButtonStyle(.secondary)
                    .disabled(vm.isScanning)
            } content: {
                Text("Walks listening ports, launchd and Homebrew daemons and Docker containers on this Mac.")
                    .symairaText(.caption)
                if vm.isScanning {
                    SymairaLoadingState("Scanning this Mac…")
                } else if let snapshot = vm.snapshot {
                    HStack(spacing: SymairaSpacing.xLarge) {
                        count("Ports", snapshot.portCount)
                        count("MCP Servers", snapshot.mcpServerCount)
                        count("Containers", snapshot.containerCount)
                        count("Daemons", snapshot.daemonCount)
                    }
                    ForEach(snapshot.notes, id: \.self) { note in
                        Text(note).symairaText(.secondary)
                    }
                }
            }
        }
    }

    private func count(_ label: String, _ value: Int) -> some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.xSmall) {
            Text("\(value)").font(SymairaTypography.readout).foregroundStyle(SymairaTokens.ink)
            Text(label).symairaText(.caption)
        }
        .accessibilityElement(children: .combine)
    }
}

/// Shared structure of both module sheets.
private struct ModuleSheetBody<Model: ManagedModuleModel, Probes: View>: View {
    let module: ManagedModule
    @ObservedObject var vm: Model
    let close: () -> Void
    @ViewBuilder let probes: Probes

    var body: some View {
        SymairaSheetScaffold(title: "\(module.title) Module", dismissTitle: "Close", dismiss: close) {
            if let error = vm.errorMessage {
                SymairaNotice(title: error, message: vm.errorDetail ?? "Check the module setup and try again.", tone: .critical)
            }
            if vm.isLoading && vm.binaryPath == nil {
                SymairaLoadingState("Checking the \(module.title) module…")
            } else if !vm.moduleEnabled {
                SymairaNotice(
                    title: "\(module.title) Is Turned Off",
                    message: "Brain manages no \(module.binary) binary while the module is off. Turning it on exposes nothing until a profile names it.",
                    tone: .informative
                )
                CommandLineView(vm.enableCommand)
            } else if vm.availability == .missing {
                setup
            } else {
                status
                probes
            }
        }
        .task { await vm.refresh() }
    }

    private var status: some View {
        SymairaFormSection("Managed Binary") {
            DetailRow(label: "Path", value: vm.binaryPath ?? "—", monospaced: true)
            if let provenance = vm.provenance {
                DetailRow(label: "Version", value: provenance.version)
                DetailRow(label: "Source", value: provenance.source)
                if let commit = provenance.receiverCommit {
                    DetailRow(label: "Commit", value: String(commit.prefix(12)), monospaced: true)
                }
                if let builtAt = provenance.formattedBuiltAt {
                    DetailRow(label: "Built", value: builtAt)
                }
                DetailRow(label: "SHA-256", value: String(provenance.binarySHA256.prefix(16)) + "…", monospaced: true)
            } else {
                DetailRow(label: "Provenance", value: "Unknown, no provenance file next to this binary")
            }
            SymairaFormDivider()
            Text("PROFILE EXPOSURE").symairaText(.sectionLabel).padding(.top, SymairaSpacing.small)
            if vm.profileExposure.isEmpty {
                Text("No profiles found.").symairaText(.caption)
            }
            ForEach(vm.profileExposure) { exposure in
                SymairaFormRow(exposure.profile, description: exposure.mode) {
                    SymairaStatusLabel(exposure.enabled ? "Exposed" : "Hidden", tone: exposure.enabled ? .positive : .neutral)
                }
            }
        }
    }

    private var setup: some View {
        VStack(alignment: .leading, spacing: SymairaSpacing.medium) {
            SymairaNotice(
                title: "\(module.binary) Is Not Installed",
                message: "Build it from a local Symaira Brain checkout, or run the command below in Terminal.",
                tone: .warning
            )
            CommandLineView(vm.setupCommand)
            if vm.isBuilding {
                SymairaLoadingState("Building \(module.binary) from source… this can take a few minutes.")
            } else {
                SymairaFormSection("Build From Source", footer: "The checkout is used for this build only and is not saved.") {
                    SymairaFormRow("Brain Checkout", description: vm.sourceRoot?.path ?? "No checkout chosen") {
                        Button(vm.sourceRoot == nil ? "Choose Checkout…" : "Change Checkout…", action: chooseCheckout)
                            .symairaButtonStyle(.secondary)
                    }
                }
                if let sourceRoot = vm.sourceRoot {
                    Button("Build and Install \(module.binary)") {
                        Task { await vm.buildAndInstall(sourceRoot: sourceRoot) }
                    }
                    .symairaButtonStyle(.primary)
                }
            }
        }
    }

    private func chooseCheckout() {
        let panel = NSOpenPanel()
        panel.title = "Choose Symaira Brain Checkout"
        panel.message = "Select the checkout root that contains the module source packages."
        panel.prompt = "Use Checkout"
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = false
        panel.allowsMultipleSelection = false
        guard panel.runModal() == .OK, let url = panel.url else { return }
        vm.selectSourceRoot(url)
    }
}

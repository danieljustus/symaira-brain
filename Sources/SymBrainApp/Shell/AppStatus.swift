import SwiftUI
import SymairaCLIRunner
import SymairaTheme
import SymBrainCore

/// Reachability of the `symbrain` CLI, shown persistently in the sidebar
/// (STATE-4) so an unreachable backend is never only a silent empty page.
@MainActor
final class AppStatus: ObservableObject {
    enum State: Equatable {
        case checking
        case ready(version: String)
        case missing
        case failed(String)
    }

    @Published private(set) var state: State = .checking
    private var probe: Task<VersionInfo, Error>?

    /// The probe runs in its own task: SwiftUI cancels a view's `.task` when
    /// the window rebuilds during launch, and a cancelled probe must not read
    /// as "CLI not responding". A newer refresh supersedes an older one.
    func refresh(client: SymBrainClient) async {
        probe?.cancel()
        let task = Task { try await client.version() }
        probe = task
        state = .checking
        do {
            let info = try await task.value
            guard probe == task else { return }
            state = .ready(version: info.version)
        } catch is CancellationError {
            return
        } catch {
            guard probe == task else { return }
            if let cliError = error as? CLIRunnerError, case .binaryNotFound = cliError {
                state = .missing
            } else {
                state = .failed(formatError(error).message)
            }
        }
    }

    var label: String {
        switch state {
        case .checking: "Checking CLI…"
        case .ready(let version): "CLI \(version) connected"
        case .missing: "CLI not found"
        case .failed: "CLI not responding"
        }
    }

    var tone: SymairaTone {
        switch state {
        case .checking: .neutral
        case .ready: .positive
        case .missing, .failed: .critical
        }
    }
}

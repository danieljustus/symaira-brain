import Foundation

/// A read-only snapshot of one launchd agent/daemon or Homebrew service.
public struct Daemon: Codable, Equatable, Sendable {
    public var label: String
    public var state: String // running | loading | not-loaded
    public var pid: Int?
    public var lastExitStatus: Int?
    public var domain: String // user | system
    public var origin: String // brew | manual | unknown
    public var keepAlive: Bool?
    public var runAtLoad: Bool?
    public var ports: [Int]
    public var notes: [String]

    public init(
        label: String,
        state: String,
        pid: Int? = nil,
        lastExitStatus: Int? = nil,
        domain: String,
        origin: String = "unknown",
        keepAlive: Bool? = nil,
        runAtLoad: Bool? = nil,
        ports: [Int] = [],
        notes: [String] = []
    ) {
        self.label = label
        self.state = state
        self.pid = pid
        self.lastExitStatus = lastExitStatus
        self.domain = domain
        self.origin = origin
        self.keepAlive = keepAlive
        self.runAtLoad = runAtLoad
        self.ports = ports.sorted()
        self.notes = notes
    }

    public enum CodingKeys: String, CodingKey {
        case label
        case state
        case pid
        case lastExitStatus = "last_exit_status"
        case domain
        case origin
        case keepAlive = "keep_alive"
        case runAtLoad = "run_at_load"
        case ports
        case notes
    }
}

/// The compact result returned by `scope daemons health`.
public struct DaemonHealthResult: Codable, Equatable, Sendable {
    public var label: String
    public var state: String
    public var healthy: Bool
    public var lastExitStatus: Int?
    public var notes: [String]

    public init(label: String, state: String, healthy: Bool, lastExitStatus: Int?, notes: [String] = []) {
        self.label = label
        self.state = state
        self.healthy = healthy
        self.lastExitStatus = lastExitStatus
        self.notes = notes
    }

    public enum CodingKeys: String, CodingKey {
        case label
        case state
        case healthy
        case lastExitStatus = "last_exit_status"
        case notes
    }
}

public struct LaunchctlRecord: Equatable, Sendable {
    public var label: String
    public var pid: Int?
    public var lastExitStatus: Int?
    public var domain: String

    public init(label: String, pid: Int?, lastExitStatus: Int?, domain: String) {
        self.label = label
        self.pid = pid
        self.lastExitStatus = lastExitStatus
        self.domain = domain
    }
}

public struct BrewServiceRecord: Equatable, Sendable {
    public var name: String
    public var status: String
    public var user: String
    public var file: String?
    public var label: String

    public init(name: String, status: String, user: String, file: String?, label: String) {
        self.name = name
        self.status = status
        self.user = user
        self.file = file
        self.label = label
    }
}

public struct PlistRecord: Equatable, Sendable {
    public var label: String
    public var domain: String
    public var origin: String
    public var keepAlive: Bool?
    public var runAtLoad: Bool?
    public var notes: [String]

    public init(
        label: String,
        domain: String,
        origin: String,
        keepAlive: Bool?,
        runAtLoad: Bool?,
        notes: [String] = []
    ) {
        self.label = label
        self.domain = domain
        self.origin = origin
        self.keepAlive = keepAlive
        self.runAtLoad = runAtLoad
        self.notes = notes
    }
}

public protocol DaemonCommandRunning: Sendable {
    func run(executable: String, arguments: [String]) throws -> String
}

public struct LocalDaemonCommandRunner: DaemonCommandRunning, Sendable {
    public init() {}

    public func run(executable: String, arguments: [String]) throws -> String {
        let result = try BoundedProcessRunner.run(
            executable: executable,
            arguments: arguments,
            timeoutSeconds: 10
        )
        if result.timedOut {
            throw DaemonCommandError.timedOut
        }
        guard result.terminationStatus == 0 else {
            throw DaemonCommandError.nonZero(result.terminationStatus)
        }
        return result.output
    }
}

public protocol DaemonFileSystemReading: Sendable {
    func directoryContents(atPath path: String) -> [String]
    func data(atPath path: String) -> Data?
    func isExecutableFile(atPath path: String) -> Bool
}

public struct LocalDaemonFileSystem: DaemonFileSystemReading, Sendable {
    public init() {}

    public func directoryContents(atPath path: String) -> [String] {
        (try? FileManager.default.contentsOfDirectory(atPath: path)) ?? []
    }

    public func data(atPath path: String) -> Data? {
        FileManager.default.contents(atPath: path)
    }

    public func isExecutableFile(atPath path: String) -> Bool {
        FileManager.default.isExecutableFile(atPath: path)
    }
}

public enum DaemonCommandError: Error, LocalizedError, Sendable {
    case nonZero(Int32)
    case timedOut

    public var errorDescription: String? {
        switch self {
        case .nonZero(let status): return "command exited with status \(status)"
        case .timedOut: return "command timed out"
        }
    }
}

/// Cached daemon inventory used by clients that poll more frequently than the
/// launchd/Homebrew collectors need to run.
///
/// The cache deliberately owns only scheduling and the last collected rows. A
/// caller can re-annotate those rows with a fresh port inventory on every poll
/// without invoking any daemon collector.
public struct DaemonRefreshCache: Sendable {
    public static let defaultRefreshInterval: TimeInterval = 60

    public let refreshInterval: TimeInterval
    public private(set) var daemons: [Daemon] = []
    public private(set) var notes: [String] = []
    private var lastRefreshAt: Date?

    public init(refreshInterval: TimeInterval = Self.defaultRefreshInterval) {
        self.refreshInterval = refreshInterval
    }

    /// Marks a collection as started when the cache is stale or a caller forces
    /// one. Recording the start time prevents a slow collector from being
    /// started again by the next 15-second poll.
    public mutating func beginRefresh(at now: Date, force: Bool = false) -> Bool {
        if !force,
           let lastRefreshAt,
           now.timeIntervalSince(lastRefreshAt) < refreshInterval {
            return false
        }
        lastRefreshAt = now
        return true
    }

    public mutating func update(daemons: [Daemon], notes: [String]) {
        self.daemons = daemons
        self.notes = notes
    }

    /// Port ownership is intentionally refreshed independently of daemon
    /// collection so conflict labels stay current on every 15-second tick.
    public mutating func annotatePorts(_ ports: [Port]) {
        daemons = DaemonService.annotatePorts(daemons, ports: ports)
    }
}

/// Read-only launchd/Homebrew inventory. All process and filesystem access is
/// injected, making parser and merge tests deterministic and safe in CI.
public struct DaemonService: Sendable {
    public let commandRunner: any DaemonCommandRunning
    public let fileSystem: any DaemonFileSystemReading
    public let portProvider: @Sendable () async throws -> [Port]
    public let homeDirectory: String

    public init(
        commandRunner: any DaemonCommandRunning = LocalDaemonCommandRunner(),
        fileSystem: any DaemonFileSystemReading = LocalDaemonFileSystem(),
        portProvider: @escaping @Sendable () async throws -> [Port] = { try await PortService.listListening() },
        homeDirectory: String = FileManager.default.homeDirectoryForCurrentUser.path
    ) {
        self.commandRunner = commandRunner
        self.fileSystem = fileSystem
        self.portProvider = portProvider
        self.homeDirectory = homeDirectory
    }

    public static func list(all: Bool = false) async -> ([Daemon], [String]) {
        await DaemonService().list(all: all)
    }

    /// Four collectors, none of which depends on another: launchd's own
    /// inventory, the plist directories, Homebrew's service list and the port
    /// inventory. They used to run one after another, which meant the whole
    /// call waited out `brew services list` — a Ruby process that is roughly
    /// 95% of the wall time here — and only then started on the rest. Running
    /// them together makes everything else effectively free.
    ///
    /// The merge order afterwards is unchanged and still sequential, because it
    /// is not independent: plist data merges into the launchd rows, and the
    /// Homebrew pass merges into the result of both.
    public func list(all: Bool = false) async -> ([Daemon], [String]) {
        async let launchctlCollector = collectLaunchctlRecords()
        async let plistCollector = collectPlistRecords()
        async let brewCollector = collectBrewServices()
        async let portCollector = collectPorts()

        var notes: [String] = []

        let (launchctlRecords, launchctlNotes) = await launchctlCollector
        notes += launchctlNotes

        var rows = launchctlRecords.map { record in
            Daemon(
                label: record.label,
                state: record.pid == nil ? "loading" : "running",
                pid: record.pid,
                lastExitStatus: record.lastExitStatus,
                domain: record.domain
            )
        }

        let (plistRecords, plistNotes) = await plistCollector
        notes += plistNotes

        for plist in plistRecords {
            if let index = rows.firstIndex(where: { $0.label == plist.label && $0.domain == plist.domain }) {
                rows[index].keepAlive = plist.keepAlive
                rows[index].runAtLoad = plist.runAtLoad
                rows[index].notes = Self.mergeNotes(rows[index].notes, plist.notes)
                rows[index].origin = Self.preferredOrigin(rows[index].origin, plist.origin)
            } else {
                rows.append(
                    Daemon(
                        label: plist.label,
                        state: "not-loaded",
                        domain: plist.domain,
                        origin: plist.origin,
                        keepAlive: plist.keepAlive,
                        runAtLoad: plist.runAtLoad,
                        notes: plist.notes
                    )
                )
            }
        }

        let (brewRecords, brewNotes) = await brewCollector
        notes += brewNotes

        for service in brewRecords {
            let candidates = [service.label, service.name, "homebrew.mxcl.\(service.name)"]
            if let index = rows.firstIndex(where: { candidates.contains($0.label) }) {
                rows[index].origin = "brew"
                rows[index].notes = Self.mergeNotes(rows[index].notes, service.status == "started" ? [] : ["homebrew status: \(service.status)"])
                if rows[index].state == "not-loaded", service.status == "started" {
                    rows[index].state = "loading"
                }
            } else {
                let state = service.status == "started" ? "loading" : "not-loaded"
                rows.append(
                    Daemon(
                        label: service.label,
                        state: state,
                        domain: "user",
                        origin: "brew",
                        notes: service.status == "started" ? [] : ["homebrew status: \(service.status)"]
                    )
                )
            }
        }

        let (ports, portNotes) = await portCollector
        notes += portNotes

        return Self.finalize(rows, all: all, ports: ports, notes: notes)
    }

    // MARK: - Collectors

    /// Runs blocking work off the Swift concurrency pool.
    ///
    /// `commandRunner` and `fileSystem` are synchronous by design — the shared
    /// `BoundedProcessRunner` waits for the child to exit — so running several
    /// of them at once must not occupy the cooperative pool's fixed number of
    /// threads. This is the same hop `BoundedProcessRunner.runAsync` makes for
    /// a single command; the collectors need it because they now overlap.
    private static func offCooperativePool<T: Sendable>(
        _ work: @escaping @Sendable () -> T
    ) async -> T {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .utility).async {
                continuation.resume(returning: work())
            }
        }
    }

    private func collectLaunchctlRecords() async -> ([LaunchctlRecord], [String]) {
        let commandRunner = self.commandRunner
        return await Self.offCooperativePool {
            var records: [LaunchctlRecord] = []
            var notes: [String] = []
            // Both domains go through one collector: they are two ~4 ms calls,
            // and keeping them in order keeps their notes in order too. They do
            // not share an invocation, though — `launchctl list` takes an
            // optional *label*, so `launchctl list system` looks up a job called
            // "system", fails with exit 113, and leaves every
            // /Library/LaunchDaemons entry to fall through to "not-loaded".
            // `launchctl print system` reads the domain unprivileged and carries
            // the same PID/status/label triples.
            do {
                let output = try commandRunner.run(executable: "/bin/launchctl", arguments: ["list"])
                records += Self.parseLaunchctlList(output, domain: "user")
            } catch {
                notes.append("launchctl user unavailable: \(error.localizedDescription)")
            }

            do {
                let output = try commandRunner.run(executable: "/bin/launchctl", arguments: ["print", "system"])
                records += Self.parseLaunchctlPrintServices(output, domain: "system")
            } catch {
                notes.append("launchctl system unavailable: \(error.localizedDescription)")
            }

            return (records, notes)
        }
    }

    private func collectPlistRecords() async -> ([PlistRecord], [String]) {
        let fileSystem = self.fileSystem
        let homeDirectory = self.homeDirectory
        return await Self.offCooperativePool {
            var records: [PlistRecord] = []
            var notes: [String] = []
            for directory in Self.plistDirectories(home: homeDirectory) {
                let domain = directory.hasSuffix("LaunchDaemons") ? "system" : "user"
                for filename in fileSystem.directoryContents(atPath: directory) where filename.hasSuffix(".plist") {
                    let path = (directory as NSString).appendingPathComponent(filename)
                    guard let data = fileSystem.data(atPath: path) else {
                        notes.append("plist unreadable: \(path)")
                        continue
                    }
                    guard let plist = Self.parsePlist(data, path: path, domain: domain) else {
                        notes.append("plist malformed: \(path)")
                        continue
                    }
                    records.append(plist)
                }
            }
            return (records, notes)
        }
    }

    private func collectBrewServices() async -> ([BrewServiceRecord], [String]) {
        let commandRunner = self.commandRunner
        let fileSystem = self.fileSystem
        return await Self.offCooperativePool {
            let brewPaths = ["/opt/homebrew/bin/brew", "/usr/local/bin/brew"]
            guard let brew = brewPaths.first(where: { fileSystem.isExecutableFile(atPath: $0) }) else {
                return ([], ["homebrew: brew CLI not found; service inventory unavailable"])
            }
            do {
                let output = try commandRunner.run(executable: brew, arguments: ["services", "list"])
                return (Self.parseBrewServices(output), [])
            } catch {
                return ([], ["homebrew: brew services unavailable: \(error.localizedDescription)"])
            }
        }
    }

    private func collectPorts() async -> ([Port], [String]) {
        do {
            return (try await portProvider(), [])
        } catch {
            return ([], ["ports: \(error.localizedDescription)"])
        }
    }

    public static func health(_ daemons: [Daemon]) -> [DaemonHealthResult] {
        daemons.map { daemon in
            // A live PID takes precedence over a stale prior exit status: a running
            // job can legitimately retain the previous invocation's signal/exit code.
            let healthy = daemon.state == "running"
                || (daemon.state != "not-loaded" && (daemon.lastExitStatus ?? 0) == 0)
            var notes = daemon.notes
            if daemon.state == "not-loaded" {
                notes = Self.mergeNotes(notes, ["not loaded (inactive by design, not a failure)"])
            }
            return DaemonHealthResult(
                label: daemon.label,
                state: daemon.state,
                healthy: healthy,
                lastExitStatus: daemon.lastExitStatus,
                notes: notes
            )
        }
    }

    /// Parses the tabular output from `launchctl list`.
    public static func parseLaunchctlList(_ output: String, domain: String) -> [LaunchctlRecord] {
        output.split(whereSeparator: \.isNewline).compactMap { rawLine in
            let fields = rawLine.split(whereSeparator: { $0 == " " || $0 == "\t" }).map(String.init)
            guard fields.count >= 3, fields[0].lowercased() != "pid" else { return nil }
            guard !fields[2].isEmpty else { return nil }
            let pid = fields[0] == "-" ? nil : Int(fields[0])
            let status = fields[1] == "-" ? nil : Int(fields[1])
            return LaunchctlRecord(label: fields[2], pid: pid, lastExitStatus: status, domain: domain)
        }
    }

    /// Parses the `services = { … }` block of `launchctl print <domain>`.
    ///
    /// The columns match `launchctl list` — PID, last exit status, label — with
    /// two differences: an unloaded job carries PID `0` rather than `-`, and the
    /// status column may hold a non-numeric annotation such as `(pe)`. Sibling
    /// blocks (`attractive services`, `disabled services`) are shaped
    /// differently and must not be read as service rows, so parsing stops at the
    /// block's closing brace.
    public static func parseLaunchctlPrintServices(_ output: String, domain: String) -> [LaunchctlRecord] {
        var records: [LaunchctlRecord] = []
        var insideServices = false

        for rawLine in output.split(whereSeparator: \.isNewline) {
            let trimmed = rawLine.trimmingCharacters(in: .whitespaces)
            guard insideServices else {
                if trimmed == "services = {" { insideServices = true }
                continue
            }
            if trimmed == "}" { break }

            let fields = trimmed.split(whereSeparator: { $0 == " " || $0 == "\t" }).map(String.init)
            guard fields.count >= 3, !fields[2].isEmpty else { continue }
            // PID 0 means "loaded, not running" here — the same state
            // `launchctl list` spells `-`.
            let pid = Int(fields[0]).flatMap { $0 > 0 ? $0 : nil }
            records.append(
                LaunchctlRecord(
                    label: fields[2],
                    pid: pid,
                    lastExitStatus: Int(fields[1]),
                    domain: domain
                )
            )
        }

        return records
    }

    /// Parses `brew services list` output: Name, Status, User, File.
    public static func parseBrewServices(_ output: String) -> [BrewServiceRecord] {
        output.split(whereSeparator: \.isNewline).compactMap { rawLine in
            let fields = rawLine.split(whereSeparator: { $0 == " " || $0 == "\t" }).map(String.init)
            guard fields.count >= 2, fields[0].lowercased() != "name" else { return nil }
            let name = fields[0]
            let status = fields[1]
            let user = fields.count > 2 ? fields[2] : ""
            let file = fields.count > 3 ? fields.dropFirst(3).joined(separator: " ") : nil
            let label = file.flatMap(Self.plistLabel(from:)) ?? "homebrew.mxcl.\(name)"
            return BrewServiceRecord(name: name, status: status, user: user, file: file, label: label)
        }
    }

    /// Parses one launchd plist. Invalid or non-dictionary property lists return nil.
    public static func parsePlist(_ data: Data, path: String, domain: String) -> PlistRecord? {
        guard let object = try? PropertyListSerialization.propertyList(from: data, options: [], format: nil),
              let dictionary = object as? [String: Any] else {
            return nil
        }
        let fallback = URL(fileURLWithPath: path).deletingPathExtension().lastPathComponent
        let rawLabel = (dictionary["Label"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines)
        let label = if let rawLabel, !rawLabel.isEmpty { rawLabel } else { fallback }
        guard !label.isEmpty else { return nil }
        let keepAlive = plistBool(dictionary["KeepAlive"])
        let runAtLoad = plistBool(dictionary["RunAtLoad"])
        let origin = label.hasPrefix("homebrew.mxcl.") || path.contains("homebrew.mxcl.") ? "brew" : "manual"
        let programNotes = (dictionary["ProgramArguments"] as? [String])?.isEmpty == false
            ? ["program: \((dictionary["ProgramArguments"] as? [String] ?? []).joined(separator: " "))"]
            : []
        return PlistRecord(
            label: label,
            domain: domain,
            origin: origin,
            keepAlive: keepAlive,
            runAtLoad: runAtLoad,
            notes: programNotes
        )
    }

    public static func annotatePorts(_ daemons: [Daemon], ports: [Port]) -> [Daemon] {
        daemons.map { daemon in
            var copy = daemon
            if let pid = daemon.pid {
                copy.ports = ports.filter { $0.pid == pid }.map(\.port).sorted()
            }
            return copy
        }
    }

    public static func plistDirectories(home: String = FileManager.default.homeDirectoryForCurrentUser.path) -> [String] {
        [
            (home as NSString).appendingPathComponent("Library/LaunchAgents"),
            "/Library/LaunchAgents",
            "/Library/LaunchDaemons",
        ]
    }

    private static func finalize(
        _ rows: [Daemon],
        all: Bool,
        ports: [Port],
        notes: [String]
    ) -> ([Daemon], [String]) {
        let filtered = all ? rows : rows.filter { !$0.label.hasPrefix("com.apple.") }
        let annotated = annotatePorts(filtered, ports: ports)
        let sorted = annotated.sorted {
            let lhsFailed = ($0.lastExitStatus ?? 0) != 0
            let rhsFailed = ($1.lastExitStatus ?? 0) != 0
            if lhsFailed != rhsFailed { return lhsFailed && !rhsFailed }
            if $0.domain != $1.domain { return $0.domain < $1.domain }
            return $0.label.localizedStandardCompare($1.label) == .orderedAscending
        }
        return (sorted, notes)
    }

    private static func plistBool(_ value: Any?) -> Bool? {
        if let bool = value as? Bool { return bool }
        if let number = value as? NSNumber { return number.boolValue }
        if value is [String: Any] { return true }
        return nil
    }

    private static func plistLabel(from file: String) -> String? {
        let name = URL(fileURLWithPath: file).deletingPathExtension().lastPathComponent
        return name.isEmpty ? nil : name
    }

    private static func preferredOrigin(_ current: String, _ candidate: String) -> String {
        if candidate == "brew" || current == "brew" { return "brew" }
        if candidate == "manual" || current == "manual" { return "manual" }
        return current
    }

    private static func mergeNotes(_ current: [String], _ additions: [String]) -> [String] {
        current + additions.filter { !current.contains($0) }
    }
}

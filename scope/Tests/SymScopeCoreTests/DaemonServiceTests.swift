import XCTest
@testable import SymScopeCore

private struct FixtureCommandRunner: DaemonCommandRunning {
    let outputs: [String: String]

    func run(executable: String, arguments: [String]) throws -> String {
        let key = ([executable] + arguments).joined(separator: " ")
        guard let output = outputs[key] else {
            throw DaemonCommandError.nonZero(127)
        }
        return output
    }
}

private final class CountingCommandRunner: DaemonCommandRunning, @unchecked Sendable {
    let outputs: [String: String]
    private let lock = NSLock()
    private var invocations = 0

    init(outputs: [String: String]) {
        self.outputs = outputs
    }

    func run(executable: String, arguments: [String]) throws -> String {
        lock.lock()
        invocations += 1
        lock.unlock()

        let key = ([executable] + arguments).joined(separator: " ")
        guard let output = outputs[key] else {
            throw DaemonCommandError.nonZero(127)
        }
        return output
    }

    var invocationCount: Int {
        lock.lock()
        defer { lock.unlock() }
        return invocations
    }
}

private struct FixtureFileSystem: DaemonFileSystemReading {
    let directories: [String: [String]]
    let files: [String: Data]
    let executables: Set<String>

    func directoryContents(atPath path: String) -> [String] {
        directories[path] ?? []
    }

    func data(atPath path: String) -> Data? {
        files[path]
    }

    func isExecutableFile(atPath path: String) -> Bool {
        executables.contains(path)
    }
}

/// Records how many collectors were inside at the same time.
///
/// Each caller waits until `expected` of them have arrived, so a genuinely
/// concurrent implementation reaches the mark deterministically rather than by
/// luck. A serial one cannot: each caller waits out the grace period alone,
/// the peak stays at one, and the assertion fails instead of the suite hanging.
/// Once the mark has been reached the gate latches open, so stragglers do not
/// pay the grace period on a passing run.
private final class ConcurrencyWitness: @unchecked Sendable {
    private let lock = NSLock()
    private let gate = DispatchSemaphore(value: 0)
    private let expected: Int
    private var active = 0
    private var highWater = 0
    private var latched = false

    init(expected: Int) {
        self.expected = expected
    }

    func witness() {
        lock.lock()
        active += 1
        highWater = max(highWater, active)
        let reached = active >= expected
        if reached { latched = true }
        let open = latched
        lock.unlock()

        if reached {
            for _ in 0..<expected { gate.signal() }
        } else if !open {
            _ = gate.wait(timeout: .now() + 1)
        }

        lock.lock()
        active -= 1
        lock.unlock()
    }

    var peak: Int {
        lock.lock()
        defer { lock.unlock() }
        return highWater
    }
}

private struct WitnessingCommandRunner: DaemonCommandRunning {
    let outputs: [String: String]
    let witness: ConcurrencyWitness

    func run(executable: String, arguments: [String]) throws -> String {
        witness.witness()
        let key = ([executable] + arguments).joined(separator: " ")
        guard let output = outputs[key] else {
            throw DaemonCommandError.nonZero(127)
        }
        return output
    }
}

final class DaemonParserTests: XCTestCase {
    func testParseLaunchctlList() {
        let output = """
        PID\tStatus\tLabel
        123\t0\tcom.example.worker
        -\t7\tcom.example.failed
        -\t0\tcom.apple.WindowServer
        """

        let records = DaemonService.parseLaunchctlList(output, domain: "user")

        XCTAssertEqual(records, [
            LaunchctlRecord(label: "com.example.worker", pid: 123, lastExitStatus: 0, domain: "user"),
            LaunchctlRecord(label: "com.example.failed", pid: nil, lastExitStatus: 7, domain: "user"),
            LaunchctlRecord(label: "com.apple.WindowServer", pid: nil, lastExitStatus: 0, domain: "user"),
        ])
    }

    func testParseBrewServices() {
        let output = """
        Name              Status  User  File
        redis             started daniel ~/Library/LaunchAgents/homebrew.mxcl.redis.plist
        postgresql@16     stopped daniel ~/Library/LaunchAgents/homebrew.mxcl.postgresql@16.plist
        """

        let records = DaemonService.parseBrewServices(output)

        XCTAssertEqual(records.count, 2)
        XCTAssertEqual(records[0].name, "redis")
        XCTAssertEqual(records[0].status, "started")
        XCTAssertEqual(records[0].label, "homebrew.mxcl.redis")
        XCTAssertEqual(records[1].label, "homebrew.mxcl.postgresql@16")
    }

    func testMalformedPlistDegradesToNil() {
        XCTAssertNil(
            DaemonService.parsePlist(
                Data("not a plist".utf8),
                path: "/fixture/home/Library/LaunchAgents/broken.plist",
                domain: "user"
            )
        )
    }

    func testListMergesSourcesFiltersAppleSortsFailuresAndLinksPorts() async {
        let home = "/fixture/home"
        let userAgents = "\(home)/Library/LaunchAgents"
        let redisPath = "\(userAgents)/homebrew.mxcl.redis.plist"
        let brokenPath = "\(userAgents)/broken.plist"
        let runner = FixtureCommandRunner(outputs: [
            "/bin/launchctl list": """
            PID Status Label
            55 0 homebrew.mxcl.redis
            - 7 com.example.failed
            99 0 com.apple.WindowServer
            """,
            "/bin/launchctl print system": "",
            "/opt/homebrew/bin/brew services list": """
            Name Status User File
            redis started daniel ~/Library/LaunchAgents/homebrew.mxcl.redis.plist
            """,
        ])
        let fileSystem = FixtureFileSystem(
            directories: [
                userAgents: ["homebrew.mxcl.redis.plist", "broken.plist"],
                "/Library/LaunchAgents": [],
                "/Library/LaunchDaemons": [],
            ],
            files: [
                redisPath: Self.plistData([
                    "Label": "homebrew.mxcl.redis",
                    "ProgramArguments": ["/opt/homebrew/bin/redis-server", "--port", "6379"],
                    "KeepAlive": true,
                    "RunAtLoad": true,
                ]),
                brokenPath: Data("broken".utf8),
            ],
            executables: ["/opt/homebrew/bin/brew"]
        )
        let service = DaemonService(
            commandRunner: runner,
            fileSystem: fileSystem,
            portProvider: {
                [Port(port: 6379, protocol_: "tcp", address: "127.0.0.1", pid: 55, process: "redis-server")]
            },
            homeDirectory: home
        )

        let (rows, notes) = await service.list()

        XCTAssertEqual(rows.map(\.label), ["com.example.failed", "homebrew.mxcl.redis"])
        XCTAssertEqual(rows[0].lastExitStatus, 7)
        XCTAssertEqual(rows[1].origin, "brew")
        XCTAssertEqual(rows[1].state, "running")
        XCTAssertEqual(rows[1].keepAlive, true)
        XCTAssertEqual(rows[1].runAtLoad, true)
        XCTAssertEqual(rows[1].ports, [6379])
        XCTAssertTrue(rows[1].notes.contains { $0.contains("program: /opt/homebrew/bin/redis-server --port 6379") })
        XCTAssertTrue(notes.contains { $0.contains("plist malformed") })
        XCTAssertFalse(rows.contains { $0.label.hasPrefix("com.apple.") })

        let (allRows, _) = await service.list(all: true)
        XCTAssertTrue(allRows.contains { $0.label == "com.apple.WindowServer" })

        let health = DaemonService.health(rows)
        XCTAssertFalse(health[0].healthy)
        XCTAssertTrue(health[1].healthy)
    }

    func testCollectorsRunConcurrentlyRatherThanWaitingOutBrew() async {
        // `brew services list` is a Ruby process and is roughly 95% of this
        // call's wall time. launchd's inventory, the plist scan and the port
        // inventory do not depend on it, so they have to run beside it rather
        // than after it. The witness makes that structural rather than timed:
        // it only lets callers through once two of them are inside at once.
        let witness = ConcurrencyWitness(expected: 2)
        let runner = WitnessingCommandRunner(
            outputs: [
                "/bin/launchctl list": "PID Status Label\n55 0 com.example.worker\n",
                "/bin/launchctl print system": "",
                "/opt/homebrew/bin/brew services list":
                    "Name Status User File\nredis started daniel ~/Library/LaunchAgents/homebrew.mxcl.redis.plist\n",
            ],
            witness: witness
        )
        let fileSystem = FixtureFileSystem(
            directories: [:],
            files: [:],
            executables: ["/opt/homebrew/bin/brew"]
        )
        let service = DaemonService(
            commandRunner: runner,
            fileSystem: fileSystem,
            portProvider: {
                witness.witness()
                return []
            },
            homeDirectory: "/fixture/home"
        )

        let (rows, notes) = await service.list()

        XCTAssertGreaterThanOrEqual(
            witness.peak, 2,
            "launchd, Homebrew and the port inventory must overlap, not run one after another"
        )
        // The merged result is unchanged by running the collectors together.
        XCTAssertEqual(rows.map(\.label).sorted(), ["com.example.worker", "homebrew.mxcl.redis"])
        XCTAssertTrue(notes.isEmpty, "a healthy fixture degrades nowhere: \(notes)")
    }

    func testHealthPrefersLivePIDOverHistoricalExitStatus() {
        let daemons = [
            Daemon(label: "running.nonzero", state: "running", pid: 42, lastExitStatus: 143, domain: "user"),
            Daemon(label: "running.zero", state: "running", pid: 43, lastExitStatus: 0, domain: "user"),
            Daemon(label: "stopped.nonzero", state: "loading", pid: nil, lastExitStatus: 255, domain: "user"),
            Daemon(label: "not.loaded", state: "not-loaded", pid: nil, lastExitStatus: nil, domain: "user"),
        ]

        let health = DaemonService.health(daemons)
        let byLabel = Dictionary(uniqueKeysWithValues: health.map { ($0.label, $0) })

        XCTAssertTrue(byLabel["running.nonzero"]!.healthy, "a live PID must not be marked unhealthy by a stale prior exit status")
        XCTAssertEqual(byLabel["running.nonzero"]!.lastExitStatus, 143, "historical exit status stays available for diagnosis")
        XCTAssertTrue(byLabel["running.zero"]!.healthy)
        XCTAssertFalse(byLabel["stopped.nonzero"]!.healthy)
        XCTAssertFalse(byLabel["not.loaded"]!.healthy)
        XCTAssertTrue(byLabel["not.loaded"]!.notes.contains { $0.contains("inactive by design") }, "not-loaded is presented as intentional, not a bare failure")
    }

    func testMissingHomebrewIsOnlyANote() async {
        let runner = FixtureCommandRunner(outputs: [
            "/bin/launchctl list": "PID Status Label\n- 0 com.example.worker\n",
            "/bin/launchctl print system": "",
        ])
        let fileSystem = FixtureFileSystem(directories: [:], files: [:], executables: [])
        let service = DaemonService(
            commandRunner: runner,
            fileSystem: fileSystem,
            portProvider: { [] },
            homeDirectory: "/fixture/home"
        )

        let (rows, notes) = await service.list()

        XCTAssertEqual(rows.map(\.label), ["com.example.worker"])
        XCTAssertTrue(notes.contains { $0.contains("brew CLI not found") })
    }

    func testConflictsIncludeLaunchdLabelForKnownPID() {
        let ports = [
            Port(port: 8080, protocol_: "tcp", address: "127.0.0.1", pid: 42, process: "worker"),
            Port(port: 8080, protocol_: "tcp", address: "0.0.0.0", pid: 43, process: "other"),
        ]
        let daemons = [Daemon(label: "com.example.worker", state: "running", pid: 42, domain: "user")]

        let conflicts = ConflictDetector.detect(ports, daemons: daemons)

        XCTAssertTrue(conflicts[0].holders.contains { $0.contains("com.example.worker") })
    }

    /// `launchctl print system` is the only unprivileged way to enumerate the
    /// system domain. Its `services = { … }` block carries the same
    /// PID / status / label triples as `launchctl list`, with two differences
    /// that matter: a PID of `0` means "loaded but not running" (where
    /// `launchctl list` prints `-`), and the status column can hold a
    /// non-numeric annotation such as `(pe)`.
    func testParseLaunchctlPrintServices() {
        let output = """
        system = {
        \tactive count = 1035
        \tservices = {
        \t\t       0      - \tcom.apple.rpmuxd
        \t\t       0   (pe) \tcom.apple.kernelmanager_helper
        \t\t     627      - \tcom.objective-see.blockblock
        \t\t    2931    255 \tcom.cloudflare.cloudflared
        \t\t       0      0 \tcom.microsoft.autoupdate.helper
        \t}
        \tattractive services = {
        \t\tcom.apple.MessagesBlastDoorService
        \t}
        \tdisabled services = {
        \t\t"com.apple.ftpd" => disabled
        \t}
        }
        """

        let records = DaemonService.parseLaunchctlPrintServices(output, domain: "system")

        XCTAssertEqual(records, [
            LaunchctlRecord(label: "com.apple.rpmuxd", pid: nil, lastExitStatus: nil, domain: "system"),
            LaunchctlRecord(label: "com.apple.kernelmanager_helper", pid: nil, lastExitStatus: nil, domain: "system"),
            LaunchctlRecord(label: "com.objective-see.blockblock", pid: 627, lastExitStatus: nil, domain: "system"),
            LaunchctlRecord(label: "com.cloudflare.cloudflared", pid: 2931, lastExitStatus: 255, domain: "system"),
            LaunchctlRecord(label: "com.microsoft.autoupdate.helper", pid: nil, lastExitStatus: 0, domain: "system"),
        ], "only the `services` block is parsed — the sibling blocks must not leak in")
    }

    /// Regression for #231: a running LaunchDaemon was reported `not-loaded`
    /// with no PID, because the system domain was probed with
    /// `launchctl list system` — an invocation `launchctl` rejects (it takes a
    /// label, not a domain) with exit 113. The fixture runner throws for any
    /// command it does not know, which is exactly what the real one did.
    func testSystemDomainDaemonsAreReportedRunning() async {
        let runner = FixtureCommandRunner(outputs: [
            "/bin/launchctl list": "PID Status Label\n",
            "/bin/launchctl print system": """
            system = {
            \tservices = {
            \t\t     627      - \tcom.objective-see.blockblock
            \t\t    2931    255 \tcom.cloudflare.cloudflared
            \t\t       0      0 \tcom.microsoft.autoupdate.helper
            \t}
            }
            """,
        ])
        let fileSystem = FixtureFileSystem(
            directories: [
                "/fixture/home/Library/LaunchAgents": [],
                "/Library/LaunchAgents": [],
                "/Library/LaunchDaemons": [
                    "com.objective-see.blockblock.plist",
                    "com.cloudflare.cloudflared.plist",
                    "com.microsoft.autoupdate.helper.plist",
                ],
            ],
            files: [
                "/Library/LaunchDaemons/com.objective-see.blockblock.plist":
                    Self.plistData(["Label": "com.objective-see.blockblock", "RunAtLoad": true]),
                "/Library/LaunchDaemons/com.cloudflare.cloudflared.plist":
                    Self.plistData(["Label": "com.cloudflare.cloudflared", "KeepAlive": true]),
                "/Library/LaunchDaemons/com.microsoft.autoupdate.helper.plist":
                    Self.plistData(["Label": "com.microsoft.autoupdate.helper"]),
            ],
            executables: []
        )
        let service = DaemonService(
            commandRunner: runner,
            fileSystem: fileSystem,
            portProvider: { [] },
            homeDirectory: "/fixture/home"
        )

        let (rows, _) = await service.list()
        let byLabel = Dictionary(uniqueKeysWithValues: rows.map { ($0.label, $0) })

        let blockblock = try! XCTUnwrap(byLabel["com.objective-see.blockblock"])
        XCTAssertEqual(blockblock.state, "running", "a system daemon with a live PID is running, not not-loaded")
        XCTAssertEqual(blockblock.pid, 627)
        XCTAssertEqual(blockblock.domain, "system")
        XCTAssertEqual(blockblock.runAtLoad, true, "the plist merge must still apply to the launchd row")

        let cloudflared = try! XCTUnwrap(byLabel["com.cloudflare.cloudflared"])
        XCTAssertEqual(cloudflared.state, "running")
        XCTAssertEqual(cloudflared.lastExitStatus, 255, "the failure signal must survive")

        let helper = try! XCTUnwrap(byLabel["com.microsoft.autoupdate.helper"])
        XCTAssertEqual(helper.state, "loading", "loaded but not running is not the same as not-loaded")
        XCTAssertNil(helper.pid)

        let health = Dictionary(
            uniqueKeysWithValues: DaemonService.health(rows).map { ($0.label, $0) }
        )
        XCTAssertTrue(health["com.objective-see.blockblock"]!.healthy)
        XCTAssertFalse(
            health["com.objective-see.blockblock"]!.notes.contains { $0.contains("inactive by design") },
            "a running daemon must not be described as inactive by design"
        )
    }

    /// When the system domain cannot be read at all, the plists on disk are
    /// still listed — but the failure has to be visible in `notes` rather than
    /// silently presented as "not loaded".
    func testUnreadableSystemDomainIsReportedAsANote() async {
        let runner = FixtureCommandRunner(outputs: ["/bin/launchctl list": "PID Status Label\n"])
        let fileSystem = FixtureFileSystem(
            directories: [
                "/fixture/home/Library/LaunchAgents": [],
                "/Library/LaunchAgents": [],
                "/Library/LaunchDaemons": ["com.example.daemon.plist"],
            ],
            files: [
                "/Library/LaunchDaemons/com.example.daemon.plist":
                    Self.plistData(["Label": "com.example.daemon"]),
            ],
            executables: []
        )
        let service = DaemonService(
            commandRunner: runner,
            fileSystem: fileSystem,
            portProvider: { [] },
            homeDirectory: "/fixture/home"
        )

        let (rows, notes) = await service.list()

        XCTAssertEqual(rows.map(\.label), ["com.example.daemon"])
        XCTAssertTrue(
            notes.contains { $0.contains("launchctl system unavailable") },
            "an unreadable system domain must be surfaced, not swallowed: \(notes)"
        )
    }

    func testRefreshCacheUsesLongCadenceAndForceRefresh() async {
        let runner = CountingCommandRunner(outputs: [
            "/bin/launchctl list": "PID Status Label\n55 0 com.example.worker\n",
            "/bin/launchctl print system": "",
        ])
        let service = DaemonService(
            commandRunner: runner,
            fileSystem: FixtureFileSystem(directories: [:], files: [:], executables: []),
            portProvider: { [] },
            homeDirectory: "/fixture/home"
        )
        var cache = DaemonRefreshCache()
        let start = Date(timeIntervalSince1970: 1_000_000)
        var collectorInvocations = 0

        // The five-minute window contains twenty 15-second UI ticks. The
        // initial collection plus the four elapsed 60-second intervals are
        // the only daemon collections allowed in that window.
        for tick in 0..<20 {
            let now = start.addingTimeInterval(TimeInterval(tick * 15))
            if cache.beginRefresh(at: now) {
                collectorInvocations += 1
                let result = await service.list()
                cache.update(daemons: result.0, notes: result.1)
            }

            cache.annotatePorts([
                Port(
                    port: 8_000 + tick,
                    protocol_: "tcp",
                    address: "127.0.0.1",
                    pid: 55,
                    process: "worker"
                ),
            ])
        }

        XCTAssertEqual(collectorInvocations, 5)
        XCTAssertEqual(runner.invocationCount, 10, "the command spy should only see two launchd calls per collection")
        XCTAssertEqual(cache.daemons.first?.ports, [8_019], "cached rows must be re-annotated on every port tick")

        XCTAssertFalse(cache.beginRefresh(at: start.addingTimeInterval(15)))
        XCTAssertTrue(
            cache.beginRefresh(at: start.addingTimeInterval(15), force: true),
            "manual refresh must bypass the daemon cache cadence"
        )
        collectorInvocations += 1
        let forcedResult = await service.list()
        cache.update(daemons: forcedResult.0, notes: forcedResult.1)

        XCTAssertEqual(collectorInvocations, 6)
        XCTAssertEqual(runner.invocationCount, 12)
    }

    private static func plistData(_ values: [String: Any]) -> Data {
        (try? PropertyListSerialization.data(fromPropertyList: values, format: .xml, options: 0)) ?? Data()
    }
}

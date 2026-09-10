import Foundation

/// Inventories local listening sockets via `lsof` (macOS).
///
/// The Go original used gopsutil (cross-platform); on macOS `lsof -nP -iTCP
/// -sTCP:LISTEN` plus a UDP pass is the native equivalent and needs no third
/// party dependency.
public enum PortService: Sendable {
    public static let lsofPath = "/usr/sbin/lsof"

    /// Listening TCP + bound UDP sockets with owning process where available.
    public static func listListening() async throws -> [Port] {
        await listListeningReport().ports
    }

    /// Collects listeners while preserving non-fatal subprocess degradation
    /// notes for aggregate callers such as `scope scan`.
    public static func listListeningReport() async -> (ports: [Port], notes: [String]) {
        var notes: [String] = []
        async let tcpTask = try? await runLsof(args: ["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pcnP"])
        async let udpTask = try? await runLsof(args: ["-nP", "-iUDP", "-F", "pcnP"])

        guard let tcpOutput = await tcpTask else {
            return ([], ["ports: lsof unavailable"])
        }
        var ports = tcpOutput.timedOut ? [] : parseLsofOutput(tcpOutput.output, protocol_: "tcp")
        if tcpOutput.timedOut {
            notes.append("ports: lsof TCP pass timed out; listener inventory is partial")
        }

        // UDP (bound sockets). `lsof -iUDP` also reports connected sockets;
        // parseLsofOutput rejects those before they can look like listeners.
        guard let udpOutput = await udpTask else {
            notes.append("ports: lsof UDP pass unavailable; listener inventory is partial")
            return (collapse(ports), notes)
        }
        if udpOutput.timedOut {
            notes.append("ports: lsof UDP pass timed out; listener inventory is partial")
        } else {
            ports += parseLsofOutput(udpOutput.output, protocol_: "udp")
        }

        // Collapse duplicate (port, proto, pid) pairs — lsof reports one line
        // per file descriptor and IPv4/IPv6 duplicates happen.
        return (collapse(ports), notes)
    }

    /// Suggest `count` free TCP ports in [rangeStart, rangeEnd], checking
    /// liveness before returning.
    public static func suggestFree(count: Int = 3, rangeStart: Int = 49152, rangeEnd: Int = 65535) async throws -> [Int] {
        let inUse = Set(try await listListening().map(\.port))
        var result: [Int] = []
        var candidate = rangeStart
        while result.count < count && candidate <= rangeEnd {
            if !inUse.contains(candidate), !isPortOpen(candidate) {
                result.append(candidate)
            }
            candidate += 1
        }
        return result
    }

    // MARK: - Internals

    private static func runLsof(args: [String]) async throws -> BoundedProcessResult {
        try await BoundedProcessRunner.runAsync(executable: lsofPath, arguments: args, timeoutSeconds: 3)
    }

    /// Parses `lsof -F` output. Fields are prefixed: p=PID, c=command,
    /// n=address:port, P=protocol name.
    static func parseLsofOutput(_ output: String, protocol_: String) -> [Port] {
        // lsof -F output groups records separated by newline; each field line
        // starts with its field char. A record begins with 'p' (pid).
        var result: [Port] = []
        var currentPID: Int = 0
        var currentCommand = ""
        var currentAddr = ""

        func flush() {
            guard currentPID > 0, !currentAddr.isEmpty else { return }
            guard let parsed = parseAddress(currentAddr) else { return }
            result.append(
                Port(
                    port: parsed.port,
                    protocol_: protocol_,
                    address: parsed.address,
                    pid: currentPID,
                    process: currentCommand.isEmpty ? "unknown" : currentCommand
                )
            )
        }

        for rawLine in output.split(separator: "\n") {
            let line = String(rawLine)
            guard let first = line.first else { continue }
            let value = String(line.dropFirst())
            switch first {
            case "p":
                flush()
                currentPID = Int(value) ?? 0
                currentCommand = ""
                currentAddr = ""
            case "c":
                currentCommand = value
            case "f":
                // A new file descriptor starts a new n/P pair within the same
                // p record. Flush the previous descriptor's address (if any)
                // before resetting; keep the pid/command, which are shared by
                // every descriptor in this record.
                flush()
                currentAddr = ""
            case "n":
                currentAddr = value
            default:
                break
            }
        }
        flush()
        return result
    }

    /// "127.0.0.1:8080" or "[::1]:8080" or "*:8080" → (port, address).
    static func parseAddress(_ value: String) -> (port: Int, address: String)? {
        // Connected UDP records contain a local and remote endpoint. They are
        // not listeners, and splitting them on the last colon would report the
        // remote port as if it were bound locally.
        guard !value.contains("->") else { return nil }
        if value.hasPrefix("[") {
            // IPv6 [addr]:port
            if let close = value.firstIndex(of: "]") {
                let addr = String(value[value.index(after: value.startIndex)..<close])
                let after = value[value.index(after: close)...]
                if after.hasPrefix(":") {
                    let portStr = String(after.dropFirst())
                    if let port = Int(portStr) {
                        return (port, addr)
                    }
                }
            }
            return nil
        }
        // IPv4 or hostname:port — split on last colon
        if let colon = value.lastIndex(of: ":") {
            let addr = String(value[..<colon])
            let portStr = String(value[value.index(after: colon)...])
            if let port = Int(portStr) {
                return (port, addr)
            }
        }
        return nil
    }

    private static func collapse(_ ports: [Port]) -> [Port] {
        var seen = Set<String>()
        var out: [Port] = []
        for p in ports {
            let key = "\(p.port)/\(p.protocol_)/\(p.pid)"
            if seen.insert(key).inserted {
                out.append(p)
            }
        }
        return out.sorted { $0.port < $1.port }
    }

    /// Probes a loopback TCP port. Internal rather than private so the
    /// descriptor-safety regression test can drive it directly.
    static func isPortOpen(_ port: Int) -> Bool {
        let socketFD = Darwin.socket(AF_INET, SOCK_STREAM, 0)
        guard socketFD >= 0 else { return true }
        defer { Darwin.close(socketFD) }
        var addr = sockaddr_in()
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = CFSwapInt16HostToBig(UInt16(port))
        addr.sin_addr.s_addr = inet_addr("127.0.0.1")
        let rc = withUnsafePointer(to: &addr) { ptr in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                Darwin.connect(socketFD, sa, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        // The `defer` above owns this descriptor. Closing it here as well
        // would leave a closed fd number that the kernel immediately reuses
        // for the next open in this process, and the deferred close would
        // then destroy that unrelated descriptor.
        return rc == 0
    }
}

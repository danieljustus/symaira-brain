import Foundation

/// Container discovery via the local `docker` CLI (mirrors the Go original's
/// shell-out approach — no SDK dependency, works with any Docker-compatible
/// daemon).
public enum ContainerService: Sendable {
    public static let dockerPath = "/usr/local/bin/docker"

    /// Lists running containers with published ports.
    /// Returns (containers, notes); missing docker binary is a note, not an error.
    public static func list() async -> ([Container], [String]) {
        guard FileManager.default.isExecutableFile(atPath: dockerPath) ||
                FileManager.default.isExecutableFile(atPath: "/opt/homebrew/bin/docker") ||
                FileManager.default.isExecutableFile(atPath: "/usr/bin/docker") else {
            return ([], ["docker CLI not found; container inventory unavailable"])
        }
        let binary = resolveDockerBinary()
        let args = ["ps", "--format", "{{.ID}}\t{{.Names}}\t{{.Image}}\t{{.Ports}}"]
        guard let result = try? await run(binary: binary, args: args) else {
            return ([], ["docker ps failed; container inventory unavailable"])
        }
        if result.timedOut {
            return ([], ["docker ps timed out; container inventory unavailable"])
        }
        guard result.terminationStatus == 0 else {
            return ([], ["docker ps failed; container inventory unavailable"])
        }
        let output = result.output
        var containers: [Container] = []
        for line in output.split(separator: "\n") {
            let parts = line.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
            guard parts.count >= 3 else { continue }
            let id = parts[0]
            let name = parts[1]
            let image = parts[2]
            let portsField = parts.count > 3 ? parts[3] : ""
            let ports = parsePublishedPorts(portsField)
            containers.append(Container(id: id, name: name, image: image, ports: ports))
        }
        return (containers, [])
    }

    // MARK: - Internals

    private static func resolveDockerBinary() -> String {
        for candidate in [dockerPath, "/opt/homebrew/bin/docker", "/usr/bin/docker"] {
            if FileManager.default.isExecutableFile(atPath: candidate) {
                return candidate
            }
        }
        return dockerPath
    }

    private static func run(binary: String, args: [String]) async throws -> BoundedProcessResult {
        try await BoundedProcessRunner.runAsync(executable: binary, arguments: args, timeoutSeconds: 10)
    }

    /// "0.0.0.0:8080->80/tcp, :::8080->80/tcp" → [8080]
    static func parsePublishedPorts(_ field: String) -> [Int] {
        var result: [Int] = []
        for entry in field.split(separator: ",") {
            let e = entry.trimmingCharacters(in: .whitespaces)
            // host:port->container/tcp — take the host part before "->"
            let hostPart: Substring
            if let arrow = e.range(of: "->") {
                hostPart = e[..<arrow.lowerBound]
            } else {
                hostPart = Substring(e)
            }
            // Extract the LAST colon-separated token that is a pure integer.
            // Handles "0.0.0.0:8080", ":::8080" (IPv6 wildcard), "[::]:8080".
            var port: Int?
            let cleaned = hostPart.replacingOccurrences(of: "]:", with: ":")
            if let colon = cleaned.lastIndex(of: ":") {
                let portStr = String(cleaned[cleaned.index(after: colon)...])
                if let p = Int(portStr), p > 0, p < 65536 {
                    port = p
                }
            }
            if let port, !result.contains(port) {
                result.append(port)
            }
        }
        return result.sorted()
    }
}

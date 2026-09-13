import Foundation

/// Detects ports held by multiple processes (process-process conflicts).
public enum ConflictDetector: Sendable {
    public static func detect(_ ports: [Port], daemons: [Daemon] = []) -> [Conflict] {
        var byPort: [Int: [Port]] = [:]
        for p in ports {
            byPort[p.port, default: []].append(p)
        }
        var conflicts: [Conflict] = []
        for (port, holders) in byPort.sorted(by: { $0.key < $1.key }) where holders.count > 1 {
            let distinctPIDs = Set(holders.map(\.pid))
            guard distinctPIDs.count > 1 else { continue }
            let labels = holders.map { port in
                let processLabel = "\(port.process)(\(port.pid))"
                guard let daemon = daemons.first(where: { $0.pid == port.pid }) else {
                    return processLabel
                }
                return "\(processLabel) · \(daemon.label)"
            }
            conflicts.append(Conflict(port: port, holders: Array(Set(labels)).sorted(), kind: "process-process"))
        }
        return conflicts
    }
}

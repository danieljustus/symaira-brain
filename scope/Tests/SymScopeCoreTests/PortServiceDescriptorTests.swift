import Darwin
import Foundation
import XCTest
@testable import SymScopeCore

/// `isPortOpen` used to close its probe socket explicitly *and* in a `defer`.
/// The kernel hands out the lowest free descriptor number, so a descriptor
/// opened between the two closes inherits the just-freed number and is then
/// destroyed by the deferred close. `suggestFree` calls the probe up to
/// 16,384 times per invocation, inside processes that hold other sockets and
/// files on other threads, so the corruption is rare, silent and very hard to
/// attribute.
final class PortServiceDescriptorTests: XCTestCase {
    /// Reproduces the hazard directly: probe on one thread while another
    /// thread opens descriptors. The kernel hands out the lowest free number,
    /// so an `open` landing in the window between the explicit close and the
    /// deferred close inherits the probe socket's number and is then destroyed
    /// by that deferred close. The opener validates each descriptor it just
    /// received, so a descriptor killed underneath it is detected.
    func testConcurrentOpensSurviveProbing() throws {
        let file = FileManager.default.temporaryDirectory
            .appendingPathComponent("portservice-fd-\(UUID().uuidString).txt")
        try Data("symaira".utf8).write(to: file)
        defer { try? FileManager.default.removeItem(at: file) }

        let corrupted = Corruption()
        let stop = Flag()
        let probing = DispatchQueue(label: "probe")
        let opening = DispatchQueue(label: "open")
        let group = DispatchGroup()

        probing.async(group: group) {
            var port = 49152
            while !stop.isSet {
                _ = PortService.isPortOpen(port)
                port = port < 65535 ? port + 1 : 49152
            }
        }

        opening.async(group: group) {
            for _ in 0..<200_000 where !corrupted.isSet {
                let fd = Darwin.open(file.path, O_RDONLY)
                guard fd >= 0 else { continue }
                // The descriptor was valid a moment ago by construction. If it
                // is invalid now, something else closed it.
                if Darwin.fcntl(fd, F_GETFD) == -1 {
                    corrupted.record("descriptor \(fd) was closed by the probe thread")
                }
                Darwin.close(fd)
            }
            stop.set()
        }

        group.wait()

        if let detail = corrupted.detail {
            XCTFail("isPortOpen destroyed an unrelated descriptor: \(detail)")
        }
    }

    /// A descriptor held across many probes must stay valid and readable.
    func testProbingDoesNotDestroyUnrelatedDescriptors() throws {
        let file = FileManager.default.temporaryDirectory
            .appendingPathComponent("portservice-fd-\(UUID().uuidString).txt")
        try Data("symaira".utf8).write(to: file)
        defer { try? FileManager.default.removeItem(at: file) }

        let guarded = Darwin.open(file.path, O_RDONLY)
        try XCTSkipIf(guarded < 0, "could not open the fixture file")
        defer { Darwin.close(guarded) }

        for port in 49152..<49452 {
            _ = PortService.isPortOpen(port)
        }

        XCTAssertNotEqual(
            Darwin.fcntl(guarded, F_GETFD), -1,
            "an unrelated descriptor was closed by isPortOpen (errno \(errno))"
        )

        var byte: UInt8 = 0
        XCTAssertEqual(Darwin.read(guarded, &byte, 1), 1, "unrelated descriptor no longer readable")
    }

    /// The probe must keep reporting a listening port as open.
    func testProbeDetectsAListeningPort() throws {
        let listener = Darwin.socket(AF_INET, SOCK_STREAM, 0)
        try XCTSkipIf(listener < 0, "could not create a listening socket")
        defer { Darwin.close(listener) }

        var reuse: Int32 = 1
        Darwin.setsockopt(listener, SOL_SOCKET, SO_REUSEADDR, &reuse, socklen_t(MemoryLayout<Int32>.size))

        var addr = sockaddr_in()
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = 0 // let the kernel choose
        addr.sin_addr.s_addr = inet_addr("127.0.0.1")

        let bound = withUnsafePointer(to: &addr) { ptr in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                Darwin.bind(listener, sa, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
        try XCTSkipIf(bound != 0, "could not bind a loopback socket")
        try XCTSkipIf(Darwin.listen(listener, 1) != 0, "could not listen")

        var actual = sockaddr_in()
        var length = socklen_t(MemoryLayout<sockaddr_in>.size)
        let named = withUnsafeMutablePointer(to: &actual) { ptr in
            ptr.withMemoryRebound(to: sockaddr.self, capacity: 1) { sa in
                Darwin.getsockname(listener, sa, &length)
            }
        }
        try XCTSkipIf(named != 0, "could not read the bound port")

        let port = Int(CFSwapInt16BigToHost(actual.sin_port))
        XCTAssertTrue(PortService.isPortOpen(port), "a listening port must probe as open")
    }
}

/// Minimal thread-safe flags for the concurrency test above.
private final class Flag: @unchecked Sendable {
    private let lock = NSLock()
    private var value = false

    var isSet: Bool {
        lock.lock(); defer { lock.unlock() }
        return value
    }

    func set() {
        lock.lock(); value = true; lock.unlock()
    }
}

private final class Corruption: @unchecked Sendable {
    private let lock = NSLock()
    private var value: String?

    var isSet: Bool { detail != nil }

    var detail: String? {
        lock.lock(); defer { lock.unlock() }
        return value
    }

    func record(_ detail: String) {
        lock.lock()
        if value == nil { value = detail }
        lock.unlock()
    }
}

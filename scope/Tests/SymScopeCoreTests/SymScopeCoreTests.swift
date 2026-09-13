import XCTest
@testable import SymScopeCore

final class PortParseTests: XCTestCase {
    func testParseIPv4Address() {
        let parsed = PortService.parseAddress("127.0.0.1:8080")
        XCTAssertEqual(parsed?.port, 8080)
        XCTAssertEqual(parsed?.address, "127.0.0.1")
    }

    func testParseWildcard() {
        let parsed = PortService.parseAddress("*:5000")
        XCTAssertEqual(parsed?.port, 5000)
        XCTAssertEqual(parsed?.address, "*")
    }

    func testParseIPv6() {
        let parsed = PortService.parseAddress("[::1]:9000")
        XCTAssertEqual(parsed?.port, 9000)
        XCTAssertEqual(parsed?.address, "::1")
    }

    func testParseConnectedIPv4IsRejected() {
        XCTAssertNil(PortService.parseAddress("192.168.188.73:64067->160.79.104.10:443"))
    }

    func testParseConnectedIPv6IsRejected() {
        XCTAssertNil(PortService.parseAddress("[::1]:64067->[2606:4700::1]:443"))
    }

    func testParseInvalid() {
        XCTAssertNil(PortService.parseAddress("not-a-port"))
        XCTAssertNil(PortService.parseAddress(":abc"))
    }

    func testParseLsofOutput() {
        let output = """
        p123
        cnode
        n127.0.0.1:8080
        p456
        cpython
        n0.0.0.0:5000
        """
        let ports = PortService.parseLsofOutput(output, protocol_: "tcp")
        XCTAssertEqual(ports.count, 2)
        XCTAssertEqual(ports[0].pid, 123)
        XCTAssertEqual(ports[0].process, "node")
        XCTAssertEqual(ports[0].port, 8080)
        XCTAssertEqual(ports[1].pid, 456)
        XCTAssertEqual(ports[1].process, "python")
        XCTAssertEqual(ports[1].port, 5000)
    }

    func testParseLsofSkipsEntriesWithoutAddress() {
        let output = """
        p123
        cnode
        p456
        cpython
        n127.0.0.1:9000
        """
        let ports = PortService.parseLsofOutput(output, protocol_: "tcp")
        XCTAssertEqual(ports.count, 1)
        XCTAssertEqual(ports[0].pid, 456)
    }

    /// Regression for #192: real `lsof -F pcnP` output emits one `f`/`P`/`n`
    /// triple per file descriptor within a single `p` record. Before the fix,
    /// only the flush on a new `p` (or EOF) emitted a port, so every address
    /// but the last for a process was silently overwritten and dropped.
    func testParseLsofOutputEmitsEveryDescriptorInARecord() {
        let output = """
        p789
        cnode
        f4
        PTCP
        n127.0.0.1:8080
        f6
        PTCP
        n*:9090
        f8
        PTCP
        n0.0.0.0:3000
        """
        let ports = PortService.parseLsofOutput(output, protocol_: "tcp")
        XCTAssertEqual(ports.count, 3)
        XCTAssertTrue(ports.allSatisfy { $0.pid == 789 && $0.process == "node" })
        XCTAssertEqual(Set(ports.map(\.port)), [8080, 9090, 3000])
    }
}

final class ContainerPortParseTests: XCTestCase {
    func testParsePublishedPortsSimple() {
        let ports = ContainerService.parsePublishedPorts("0.0.0.0:8080->80/tcp")
        XCTAssertEqual(ports, [8080])
    }

    func testParsePublishedPortsIPv6() {
        let ports = ContainerService.parsePublishedPorts(":::8080->80/tcp")
        XCTAssertEqual(ports, [8080])
    }

    func testParseMultiple() {
        let ports = ContainerService.parsePublishedPorts("0.0.0.0:8080->80/tcp, 0.0.0.0:8443->443/tcp")
        XCTAssertEqual(ports, [8080, 8443])
    }

    func testParseNoPorts() {
        XCTAssertEqual(ContainerService.parsePublishedPorts(""), [])
        XCTAssertEqual(ContainerService.parsePublishedPorts("80/tcp"), [])
    }

    func testParseDeduplicatesAndSorts() {
        let ports = ContainerService.parsePublishedPorts("0.0.0.0:9090->90/tcp, 0.0.0.0:8080->80/tcp, [::]:8080->80/tcp")
        XCTAssertEqual(ports, [8080, 9090])
    }
}

final class ContainerServiceListTests: XCTestCase {
    /// `list()` shells out to the local `docker` CLI. Whether or not docker
    /// is installed on the machine running this test, the contract is fixed:
    /// either a well-formed container inventory, or an explanatory note —
    /// never a crash or a silently-swallowed failure.
    func testListReturnsInventoryOrExplanatoryNote() async {
        let (containers, notes) = await ContainerService.list()

        if containers.isEmpty {
            for note in notes {
                XCTAssertTrue(
                    note == "docker CLI not found; container inventory unavailable"
                        || note == "docker ps failed; container inventory unavailable"
                        || notes.isEmpty,
                    "unexpected note: \(note)"
                )
            }
        } else {
            XCTAssertTrue(notes.isEmpty)
            for container in containers {
                XCTAssertFalse(container.id.isEmpty)
                XCTAssertFalse(container.name.isEmpty)
                XCTAssertFalse(container.image.isEmpty)
                XCTAssertTrue(container.ports.allSatisfy { $0 > 0 && $0 < 65536 })
            }
        }
    }
}

final class ConflictDetectorTests: XCTestCase {
    func testDetectsSamePortDifferentProcesses() {
        let ports = [
            Port(port: 8080, protocol_: "tcp", address: "127.0.0.1", pid: 100, process: "node"),
            Port(port: 8080, protocol_: "tcp", address: "0.0.0.0", pid: 200, process: "python"),
            Port(port: 9090, protocol_: "tcp", address: "127.0.0.1", pid: 300, process: "nginx"),
        ]
        let conflicts = ConflictDetector.detect(ports)
        XCTAssertEqual(conflicts.count, 1)
        XCTAssertEqual(conflicts[0].port, 8080)
        XCTAssertEqual(conflicts[0].kind, "process-process")
    }

    func testIgnoresSameProcessTwice() {
        let ports = [
            Port(port: 8080, protocol_: "tcp", address: "127.0.0.1", pid: 100, process: "node"),
            Port(port: 8080, protocol_: "tcp", address: "::1", pid: 100, process: "node"),
        ]
        XCTAssertEqual(ConflictDetector.detect(ports).count, 0)
    }
}

final class VersionTests: XCTestCase {
    func testVersionInfoSchema() {
        let info = Version.info()
        XCTAssertEqual(info.tool, "symscope")
        XCTAssertEqual(info.schemaVersion, 1)
        XCTAssertFalse(info.version.isEmpty)
    }
}

final class BoundedProcessRunnerTests: XCTestCase {
    func testSlowProcessIsTerminatedAtDeadline() throws {
        let started = Date()
        let result = try BoundedProcessRunner.run(
            executable: "/bin/sleep",
            arguments: ["2"],
            timeoutSeconds: 0.05
        )
        XCTAssertTrue(result.timedOut)
        XCTAssertLessThan(Date().timeIntervalSince(started), 1)
    }

    func testAsyncRunnerReturnsOutputWithoutChangingTimeoutContract() async throws {
        let result = try await BoundedProcessRunner.runAsync(
            executable: "/usr/bin/printf",
            arguments: ["hello"],
            timeoutSeconds: 1
        )

        XCTAssertFalse(result.timedOut)
        XCTAssertEqual(result.terminationStatus, 0)
        XCTAssertEqual(result.output, "hello")
    }
}

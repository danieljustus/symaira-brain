import Foundation
import FreeformSnapshotCore

// No source/destination arguments and no network or UI dependencies.
guard CommandLine.arguments.count == 1 else {
    FileHandle.standardError.write(Data("This helper accepts no arguments.\n".utf8))
    exit(2)
}
do {
    let status = try FreeformSnapshot.run(source: FreeformSnapshot.source, destination: FreeformSnapshot.directory)
    exit(status.ok ? 0 : 1)
} catch {
    FileHandle.standardError.write(Data("\(error.localizedDescription)\n".utf8))
    exit(1)
}

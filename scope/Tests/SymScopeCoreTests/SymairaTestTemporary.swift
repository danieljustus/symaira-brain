import Foundation

enum SymairaTestTemporary {
    static func directory(_ prefix: String) throws -> URL {
        let environment = ProcessInfo.processInfo.environment
        let base: URL

        #if os(macOS)
        if environment["CI"]?.isEmpty != false {
            guard let rootPath = environment["SYMAIRA_EXTERNAL_RUNTIME_ROOT"], !rootPath.isEmpty else {
                throw NSError(domain: "SymairaTestTemporary", code: 1, userInfo: [
                    NSLocalizedDescriptionKey: "SYMAIRA_EXTERNAL_RUNTIME_ROOT is required for local macOS tests",
                ])
            }
            let nvmeRoot = URL(fileURLWithPath: "/Volumes/1TB_NVMe_SN850X", isDirectory: true)
                .resolvingSymlinksInPath()
                .standardizedFileURL
            let root = URL(fileURLWithPath: rootPath, isDirectory: true)
                .resolvingSymlinksInPath()
                .standardizedFileURL
            var nvmeIsDirectory = ObjCBool(false)
            var rootIsDirectory = ObjCBool(false)
            guard FileManager.default.fileExists(atPath: nvmeRoot.path, isDirectory: &nvmeIsDirectory),
                  nvmeIsDirectory.boolValue,
                  FileManager.default.fileExists(atPath: root.path, isDirectory: &rootIsDirectory),
                  rootIsDirectory.boolValue,
                  root.path == nvmeRoot.path || root.path.hasPrefix(nvmeRoot.path + "/") else {
                throw NSError(domain: "SymairaTestTemporary", code: 2, userInfo: [
                    NSLocalizedDescriptionKey: "SYMAIRA_EXTERNAL_RUNTIME_ROOT must be below /Volumes/1TB_NVMe_SN850X",
                ])
            }
            base = root
        } else {
            base = FileManager.default.temporaryDirectory
        }
        #else
        base = FileManager.default.temporaryDirectory
        #endif

        let directory = base
            .appendingPathComponent("swift-tests", isDirectory: true)
            .appendingPathComponent("\(prefix)-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }
}

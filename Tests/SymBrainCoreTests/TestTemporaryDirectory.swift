import Foundation

func symBrainTestTemporaryDirectory() -> URL {
#if os(macOS)
    let environment = ProcessInfo.processInfo.environment
    if environment["CI"]?.isEmpty != false {
        guard let rawRoot = environment["SYMAIRA_EXTERNAL_RUNTIME_ROOT"], !rawRoot.isEmpty else {
            fatalError("SYMAIRA_EXTERNAL_RUNTIME_ROOT is required for local macOS tests")
        }

        let fileManager = FileManager.default
        let nvmeRoot = URL(fileURLWithPath: "/Volumes/1TB_NVMe_SN850X", isDirectory: true)
            .resolvingSymlinksInPath()
            .standardizedFileURL
        let runtimeRoot = URL(fileURLWithPath: rawRoot, isDirectory: true)
            .resolvingSymlinksInPath()
            .standardizedFileURL
        var nvmeIsDirectory = ObjCBool(false)
        var runtimeIsDirectory = ObjCBool(false)
        guard fileManager.fileExists(atPath: nvmeRoot.path, isDirectory: &nvmeIsDirectory),
              nvmeIsDirectory.boolValue,
              fileManager.fileExists(atPath: runtimeRoot.path, isDirectory: &runtimeIsDirectory),
              runtimeIsDirectory.boolValue,
              runtimeRoot.path == nvmeRoot.path || runtimeRoot.path.hasPrefix(nvmeRoot.path + "/") else {
            fatalError("SYMAIRA_EXTERNAL_RUNTIME_ROOT must be under /Volumes/1TB_NVMe_SN850X")
        }
        return runtimeRoot
    }
#endif
    return FileManager.default.temporaryDirectory
}

import Foundation

func operateSmokeTestTemporaryDirectory() -> URL {
    let baseURL: URL

#if os(macOS)
    if ProcessInfo.processInfo.environment["CI"]?.isEmpty != false {
        guard let rawRoot = ProcessInfo.processInfo.environment["SYMAIRA_EXTERNAL_RUNTIME_ROOT"], !rawRoot.isEmpty else {
            fatalError("SYMAIRA_EXTERNAL_RUNTIME_ROOT is required for local macOS Operate smoke tests")
        }

        let nvmeURL = URL(fileURLWithPath: "/Volumes/1TB_NVMe_SN850X", isDirectory: true)
            .resolvingSymlinksInPath()
            .standardizedFileURL
        let rootURL = URL(fileURLWithPath: rawRoot, isDirectory: true)
            .resolvingSymlinksInPath()
            .standardizedFileURL
        var nvmeIsDirectory = ObjCBool(false)
        var rootIsDirectory = ObjCBool(false)
        guard rootURL.path == nvmeURL.path || rootURL.path.hasPrefix(nvmeURL.path + "/") else {
            fatalError("SYMAIRA_EXTERNAL_RUNTIME_ROOT must be on /Volumes/1TB_NVMe_SN850X")
        }
        guard FileManager.default.fileExists(atPath: nvmeURL.path, isDirectory: &nvmeIsDirectory),
              nvmeIsDirectory.boolValue,
              FileManager.default.fileExists(atPath: rootURL.path, isDirectory: &rootIsDirectory),
              rootIsDirectory.boolValue else {
            fatalError("external NVMe is not mounted at /Volumes/1TB_NVMe_SN850X")
        }
        baseURL = rootURL.appendingPathComponent("operate-smoke-tests", isDirectory: true)
    } else {
        baseURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("symoperate-smoke-tests", isDirectory: true)
    }
#else
    baseURL = FileManager.default.temporaryDirectory
        .appendingPathComponent("symoperate-smoke-tests", isDirectory: true)
#endif

    do {
        try FileManager.default.createDirectory(at: baseURL, withIntermediateDirectories: true)
    } catch {
        fatalError("could not create Operate smoke-test root: \(error)")
    }
    return baseURL
}

// symscope — legacy entrypoint, now a thin wrapper over SymScopeCLI.
// The unified entrypoint is `symcockpit scope <command>`.
import Foundation
import SymScopeCLI

@main
struct SymScopeMain {
    static func main() async {
        let args = Array(CommandLine.arguments.dropFirst())
        let code = await ScopeMain.run(args)
        exit(code)
    }
}

// symoperate — legacy entrypoint, now a thin wrapper over SymOperateCLI.
// The unified entrypoint is `symcockpit operate <command>`.
import Foundation
import SymOperateCLI

let args = Array(CommandLine.arguments.dropFirst())
let code = OperateMain.run(args)
exit(code)

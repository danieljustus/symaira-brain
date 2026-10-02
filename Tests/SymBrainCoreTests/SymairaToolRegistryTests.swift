import Testing
import SymairaToolKit

struct SymairaToolRegistryTests {
    @Test func activeToolsExcludeAbsorbedBinariesAndIncludeSymroom() {
        let absorbedToolIDs: Set<String> = [
            "symmemory",
            "symseek",
            "symfetch",
            "symscope",
            "symprint",
            "symskills",
            "symguard",
        ]
        let activeTools = SymairaToolRegistry.active
        let activeIDs = Set(activeTools.map(\.id))

        #expect(activeIDs.contains("symroom"))
        #expect(absorbedToolIDs.isDisjoint(with: activeIDs))
        #expect(activeTools.allSatisfy { $0.deprecated == nil })
    }
}

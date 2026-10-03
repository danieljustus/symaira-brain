//! Frozen Go flag-package default text for each memory command.

use std::io::Write;

pub(super) fn write(verb: &str, stderr: &mut dyn Write) {
    let text = match verb {
        "list" => {
            concat!(
                "Usage of memory list:\n",
                "  -db string\n",
                "    \tdatabase path override (default: the configured memory database)\n",
                "  -l int\n",
                "    \tmaximum number of memories to return\n",
                "  -limit int\n",
                "    \tmaximum number of memories to return\n",
                "  -s string\n",
                "    \tfilter by scope: global, project, agent, user, or session\n",
                "  -scope string\n",
                "    \tfilter by scope: global, project, agent, user, or session\n",
            )
        }
        "rules" => {
            concat!(
                "Usage of memory rules:\n",
                "  -db string\n",
                "    \tdatabase path override (default: the configured memory database)\n",
                "  -s string\n",
                "    \tfilter by scope: global, project, agent, user, or session\n",
                "  -scope string\n",
                "    \tfilter by scope: global, project, agent, user, or session\n",
            )
        }
        "query-log" => {
            concat!(
                "Usage of memory query-log:\n",
                "  -actor string\n",
                "    \tfilter recent entries by actor\n",
                "  -db string\n",
                "    \tdatabase path override (default: the configured memory database)\n",
                "  -l int\n",
                "    \tmaximum recent entries to return\n",
                "  -limit int\n",
                "    \tmaximum recent entries to return\n",
            )
        }
        "search" => {
            concat!(
                "Usage of memory search:\n",
                "  -db string\n",
                "    \tdatabase path override (default: the configured memory database)\n",
                "  -l int\n",
                "    \tmaximum number of memories to return\n",
                "  -limit int\n",
                "    \tmaximum number of memories to return\n",
                "  -s string\n",
                "    \tfilter by scope: global, project, agent, user, or session\n",
                "  -scope string\n",
                "    \tfilter by scope: global, project, agent, user, or session\n",
            )
        }
        "set" => {
            concat!(
                "Usage of memory set:\n",
                "  -author string\n",
                "    \tauthor recorded on the memory (default \"cli:symbrain\")\n",
                "  -db string\n",
                "    \tdatabase path override (default: the configured memory database)\n",
                "  -entities string\n",
                "    \toptional comma-separated entity names to link\n",
                "  -k string\n",
                "    \tsemantic kind: user, feedback, project, or reference (required)\n",
                "  -kind string\n",
                "    \tsemantic kind: user, feedback, project, or reference (required)\n",
                "  -metadata string\n",
                "    \toptional JSON object of metadata key/value pairs\n",
                "  -s string\n",
                "    \tscope: global, project, agent, user, or session (default \"global\")\n",
                "  -scope string\n",
                "    \tscope: global, project, agent, user, or session (default \"global\")\n",
                "  -staged\n",
                "    \tstore as a staged candidate, excluded from retrieval until promoted\n",
            )
        }
        "delete" => {
            concat!(
                "Usage of memory delete:\n",
                "  -db string\n",
                "    \tdatabase path override (default: the configured memory database)\n",
            )
        }
        _ => unreachable!("known memory verb"),
    };
    let _ = write!(stderr, "{text}");
}

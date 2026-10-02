import Foundation
import SymScopeCore

enum MCPDiscoveryDiagnosticFixture {
    static let inventoryJSON = """
    {
      "schema_version": 2,
      "project_dir": null,
      "harnesses": [
        {
          "name": "codex",
          "display_name": "Codex",
          "global": {
            "path": "/home/fixture/.codex/config.toml",
            "exists": true,
            "parsed": true,
            "error": null,
            "servers": [
              {
                "name": "tasker",
                "transport": "stdio",
                "command": "bridge",
                "args": ["mcp"],
                "env_names": ["TASKER_API_KEY"]
              }
            ]
          }
        },
        {
          "name": "claude",
          "display_name": "Claude",
          "global": {
            "path": "/home/fixture/.claude/settings.json",
            "exists": true,
            "parsed": true,
            "error": null,
            "servers": []
          }
        },
        {
          "name": "cursor",
          "display_name": "Cursor",
          "global": {
            "path": "/home/fixture/.cursor/mcp.json",
            "exists": false,
            "parsed": false,
            "error": null,
            "servers": []
          }
        },
        {
          "name": "zed",
          "display_name": "Zed",
          "global": {
            "path": "/home/fixture/.config/zed/settings.json",
            "exists": true,
            "parsed": false,
            "error": "invalid JSON near api_key=RAW_CONFIG_CANARY; env=RESOLVED_ENV_CANARY",
            "servers": []
          },
          "project": {
            "path": "/workspace/.zed/settings.json",
            "exists": true,
            "parsed": true,
            "error": null,
            "servers": []
          }
        }
      ]
    }
    """
}

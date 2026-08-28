package mcp

import "testing"

func TestActivityToolsAreDefaultDenyInStandaloneServer(t *testing.T) {
	result := callMCP(helperServer(t), "tools/list", nil)
	tools := result["result"].(map[string]interface{})["tools"].([]interface{})
	for _, raw := range tools {
		name := raw.(map[string]interface{})["name"].(string)
		if name == "activity_search" || name == "activity_get" || name == "activity_status" {
			t.Fatalf("standalone memory server exposed profile-gated tool %q", name)
		}
	}
}

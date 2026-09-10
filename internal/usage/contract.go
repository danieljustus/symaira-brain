package usage

// ContractProvider is the credential-free provider graph exposed by the
// schema-v1 usage report. Keep this list aligned with AllProviders.
type ContractProvider struct {
	ID          string `json:"id"`
	DisplayName string `json:"display_name"`
}

var contractProviders = []ContractProvider{
	{ID: "claude", DisplayName: "Claude"},
	{ID: "codex", DisplayName: "Codex"},
	{ID: "copilot", DisplayName: "GitHub Copilot"},
	{ID: "cursor", DisplayName: "Cursor"},
	{ID: "kimi", DisplayName: "Kimi Code"},
	{ID: "moonshot", DisplayName: "Moonshot"},
	{ID: "nous", DisplayName: "Nous Portal"},
	{ID: "opencode", DisplayName: "OpenCode Go"},
	{ID: "openrouter", DisplayName: "OpenRouter"},
	{ID: "antigravity", DisplayName: "Antigravity"},
}

// ContractProviders returns the complete stable provider graph without
// resolving credentials or contacting a provider.
func ContractProviders() []ContractProvider {
	return append([]ContractProvider(nil), contractProviders...)
}

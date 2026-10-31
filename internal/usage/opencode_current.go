package usage

import "net/http"

// Keep the historical cookie-only constructor available to anchored web oracles.
// Production selects the official API before constructing any web credentials.
type openCodeCurrentProvider struct {
	*OpenCodeProvider
	apiKey, source string
	err            error
}

func newCurrentOpenCodeProvider(client *http.Client) *openCodeCurrentProvider {
	key, source, err := openCodeAPIKey()
	if key != "" || err != nil {
		return &openCodeCurrentProvider{OpenCodeProvider: &OpenCodeProvider{client: client}, apiKey: key, source: source, err: err}
	}
	return &openCodeCurrentProvider{OpenCodeProvider: NewOpenCodeProvider(client)}
}
func (p *openCodeCurrentProvider) IsConfigured() bool {
	return p.apiKey != "" || p.OpenCodeProvider.IsConfigured()
}
func (p *openCodeCurrentProvider) AuthStatus() AuthStatus {
	if p.err != nil {
		return AuthStatus{Status: "missing", Detail: p.err.Error(), Source: p.source}
	}
	if p.apiKey != "" {
		return AuthStatus{Status: "available", Detail: "OpenCode Go API key configured", Source: p.source}
	}
	return p.OpenCodeProvider.AuthStatus()
}
func (p *openCodeCurrentProvider) Strategies() []Strategy {
	if p.err != nil {
		return nil
	}
	if p.apiKey != "" {
		return []Strategy{&openCodeAPIStrategy{key: p.apiKey, client: p.client}}
	}
	return p.OpenCodeProvider.Strategies()
}

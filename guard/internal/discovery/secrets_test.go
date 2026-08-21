package discovery

import "testing"

func TestLooksLikeSecret(t *testing.T) {
	tests := []struct {
		name  string
		key   string
		value string
		want  bool
	}{
		{"api key by key name", "BRAVE_API_KEY", "bsk_test_abc123", true},
		{"token by key name", "ANTHROPIC_AUTH_TOKEN", "x", true},
		{"secret by key name", "MY_SECRET", "anything", true},
		{"password by key name", "DB_PASSWORD", "hunter2", true},
		{"api key by value prefix", "ANTHROPIC_KEY", "sk-ant-abc123", true},
		{"github token by value prefix", "GH_TOKEN", "ghp_abc123", true},
		{"aws key by value prefix", "AWS_ACCESS_KEY", "AKIA1234567890", true},
		{"key name match is case insensitive", "api_key", "v", true},
		{"empty value is not a secret", "API_KEY", "", false},
		{"env reference is not plaintext", "API_KEY", "${API_KEY}", false},
		{"dollar name reference is not plaintext", "API_KEY", "$API_KEY", false},
		{"plain key and value is not a secret", "ENDPOINT", "https://api.example.com", false},
		{"url-like value is not a secret", "HOME", "/Users/test", false},
		{"key with secret substring in longer name", "NOT_A_SECRET_AT_ALL", "public", true},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := LooksLikeSecret(tt.key, tt.value); got != tt.want {
				t.Errorf("LooksLikeSecret(%q, %q) = %v, want %v", tt.key, tt.value, got, tt.want)
			}
		})
	}
}

func TestIsEnvReference(t *testing.T) {
	tests := []struct {
		name  string
		value string
		want  bool
	}{
		{"braced reference", "${API_KEY}", true},
		{"dollar name reference", "$API_KEY", true},
		{"underscore in reference", "$MY_API_KEY", true},
		{"bare dollar", "$", false},
		{"literal value", "sk-ant-abc", false},
		{"path with dollars", "$HOME/.config", false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := isEnvReference(tt.value); got != tt.want {
				t.Errorf("isEnvReference(%q) = %v, want %v", tt.value, got, tt.want)
			}
		})
	}
}

func TestServer_PlaintextSecretKeys(t *testing.T) {
	tests := []struct {
		name string
		s    Server
		want []string
	}{
		{
			name: "flags secret-looking values, skips references",
			s: Server{
				EnvKeys:   []string{"API_KEY", "TOKEN", "ENDPOINT", "SECRET"},
				EnvValues: []string{"sk-ant-abc", "${TOKEN}", "https://api.example.com", "hunter2"},
			},
			want: []string{"API_KEY", "SECRET"},
		},
		{
			name: "no env vars",
			s:    Server{},
			want: nil,
		},
		{
			name: "all values are references",
			s: Server{
				EnvKeys:   []string{"API_KEY", "TOKEN"},
				EnvValues: []string{"${API_KEY}", "${TOKEN}"},
			},
			want: nil,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := tt.s.PlaintextSecretKeys()
			if len(got) != len(tt.want) {
				t.Fatalf("PlaintextSecretKeys() = %v, want %v", got, tt.want)
			}
			for i := range tt.want {
				if got[i] != tt.want[i] {
					t.Errorf("PlaintextSecretKeys()[%d] = %q, want %q", i, got[i], tt.want[i])
				}
			}
		})
	}
}

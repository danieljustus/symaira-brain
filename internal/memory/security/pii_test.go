package security

import (
	"testing"
)

func TestPIIGuardRedaction(t *testing.T) {
	pg := NewPIIGuard()

	tests := []struct {
		input    string
		expected string
	}{
		{
			input:    "My OpenAI project key is sk-proj-12345abcde12345abcde12345abcde12345abcde12345 and email is test@domain.com",
			expected: "My OpenAI project key is [REDACTED_API_KEY] and email is [REDACTED_EMAIL]",
		},
		{
			input:    "Send payment details of card 4111 1111 1111 1111 to help@symaira.com",
			expected: "Send payment details of card [REDACTED_CARD_NUMBER] to [REDACTED_EMAIL]",
		},
		{
			input:    "GitHub auth token is gho_abcdefabcdefabcdefabcdefabcdefabcdef",
			expected: "GitHub auth token is [REDACTED_API_KEY]",
		},
		{
			input:    "AWS key is AKIA1234567890ABCDEF and Slack is xoxb-123456789012-abcdef",
			expected: "AWS key is [REDACTED_API_KEY] and Slack is [REDACTED_API_KEY]",
		},
		{
			input:    "API key sk-abcdefghijklmnopqrstuvwxyz",
			expected: "API key [REDACTED_API_KEY]",
		},
		{
			input:    "-----BEGIN PRIVATE KEY-----\nsecret\n-----END PRIVATE KEY-----",
			expected: "[REDACTED_API_KEY]",
		},
		{
			input:    "AWS key pair: AKIA1234567890ABCDEF:wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
			expected: "AWS key pair: [REDACTED_API_KEY]",
		},
		{
			input:    "Azure connection: DefaultEndpointsProtocol=https;AccountName=mystore;AccountKey=abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefgh0=",
			expected: "Azure connection: DefaultEndpointsProtocol=https;AccountName=mystore;[REDACTED_API_KEY]",
		},
		{
			input:    "UnionPay card: 6222725413964173",
			expected: "UnionPay card: [REDACTED_CARD_NUMBER]",
		},
	}

	for _, tt := range tests {
		result := pg.Redact(tt.input)
		if result != tt.expected {
			t.Errorf("expected redacted text:\n  '%s'\ngot:\n  '%s'", tt.expected, result)
		}
	}
}

func TestRedactMap(t *testing.T) {
	tests := []struct {
		name string
		meta map[string]string
		want map[string]string
	}{
		{
			name: "nil map returns nil",
			meta: nil,
			want: nil,
		},
		{
			name: "empty map returns empty map",
			meta: map[string]string{},
			want: map[string]string{},
		},
		{
			name: "email in value is redacted",
			meta: map[string]string{"note": "email is alice@example.com"},
			want: map[string]string{"note": "email is [REDACTED_EMAIL]"},
		},
		{
			name: "API key in value is redacted",
			meta: map[string]string{"token": "ghp_abcdefghijklmnopqrstuvwxyz0123456789"},
			want: map[string]string{"token": "[REDACTED_API_KEY]"},
		},
		{
			name: "clean values are unchanged",
			meta: map[string]string{"source": "import", "version": "1.0"},
			want: map[string]string{"source": "import", "version": "1.0"},
		},
		{
			name: "mixed clean and PII values",
			meta: map[string]string{
				"source":  "sync",
				"contact": "bob@example.com",
				"key":     "sk-proj-abc123def456ghi789jkl012mno345pqr678",
			},
			want: map[string]string{
				"source":  "sync",
				"contact": "[REDACTED_EMAIL]",
				"key":     "[REDACTED_API_KEY]",
			},
		},
		{
			name: "original map is not mutated",
			meta: map[string]string{"note": "user@host.com"},
			want: map[string]string{"note": "[REDACTED_EMAIL]"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			origCopy := make(map[string]string)
			for k, v := range tt.meta {
				origCopy[k] = v
			}

			got := RedactMap(tt.meta)

			if tt.meta == nil {
				if got != nil {
					t.Errorf("expected nil, got %v", got)
				}
				return
			}

			for k, v := range tt.want {
				if got[k] != v {
					t.Errorf("key %q: expected %q, got %q", k, v, got[k])
				}
			}
			if len(got) != len(tt.want) {
				t.Errorf("expected %d keys, got %d", len(tt.want), len(got))
			}

			for k, v := range origCopy {
				if tt.meta[k] != v {
					t.Errorf("original map mutated at key %q: expected %q, got %q", k, v, tt.meta[k])
				}
			}
		})
	}
}

func TestRedact(t *testing.T) {
	tests := []struct {
		input    string
		expected string
	}{
		{
			input:    "My OpenAI project key is sk-proj-12345abcde12345abcde12345abcde12345abcde12345 and email is test@domain.com",
			expected: "My OpenAI project key is [REDACTED_API_KEY] and email is [REDACTED_EMAIL]",
		},
		{
			input:    "Send payment details of card 4111 1111 1111 1111 to help@symaira.com",
			expected: "Send payment details of card [REDACTED_CARD_NUMBER] to [REDACTED_EMAIL]",
		},
		{
			input:    "GitHub auth token is ghp_abcdefghijklmnopqrstuvwxyz0123456789",
			expected: "GitHub auth token is [REDACTED_API_KEY]",
		},
		{
			input:    "Clean text with no PII should pass through unchanged",
			expected: "Clean text with no PII should pass through unchanged",
		},
		{
			input:    "",
			expected: "",
		},
	}

	for _, tt := range tests {
		result := Redact(tt.input)
		if result != tt.expected {
			t.Errorf("Redact(%q):\n  expected: %q\n  got:      %q", tt.input, tt.expected, result)
		}
	}
}

func TestCreditCardPrefixValidation(t *testing.T) {
	pg := NewPIIGuard()

	tests := []struct {
		name     string
		input    string
		expected string
	}{
		{
			name:     "Valid Visa with correct prefix and Luhn",
			input:    "Card: 4111111111111111",
			expected: "Card: [REDACTED_CARD_NUMBER]",
		},
		{
			name:     "Valid Mastercard 51-55 prefix with Luhn",
			input:    "Card: 5500000000000004",
			expected: "Card: [REDACTED_CARD_NUMBER]",
		},
		{
			name:     "Valid Mastercard 22-27 prefix with Luhn",
			input:    "Card: 2221000000000009",
			expected: "Card: [REDACTED_CARD_NUMBER]",
		},
		{
			name:     "Valid Amex 34 prefix with Luhn",
			input:    "Card: 340000000000009",
			expected: "Card: [REDACTED_CARD_NUMBER]",
		},
		{
			name:     "Valid Amex 37 prefix with Luhn",
			input:    "Card: 371449635398431",
			expected: "Card: [REDACTED_CARD_NUMBER]",
		},
		{
			name:     "Valid Discover 6011 prefix with Luhn",
			input:    "Card: 6011000000000004",
			expected: "Card: [REDACTED_CARD_NUMBER]",
		},
		{
			name:     "Valid Discover 65 prefix with Luhn",
			input:    "Card: 6500000000000002",
			expected: "Card: [REDACTED_CARD_NUMBER]",
		},
		{
			name:     "Wrong prefix but valid Luhn should NOT redact",
			input:    "Card: 9111111111111111",
			expected: "Card: 9111111111111111",
		},
		{
			name:     "Wrong prefix 1xxx but valid Luhn should NOT redact",
			input:    "Card: 1111111111111118",
			expected: "Card: 1111111111111118",
		},
		{
			name:     "Correct Visa prefix but invalid Luhn should NOT redact",
			input:    "Card: 4111111111111112",
			expected: "Card: 4111111111111112",
		},
		{
			name:     "Correct Mastercard prefix but invalid Luhn should NOT redact",
			input:    "Card: 5500000000000001",
			expected: "Card: 5500000000000001",
		},
		{
			name:     "Correct Amex prefix but invalid Luhn should NOT redact",
			input:    "Card: 371449635398432",
			expected: "Card: 371449635398432",
		},
		{
			name:     "Card with spaces and valid prefix+Luhn",
			input:    "Card: 4111 1111 1111 1111",
			expected: "Card: [REDACTED_CARD_NUMBER]",
		},
		{
			name:     "Card with dashes and valid prefix+Luhn",
			input:    "Card: 4111-1111-1111-1111",
			expected: "Card: [REDACTED_CARD_NUMBER]",
		},
		{
			name:     "GCP service account private_key field",
			input:    `"private_key": "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA0Z3VS5JJcds3xfn/ygWyF8PbnGy5AH...-----END RSA PRIVATE KEY-----\n"`,
			expected: "[REDACTED_API_KEY]",
		},
		{
			name:     "GCP service account without key type prefix",
			input:    `"private_key": "-----BEGIN PRIVATE KEY-----\nMIIEvgIBADANBgkqhkiG9w0BAQEFAASC...-----END PRIVATE KEY-----\n"`,
			expected: "[REDACTED_API_KEY]",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := pg.Redact(tt.input)
			if result != tt.expected {
				t.Errorf("expected:\n  '%s'\ngot:\n  '%s'", tt.expected, result)
			}
		})
	}
}

func TestPIIGuardURLCredentials(t *testing.T) {
	pg := NewPIIGuard()

	tests := []struct {
		name     string
		input    string
		expected string
	}{
		{
			name:     "HTTPS URL with credentials",
			input:    "Repo is at https://user:p4ssw0rd@github.com/org/repo.git",
			expected: "Repo is at [REDACTED_URL_CREDENTIALS]",
		},
		{
			name:     "MongoDB connection string with credentials",
			input:    "mongodb://admin:secret123@mongodb.example.com:27017/db",
			expected: "[REDACTED_URL_CREDENTIALS]",
		},
		{
			name:     "FTP URL with credentials",
			input:    "ftp://ftpuser:ftppass@ftp.example.com/file.txt",
			expected: "[REDACTED_URL_CREDENTIALS]",
		},
		{
			name:     "HTTP URL without credentials is preserved",
			input:    "See https://example.com/path?query=1 for details",
			expected: "See https://example.com/path?query=1 for details",
		},
		{
			name:     "Connection string without credentials is preserved",
			input:    "Connect via mongodb://mongodb.example.com:27017/db",
			expected: "Connect via mongodb://mongodb.example.com:27017/db",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := pg.Redact(tt.input)
			if result != tt.expected {
				t.Errorf("expected:\n  '%s'\ngot:\n  '%s'", tt.expected, result)
			}
		})
	}
}

func TestPIIGuardVendorTokens(t *testing.T) {
	pg := NewPIIGuard()

	tests := []struct {
		name     string
		input    string
		expected string
	}{
		{
			name:     "GitLab personal access token",
			input:    "glpat-" + "abc123def456ghi789jkl012mno345pqr678",
			expected: "[REDACTED_API_KEY]",
		},
		{
			name:     "npm access token",
			input:    "npm_" + "abcdefghijklmnopqrstuvwxyz0123456789",
			expected: "[REDACTED_API_KEY]",
		},
		{
			name:     "GitHub app token",
			input:    "ghs_" + "abcdefghijklmnopqrstuvwxyz0123456789",
			expected: "[REDACTED_API_KEY]",
		},
		{
			name:     "GitHub refresh token",
			input:    "ghr_" + "abcdefghijklmnopqrstuvwxyz0123456789",
			expected: "[REDACTED_API_KEY]",
		},
		{
			name:     "Firebase Cloud Messaging server key",
			input:    "AAAA" + "1234567890abcdefghijklmnopqrstuvwxyz1234567890abcdefghijklmnopqrstuvwxyz1234567890abcdefghijklmnopqrstuvwxyz1234567890abcdefghijklmnopqrstuvwxyz1234",
			expected: "[REDACTED_API_KEY]",
		},
		{
			name:     "HTTP Basic Auth header",
			input:    "Authorization: basic " + "dXNlcjpwYXNzd29yZA==",
			expected: "Authorization: [REDACTED_API_KEY]",
		},
		{
			name:     "Docker config auth",
			input:    `"auth": "` + "dXNlcjpwYXNzd29yZA==" + `"`,
			expected: "[REDACTED_API_KEY]",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := pg.Redact(tt.input)
			if result != tt.expected {
				t.Errorf("expected:\n  '%s'\ngot:\n  '%s'", tt.expected, result)
			}
		})
	}
}

func TestPIIGuardEntropyFallback(t *testing.T) {
	pg := NewPIIGuard()

	tests := []struct {
		name     string
		input    string
		expected string
	}{
		{
			name:     "High entropy unknown token in api_key assignment",
			input:    "api_key=Ab1_cd2.ef3_gh4_ij5_kl6_mn7_op8_qr9",
			expected: "api_key=[REDACTED_API_KEY]",
		},
		{
			name:     "High entropy unknown token with colon assignment",
			input:    "token: Xy9_ab87.CD65_ef43.GH21_ij09.KL87_mn65",
			expected: "token: [REDACTED_API_KEY]",
		},
		{
			name:     "Password assignment with high entropy value",
			input:    "password = 9aB3_cD7.eF1_gH5_iJ2_kL8_mN4_oP0_qR6",
			expected: "password = [REDACTED_API_KEY]",
		},
		{
			name:     "Low entropy assignment is preserved",
			input:    "password=correcthorsebatterystaple",
			expected: "password=correcthorsebatterystaple",
		},
		{
			name:     "Short value in secret assignment is preserved",
			input:    "token=short",
			expected: "token=short",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := pg.Redact(tt.input)
			if result != tt.expected {
				t.Errorf("expected:\n  '%s'\ngot:\n  '%s'", tt.expected, result)
			}
		})
	}
}

func TestPIIGuardPreservesNonSecrets(t *testing.T) {
	pg := NewPIIGuard()

	tests := []struct {
		name  string
		input string
	}{
		{name: "Git SHA-1", input: "commit a42dfbb6a2e81089ebe84fb62d058fe19829a06e"},
		{name: "SHA-256", input: "hash e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"},
		{name: "UUID", input: "id 550e8400-e29b-41d4-a716-446655440000"},
		{name: "Ordinary prose", input: "The quick brown fox jumps over the lazy dog."},
		{name: "URL without credentials", input: "Visit https://example.com/settings?tab=security"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			result := pg.Redact(tt.input)
			if result != tt.input {
				t.Errorf("expected input to be unchanged:\n  '%s'\ngot:\n  '%s'", tt.input, result)
			}
		})
	}
}

func FuzzPIIGuardRedaction(f *testing.F) {
	f.Add("sk-proj-" + "12345abcde12345abcde12345abcde12345abcde12345")
	f.Add("email is test@domain.com")
	f.Add("api_key=Ab1_cd2.ef3_gh4_ij5_kl6_mn7_op8_qr9")
	f.Add("The quick brown fox jumps over the lazy dog.")
	f.Add("https://user:pass@example.com/path")

	f.Fuzz(func(t *testing.T, input string) {
		result := Redact(input)

		if second := Redact(result); second != result {
			t.Errorf("redaction not stable:\n  first:  %q\n  second: %q", result, second)
		}
	})
}

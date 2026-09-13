package security

import (
	"go/ast"
	"go/parser"
	"go/token"
	"regexp"
	"strconv"
	"testing"
)

var sensitivePayloadName = regexp.MustCompile(
	`[Vv]ault|[Cc]redential|[Ss]ecret|[Tt]oken|[Pp]assword|[Kk]ey[Aa]ge|identity|recipient`)

// Preserve detection of explicitly labelled literal payloads, including the
// original planted negative test. Ordinary terminology such as "token budget"
// or "secret placeholder" does not label a payload value.
var sensitivePayloadLabel = regexp.MustCompile(
	`(?i)^\s*(vault|credentials?|secret|token|password|keyage|identity|recipient)\s*[:=]\s*%[-+# 0-9.]*[sqvxX]`)

// vaultPayloadLogCalls checks expressions passed to the same fmt functions as
// the original scan. Literal terminology (including config-key and insecure-HTTP
// warnings) is not a payload, unlike an explicitly labelled literal value. Parsing also covers multiline calls and prevents
// comments or unrelated expressions on the same line from triggering the scan.
func vaultPayloadLogCalls(path string, src any) ([]token.Position, error) {
	fset := token.NewFileSet()
	file, err := parser.ParseFile(fset, path, src, 0)
	if err != nil {
		return nil, err
	}
	var violations []token.Position
	ast.Inspect(file, func(n ast.Node) bool {
		call, ok := n.(*ast.CallExpr)
		if !ok {
			return true
		}
		selector, ok := call.Fun.(*ast.SelectorExpr)
		if !ok {
			return true
		}
		pkg, ok := selector.X.(*ast.Ident)
		if !ok || pkg.Name != "fmt" {
			return true
		}
		args := call.Args
		switch selector.Sel.Name {
		case "Errorf", "Sprintf", "Printf", "Println", "Print":
		case "Fprintf":
			// The writer is a destination, not a formatted value.
			if len(args) > 0 {
				args = args[1:]
			}
		default:
			return true
		}
		if hasLabelledLiteralPayload(args) {
			violations = append(violations, fset.Position(call.Pos()))
			return true
		}
		for _, arg := range args {
			if hasSensitivePayload(arg) {
				violations = append(violations, fset.Position(call.Pos()))
				break
			}
		}
		return true
	})
	return violations, nil
}

func hasLabelledLiteralPayload(args []ast.Expr) bool {
	if len(args) < 2 {
		return false
	}
	format, ok := args[0].(*ast.BasicLit)
	if !ok || format.Kind != token.STRING {
		return false
	}
	text, err := strconv.Unquote(format.Value)
	if err != nil || !sensitivePayloadLabel.MatchString(text) {
		return false
	}
	value, ok := args[1].(*ast.BasicLit)
	return ok && value.Kind == token.STRING
}

func hasSensitivePayload(expr ast.Expr) bool {
	found := false
	ast.Inspect(expr, func(n ast.Node) bool {
		if found {
			return false
		}
		switch n := n.(type) {
		case *ast.Ident:
			switch n.Name {
			// These exact names are non-payload metadata in Browse:
			// flows/record.go's placeholder counter, state/keyresolver.go's
			// entry-name constant, and fetch/output's integer token counts.
			// Do not exempt arbitrary *Count/*Name identifiers or their
			// containing expressions: credentials.TokensTotal still leaks.
			case "secretCount", "VaultEntryName", "EstTokens", "TokensReturned", "TokensTotal":
				return false
			// Multiline CLI/policy messages print these public constants:
			// an environment variable name, a server alias, and mode names.
			case "memorySyncTokenEnv", "ServerVault", "VaultModeRequestOnly", "VaultModeFull", "VaultModeOff":
				return false
			}
			found = sensitivePayloadName.MatchString(n.Name)
		case *ast.SelectorExpr:
			// Match the age qualifier, not the suffix of currentImage.
			if pkg, ok := n.X.(*ast.Ident); ok && (pkg.Name == "age" || pkg.Name == "Age") {
				found = true
			}
		}
		return !found
	})
	return found
}

func TestNoVaultPayloadsInLogs_Expressions(t *testing.T) {
	t.Parallel()
	cases := []struct {
		name string
		body string
		want int
	}{
		{name: "token budget", body: `fmt.Errorf("serialize output for token budget: %w", err)`},
		{name: "credential context", body: `fmt.Errorf("inspect peer credentials: %w", controlErr)`},
		{name: "credential risk class", body: `fmt.Errorf("header %q requires the credential risk class", name)`},
		{name: "constant message", body: `fmt.Errorf("peer credentials require a Unix connection")`},
		{name: "vault context", body: `fmt.Errorf("symvault entry %q: %w", entry, err)`},
		{name: "vault entry reference", body: `fmt.Errorf("symvault entry %q: %w", VaultEntryName, err)`},
		{name: "secret placeholder", body: `fmt.Sprintf("secret placeholder %s — resolve via symvault/1Password before running", ref)`},
		{name: "placeholder counter", body: `fmt.Sprintf("op://recording/secret-%d", secretCount)`},
		{name: "token counts", body: `fmt.Fprintf(w, "%d of %d tokens", marker.TokensReturned, marker.TokensTotal)`},
		{name: "multiline token estimate", body: `fmt.Fprintf(&sb, "> **%s** · %d · ~%d tokens",
			meta.Title, meta.StatusCode, meta.EstTokens)`},
		{name: "image bounds", body: `fmt.Errorf("dimension mismatch: %v vs %v", bounds, currentImage.Bounds())`},
		{name: "literal argument", body: `fmt.Printf("%s", "secret placeholder")`},
		{name: "raw literal", body: "fmt.Print(`token budget`)"},
		{name: "comment", body: `fmt.Printf("%s", name) // secret token`},
		{name: "commented call", body: `/* fmt.Printf("%s", secret) */`},
		{name: "adjacent expression", body: `fmt.Print(name); _ = secret`},
		{name: "unknown key warning", body: `fmt.Errorf("unknown key %q in secret configuration", name)`},
		{name: "insecure HTTP warning", body: `fmt.Printf("--allow-insecure-http is set: bearer tokens travel in the clear")`},
		{name: "writer", body: `fmt.Fprintf(vaultWriter, "%s", name)`},
		{name: "non-formatting call", body: `fmt.Sscanf(input, "%s", &token)`},
		{name: "token environment name", body: `fmt.Fprintf(w, "Bearer token may come from %s", memorySyncTokenEnv)`},
		{name: "server alias", body: `fmt.Errorf("supports %q and %q", profile.ServerVault, profile.ServerMemory)`},
		{name: "vault mode names", body: `fmt.Errorf("invalid mode %q: want %s, %s, %s", mode, VaultModeRequestOnly, VaultModeFull, VaultModeOff)`},
		{name: "labelled literal", body: `fmt.Printf("secret: %s", "hunter2")`, want: 1},
		{name: "labelled literal error", body: `fmt.Errorf("password=%q", "test-only")`, want: 1},
		{name: "labelled literal writer", body: `fmt.Fprintf(w, "token: %s", "test-only")`, want: 1},
		{name: "labelled literal raw format", body: "fmt.Printf(`credential: %s`, `test-only`)", want: 1},
		{name: "label terminology", body: `fmt.Printf("secret placeholder: %s", "test reference")`},
		{name: "label in payload literal", body: `fmt.Printf("%s", "secret: %s")`},
		{name: "secret constant", body: `const secret = "test-only"; fmt.Print(secret)`, want: 1},
		{name: "secret", body: `fmt.Printf("%s", secret)`, want: 1},
		{name: "vault", body: `fmt.Sprintf("%v", vaultPayload)`, want: 1},
		{name: "credential", body: `fmt.Errorf("%v", credential)`, want: 1},
		{name: "token", body: `fmt.Print(token)`, want: 1},
		{name: "password", body: `fmt.Println(password)`, want: 1},
		{name: "key age", body: `fmt.Fprintf(w, "%v", keyAge)`, want: 1},
		{name: "identity", body: `fmt.Printf("%v", identity)`, want: 1},
		{name: "recipient", body: `fmt.Printf("%v", recipient)`, want: 1},
		{name: "age qualifier", body: `fmt.Printf("%v", age.Value)`, want: 1},
		{name: "selector", body: `fmt.Printf("%s", response.Token)`, want: 1},
		{name: "nested conversion", body: `fmt.Printf("%s", string(secretBytes))`, want: 1},
		{name: "concatenation", body: `fmt.Print("prefix " + secret)`, want: 1},
		{name: "indexed expression", body: `fmt.Printf("%v", credentials[0])`, want: 1},
		{name: "multiline payload", body: `fmt.Printf(
			"%s",
			secret,
		)`, want: 1},
		{name: "dynamic format", body: `fmt.Printf(secret)`, want: 1},
		{name: "multiple payloads one call", body: `fmt.Printf("%s %s", token, password)`, want: 1},
		{name: "two calls", body: `fmt.Print(secret); fmt.Print(token)`, want: 2},
		{name: "metadata selector base", body: `fmt.Printf("%v", credentials.TokensTotal)`, want: 1},
		{name: "metadata sibling", body: `fmt.Printf("%v", secretCount + password)`, want: 1},
		{name: "metadata nested argument", body: `fmt.Print(secretCount(secret))`, want: 1},
		{name: "no broad count exemption", body: `fmt.Printf("%d", secretPayloadCount)`, want: 1},
		{name: "no broad name exemption", body: `fmt.Printf("%s", secretName)`, want: 1},
		{name: "unknown key cannot hide payload", body: `fmt.Errorf("unknown key %s", secret)`, want: 1},
		{name: "HTTP warning cannot hide payload", body: `fmt.Printf("--allow-insecure-http is set: %s", token)`, want: 1},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			source := "package p\nimport \"fmt\"\nfunc run() {\n" + tc.body + "\n}\n"
			violations, err := vaultPayloadLogCalls("regression.go", source)
			if err != nil {
				t.Fatalf("scan: %v", err)
			}
			if len(violations) != tc.want {
				t.Fatalf("got %d violations, want %d", len(violations), tc.want)
			}
			for _, pos := range violations {
				if pos.Filename != "regression.go" || pos.Line != 4 {
					t.Errorf("unexpected violation position: %s", pos)
				}
			}
		})
	}
}

func TestNoVaultPayloadsInLogs_ParseError(t *testing.T) {
	t.Parallel()
	if _, err := vaultPayloadLogCalls("broken.go", "package p\nfunc broken("); err == nil {
		t.Fatal("unparseable production source must fail the scan")
	}
}

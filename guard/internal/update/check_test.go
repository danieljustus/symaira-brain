package update

import (
	"context"
	"errors"
	"testing"

	"github.com/danieljustus/symaira-corekit/updatecheck"
)

// stubChecker returns a fixed release or error for every Check call.
type stubChecker struct {
	release *updatecheck.Release
	err     error
}

func (s stubChecker) Check(context.Context, string) (*updatecheck.Release, error) {
	return s.release, s.err
}

// checkerFunc adapts a function to the Checker interface.
type checkerFunc func(ctx context.Context, currentVersion string) (*updatecheck.Release, error)

func (f checkerFunc) Check(ctx context.Context, currentVersion string) (*updatecheck.Release, error) {
	return f(ctx, currentVersion)
}

func TestCheck_SkipsDevAndEmpty(t *testing.T) {
	for _, v := range []string{"", "dev"} {
		if got := Check(context.Background(), v); got != nil {
			t.Errorf("Check(%q) = %v, want nil", v, got)
		}
	}
}

func TestCheck(t *testing.T) {
	old := newChecker
	t.Cleanup(func() { newChecker = old })

	tests := []struct {
		name    string
		release *updatecheck.Release
		err     error
		want    *Info
	}{
		{"up to date", nil, nil, nil},
		{"checker error swallowed", nil, errors.New("network down"), nil},
		{"update available", &updatecheck.Release{TagName: "v1.2.0", HTMLURL: "https://example.com/r"}, nil,
			&Info{Current: "v1.1.0", Latest: "v1.2.0", URL: "https://example.com/r"}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			newChecker = func() Checker { return stubChecker{release: tt.release, err: tt.err} }
			got := Check(context.Background(), "v1.1.0")
			if tt.want == nil {
				if got != nil {
					t.Errorf("Check() = %v, want nil", got)
				}
				return
			}
			if got == nil || *got != *tt.want {
				t.Errorf("Check() = %+v, want %+v", got, tt.want)
			}
		})
	}
}

func TestCheck_AppliesRequestTimeout(t *testing.T) {
	// The checker must receive a context with a deadline, never a bare
	// background context, so an unreachable GitHub cannot stall a command.
	old := newChecker
	t.Cleanup(func() { newChecker = old })

	gotDeadline := false
	newChecker = func() Checker {
		return checkerFunc(func(ctx context.Context, _ string) (*updatecheck.Release, error) {
			_, gotDeadline = ctx.Deadline()
			return nil, nil
		})
	}
	Check(context.Background(), "v1.1.0")
	if !gotDeadline {
		t.Error("Check() did not apply a deadline to the request context")
	}
}

func TestFormat(t *testing.T) {
	tests := []struct {
		name string
		info *Info
		want string
	}{
		{"nil", nil, ""},
		{"update available", &Info{Current: "v1.0.0", Latest: "v1.1.0", URL: "https://example.com/r"},
			"symguard: update available: v1.0.0 → v1.1.0\n  https://example.com/r\n"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := Format(tt.info); got != tt.want {
				t.Errorf("Format() = %q, want %q", got, tt.want)
			}
		})
	}
}

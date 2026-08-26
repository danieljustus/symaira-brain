package main

import (
	"reflect"
	"testing"

	"github.com/danieljustus/symaira-brain/internal/output"
)

func TestNormalizeFlags(t *testing.T) {
	tests := []struct {
		name string
		args []string
		want []string
	}{
		{
			name: "nil args",
			args: nil,
			want: []string{},
		},
		{
			name: "empty args",
			args: []string{},
			want: []string{},
		},
		{
			name: "no dashes",
			args: []string{"doctor", "check"},
			want: []string{"doctor", "check"},
		},
		{
			name: "single dash preserved",
			args: []string{"-json", "-v"},
			want: []string{"-json", "-v"},
		},
		{
			name: "double dash converted to single dash",
			args: []string{"--json", "--vault-agent", "claude", "--fix"},
			want: []string{"-json", "-vault-agent", "claude", "-fix"},
		},
		{
			name: "double dash with equals converted to single dash",
			args: []string{"--vault-agent=claude", "--from=personal"},
			want: []string{"-vault-agent=claude", "-from=personal"},
		},
		{
			name: "bare single dash preserved",
			args: []string{"-"},
			want: []string{"-"},
		},
		{
			name: "bare double dash acts as terminator",
			args: []string{"--flag", "--", "--not-a-flag", "--another"},
			want: []string{"-flag", "--", "--not-a-flag", "--another"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := normalizeFlags(tt.args)
			if !reflect.DeepEqual(got, tt.want) {
				t.Errorf("normalizeFlags(%v) = %v, want %v", tt.args, got, tt.want)
			}
		})
	}
}

func TestExtractFormat(t *testing.T) {
	tests := []struct {
		name       string
		args       []string
		wantFormat output.Format
		wantClean  []string
		wantErr    bool
	}{
		{
			name:       "empty args",
			args:       []string{},
			wantFormat: output.FormatTable,
			wantClean:  []string{},
		},
		{
			name:       "double-dash json",
			args:       []string{"doctor", "--json"},
			wantFormat: output.FormatJSON,
			wantClean:  []string{"doctor"},
		},
		{
			name:       "single-dash json",
			args:       []string{"doctor", "-json"},
			wantFormat: output.FormatJSON,
			wantClean:  []string{"doctor"},
		},
		{
			name:       "double-dash output json",
			args:       []string{"doctor", "--output", "json"},
			wantFormat: output.FormatJSON,
			wantClean:  []string{"doctor"},
		},
		{
			name:       "single-dash output json",
			args:       []string{"doctor", "-output", "json"},
			wantFormat: output.FormatJSON,
			wantClean:  []string{"doctor"},
		},
		{
			name:       "double-dash output=json",
			args:       []string{"doctor", "--output=json"},
			wantFormat: output.FormatJSON,
			wantClean:  []string{"doctor"},
		},
		{
			name:       "single-dash output=json",
			args:       []string{"doctor", "-output=json"},
			wantFormat: output.FormatJSON,
			wantClean:  []string{"doctor"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			format, clean, err := extractFormat(tt.args)
			if (err != nil) != tt.wantErr {
				t.Fatalf("extractFormat(%v) error = %v, wantErr %v", tt.args, err, tt.wantErr)
			}
			if format != tt.wantFormat {
				t.Errorf("extractFormat(%v) format = %v, want %v", tt.args, format, tt.wantFormat)
			}
			if !reflect.DeepEqual(clean, tt.wantClean) {
				t.Errorf("extractFormat(%v) clean = %v, want %v", tt.args, clean, tt.wantClean)
			}
		})
	}
}

package workflows

import (
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"

	"gopkg.in/yaml.v3"
)

// #990 changes runner/release support, not retained platform-specific source.
func TestSupportedNativeRunnerPolicy(t *testing.T) {
	_, source, _, _ := runtime.Caller(0)
	directory := filepath.Join(filepath.Dir(source), "..", "workflows")
	files, err := os.ReadDir(directory)
	if err != nil {
		t.Fatal(err)
	}
	for _, file := range files {
		if !strings.HasSuffix(file.Name(), ".yml") && !strings.HasSuffix(file.Name(), ".yaml") {
			continue
		}
		t.Run(file.Name(), func(t *testing.T) {
			var parsed struct {
				Jobs map[string]struct {
					Runner   any `yaml:"runs-on"`
					Strategy any `yaml:"strategy"`
				} `yaml:"jobs"`
			}
			if err := yaml.Unmarshal(repositoryFile(t, ".github/workflows/"+file.Name()), &parsed); err != nil {
				t.Fatal(err)
			}
			for name, job := range parsed.Jobs {
				for _, value := range []any{job.Runner, job.Strategy} {
					text, err := yaml.Marshal(value)
					if err != nil {
						t.Fatal(err)
					}
					for _, forbidden := range []string{"windows", "macos-15-intel", "macos-13", "x86_64-apple-darwin", "darwin-amd64"} {
						if strings.Contains(strings.ToLower(string(text)), forbidden) {
							t.Errorf("job %s selects unsupported target %q", name, forbidden)
						}
					}
				}
			}
		})
	}
}

func TestSupportedCLIReleasePolicy(t *testing.T) {
	var config struct {
		Builds []struct {
			OS     []string `yaml:"goos"`
			Arch   []string `yaml:"goarch"`
			Ignore []struct {
				OS   string `yaml:"goos"`
				Arch string `yaml:"goarch"`
			} `yaml:"ignore"`
		} `yaml:"builds"`
	}
	if err := yaml.Unmarshal(repositoryFile(t, ".goreleaser.yml"), &config); err != nil {
		t.Fatal(err)
	}
	var targets []string
	for _, build := range config.Builds {
		for _, os := range build.OS {
			for _, arch := range build.Arch {
				excluded := false
				for _, ignore := range build.Ignore {
					if ignore.OS == os && ignore.Arch == arch {
						excluded = true
					}
				}
				if !excluded {
					targets = append(targets, os+"/"+arch)
				}
			}
		}
	}
	slices.Sort(targets)
	if want := []string{"darwin/arm64", "linux/amd64", "linux/arm64"}; !slices.Equal(targets, want) {
		t.Fatalf("release targets = %v, want %v", targets, want)
	}
}

package managed

import (
	"archive/tar"
	"archive/zip"
	"bytes"
	"compress/gzip"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type archiveTestEntry struct {
	name     string
	data     []byte
	typeflag byte
	mode     os.FileMode
	linkname string
}

func writeTarFixture(t *testing.T, entries []archiveTestEntry) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "fixture.tar.gz")
	file, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	gz := gzip.NewWriter(file)
	writer := tar.NewWriter(gz)
	for _, entry := range entries {
		typeflag := entry.typeflag
		if typeflag == 0 {
			typeflag = tar.TypeReg
		}
		size := int64(0)
		if typeflag == tar.TypeReg || typeflag == tar.TypeRegA {
			size = int64(len(entry.data))
		}
		header := &tar.Header{
			Name: entry.name, Mode: 0o755, Size: size,
			Typeflag: typeflag, Linkname: entry.linkname,
		}
		if err := writer.WriteHeader(header); err != nil {
			t.Fatal(err)
		}
		if size > 0 {
			if _, err := writer.Write(entry.data); err != nil {
				t.Fatal(err)
			}
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	if err := gz.Close(); err != nil {
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	return path
}

func writeZipFixture(t *testing.T, entries []archiveTestEntry) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "fixture.zip")
	file, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	writer := zip.NewWriter(file)
	for _, entry := range entries {
		header := &zip.FileHeader{Name: entry.name, Method: zip.Store}
		if entry.mode != 0 {
			header.SetMode(entry.mode)
		} else {
			header.SetMode(0o755)
		}
		part, err := writer.CreateHeader(header)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := part.Write(entry.data); err != nil {
			t.Fatal(err)
		}
	}
	if err := writer.Close(); err != nil {
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestSafeArchivePath(t *testing.T) {
	tests := []struct {
		name    string
		want    string
		wantErr bool
	}{
		{"symvault", "symvault", false},
		{"./dir/symvault", "dir/symvault", false},
		{"../symvault", "", true},
		{"dir/../symvault", "", true},
		{`dir\..\symvault`, "", true},
		{"/tmp/symvault", "", true},
		{`C:\tmp\symvault`, "", true},
		{"", "", true},
	}
	for _, test := range tests {
		got, err := safeArchivePath(test.name)
		if (err != nil) != test.wantErr || got != test.want {
			t.Errorf("safeArchivePath(%q) = (%q, %v), want (%q, error=%v)", test.name, got, err, test.want, test.wantErr)
		}
	}
}

func TestExtractBinaryTarGzRejectsUnsafeEntries(t *testing.T) {
	core := &Core{BinaryName: "symbrain", AssetPrefix: "symbrain", Version: "v1"}
	tests := []struct {
		name  string
		entry archiveTestEntry
	}{
		{"traversal", archiveTestEntry{name: "../symbrain", data: []byte("bad")}},
		{"absolute", archiveTestEntry{name: "/tmp/symbrain", data: []byte("bad")}},
		{"symlink", archiveTestEntry{name: "symbrain", typeflag: tar.TypeSymlink, linkname: "/tmp/bad"}},
		{"hardlink", archiveTestEntry{name: "symbrain", typeflag: tar.TypeLink, linkname: "other"}},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			path := writeTarFixture(t, []archiveTestEntry{test.entry})
			if _, err := extractBinaryTarGz(path, core, "linux", "amd64"); err == nil {
				t.Fatal("unsafe tar entry was accepted")
			}
		})
	}
}

func TestExtractBinaryZipRejectsUnsafeEntries(t *testing.T) {
	core := &Core{BinaryName: "symbrain", AssetPrefix: "symbrain", Version: "v1"}
	tests := []archiveTestEntry{
		{name: "../symbrain", data: []byte("bad")},
		{name: `dir\..\symbrain`, data: []byte("bad")},
		{name: "symbrain", data: []byte("target"), mode: os.ModeSymlink | 0o777},
	}
	for _, entry := range tests {
		path := writeZipFixture(t, []archiveTestEntry{entry})
		if _, err := extractBinaryZip(path, core, "windows", "amd64"); err == nil {
			t.Fatalf("unsafe zip entry %q was accepted", entry.name)
		}
	}
}

func TestExtractBinaryPrefersExactAndRejectsAmbiguousFallbacks(t *testing.T) {
	core := &Core{BinaryName: "symbrain", AssetPrefix: "symbrain", Version: "v1"}
	exact := []byte("exact")
	entries := []archiveTestEntry{
		{name: "nested/symbrain", data: []byte("fallback")},
		{name: "symbrain", data: exact},
	}
	tarPath := writeTarFixture(t, entries)
	got, err := extractBinaryTarGz(tarPath, core, "linux", "amd64")
	if err != nil || !bytes.Equal(got, exact) {
		t.Fatalf("tar exact selection = %q, %v", got, err)
	}
	zipPath := writeZipFixture(t, entries)
	got, err = extractBinaryZip(zipPath, core, "windows", "amd64")
	if err != nil || !bytes.Equal(got, exact) {
		t.Fatalf("zip exact selection = %q, %v", got, err)
	}

	ambiguous := []archiveTestEntry{
		{name: "one/symbrain", data: []byte("one")},
		{name: "two/symbrain", data: []byte("two")},
	}
	if _, err := extractBinaryTarGz(writeTarFixture(t, ambiguous), core, "linux", "amd64"); err == nil || !strings.Contains(err.Error(), "ambiguous") {
		t.Fatalf("ambiguous tar error = %v", err)
	}
	if _, err := extractBinaryZip(writeZipFixture(t, ambiguous), core, "windows", "amd64"); err == nil || !strings.Contains(err.Error(), "ambiguous") {
		t.Fatalf("ambiguous zip error = %v", err)
	}
}

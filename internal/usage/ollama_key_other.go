//go:build !unix

package usage

// Fail closed until an equivalent owner/ACL and no-follow boundary is available.
func readOllamaKeyFile(string) ([]byte, error) { return nil, ollamaKeyError() }

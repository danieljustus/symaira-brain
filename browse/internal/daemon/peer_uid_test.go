package daemon

import "testing"

func TestPeerUIDMatchPolicy(t *testing.T) {
	for _, test := range []struct {
		name     string
		peer     uint32
		daemon   uint32
		accepted bool
	}{
		{name: "same uid", peer: 501, daemon: 501, accepted: true},
		{name: "different uid", peer: 502, daemon: 501, accepted: false},
	} {
		t.Run(test.name, func(t *testing.T) {
			if got := peerUIDMatches(test.peer, test.daemon); got != test.accepted {
				t.Fatalf("peerUIDMatches(%d, %d) = %t, want %t", test.peer, test.daemon, got, test.accepted)
			}
		})
	}
}

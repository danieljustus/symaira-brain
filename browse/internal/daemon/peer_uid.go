package daemon

func peerUIDMatches(peerUID, daemonUID uint32) bool {
	return peerUID == daemonUID
}

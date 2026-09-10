//go:build !darwin && !dragonfly && !freebsd && !linux && !netbsd && !openbsd && !solaris && !aix

package usage

import "os/exec"

func configureProbeProcess(_ *exec.Cmd) {}

func killProbeProcess(cmd *exec.Cmd) {
	if cmd.Process != nil {
		_ = cmd.Process.Kill()
	}
}

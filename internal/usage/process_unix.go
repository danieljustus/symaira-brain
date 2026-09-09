//go:build darwin || dragonfly || freebsd || linux || netbsd || openbsd || solaris || aix

package usage

import (
	"os/exec"
	"syscall"
)

func configureProbeProcess(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true}
}

func killProbeProcess(cmd *exec.Cmd) {
	if cmd.Process == nil {
		return
	}
	_ = syscall.Kill(-cmd.Process.Pid, syscall.SIGKILL)
	_ = cmd.Process.Kill()
}

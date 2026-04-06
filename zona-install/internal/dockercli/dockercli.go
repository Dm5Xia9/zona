package dockercli

import (
	"errors"
	"fmt"
	"io"
	"os/exec"
	"runtime"
)

// CheckInstall verifies docker binary, running daemon, and compose (v2 plugin or docker-compose).
func CheckInstall() error {
	if _, err := exec.LookPath("docker"); err != nil {
		return installHint()
	}
	cmd := exec.Command("docker", "info")
	cmd.Stderr = io.Discard
	cmd.Stdout = io.Discard
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("docker is installed but the daemon is not running; start Docker and retry: %w", err)
	}
	c := exec.Command("docker", "compose", "version")
	c.Stderr = io.Discard
	c.Stdout = io.Discard
	if c.Run() == nil {
		return nil
	}
	if _, err := exec.LookPath("docker-compose"); err == nil {
		return nil
	}
	return errors.New("docker compose not found; install Docker Desktop or Docker Compose v2 plugin")
}

func installHint() error {
	switch runtime.GOOS {
	case "linux":
		return errors.New("docker not found; install Docker: https://docs.docker.com/engine/install/ or curl -fsSL https://get.docker.com | sudo sh")
	case "windows":
		return errors.New("docker not found; install Docker Desktop: https://docs.docker.com/desktop/install/windows-install/")
	case "darwin":
		return errors.New("docker not found; install Docker Desktop: https://docs.docker.com/desktop/install/mac-install/")
	default:
		return errors.New("docker not found; install Docker for your OS")
	}
}

// ComposeUpDetachedBuild returns a command with Dir set; caller sets Stdout/Stderr/Env.
func ComposeUpDetachedBuild(workDir string) *exec.Cmd {
	var cmd *exec.Cmd
	check := exec.Command("docker", "compose", "version")
	check.Stderr = io.Discard
	check.Stdout = io.Discard
	if check.Run() == nil {
		cmd = exec.Command("docker", "compose", "up", "--detach", "--build")
	} else {
		cmd = exec.Command("docker-compose", "up", "--detach", "--build")
	}
	cmd.Dir = workDir
	return cmd
}

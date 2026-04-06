// Package app wires config, download, templates, and docker compose.
package app

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"

	"github.com/Dm5Xia9/zona/zona-install/internal/archivex"
	"github.com/Dm5Xia9/zona/zona-install/internal/config"
	"github.com/Dm5Xia9/zona/zona-install/internal/dockercli"
	"github.com/Dm5Xia9/zona/zona-install/internal/githubrel"
	"github.com/Dm5Xia9/zona/zona-install/internal/render"
	"github.com/Dm5Xia9/zona/zona-install/internal/seed"
)

// Run executes the full install flow.
func Run(cfg config.Config) error {
	if err := dockercli.CheckInstall(); err != nil {
		return err
	}

	fmt.Println("zona-install: fetching latest GitHub release for", cfg.GitHubRepo)
	assetURL, err := githubrel.LatestAssetURL(cfg.GitHubRepo, config.AssetName)
	if err != nil {
		return err
	}

	if err := os.MkdirAll(cfg.DeployDir, 0o755); err != nil {
		return err
	}

	tgzPath := filepath.Join(cfg.DeployDir, config.AssetName)
	fmt.Println("zona-install: downloading", config.AssetName)
	n, err := githubrel.Download(assetURL, tgzPath)
	if err != nil {
		return err
	}
	defer os.Remove(tgzPath)
	fmt.Printf("zona-install: saved %d bytes -> %s\n", n, tgzPath)

	fmt.Println("zona-install: extracting", config.AssetName)
	binData, err := archivex.ExtractZonaP2PBinary(tgzPath)
	if err != nil {
		return err
	}
	binPath := filepath.Join(cfg.DeployDir, "zona-p2p")
	if err := os.WriteFile(binPath, binData, 0o755); err != nil {
		return err
	}

	nodeSeed, err := seed.RandomNodeSeedHex()
	if err != nil {
		return err
	}

	dockerData := render.DockerData{
		BaseImage:          "debian:bookworm-slim",
		CopyBinaryName:     "zona-p2p",
		ContainerAdminPort: 7701,
	}
	if err := render.WriteDockerfile(cfg.DeployDir, dockerData); err != nil {
		return err
	}

	composePlatform := ""
	if runtime.GOARCH == "arm64" {
		composePlatform = "linux/amd64"
		fmt.Println("zona-install: using compose platform linux/amd64 (ARM host + Linux/amd64 image binary)")
	}
	composeData := render.ComposeData{
		Image:           "zona-p2p-prebuilt:local",
		ComposePlatform: composePlatform,
		HostPort:        cfg.HostPort,
		NodeSeedHex:     nodeSeed,
	}
	if err := render.WriteCompose(cfg.DeployDir, composeData); err != nil {
		return err
	}

	fmt.Println("zona-install: docker compose up --detach --build")
	cmd := dockercli.ComposeUpDetachedBuild(cfg.DeployDir)
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Env = os.Environ()
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("docker compose: %w", err)
	}

	fmt.Println()
	fmt.Println("zona-install: stack directory:", cfg.DeployDir)
	fmt.Printf("zona-install: admin API: http://localhost:%d/api/info\n", cfg.HostPort)
	fmt.Println("zona-install: logs:    cd", cfg.DeployDir, "&& docker compose logs -f")
	fmt.Println("zona-install: stop:    cd", cfg.DeployDir, "&& docker compose down")
	return nil
}

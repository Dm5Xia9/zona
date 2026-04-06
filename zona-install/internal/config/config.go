// Package config holds resolved installer settings (flags + environment).
package config

import (
	"os"
	"path/filepath"

	"github.com/Dm5Xia9/zona/zona-install/internal/gitremote"
)

const (
	DefaultGitHubRepo = "Dm5Xia9/zona"
	DefaultHostPort   = 17701
	AssetName         = "zona-p2p-linux-x64.tar.gz"
	DefaultDeployDir  = "zona-node-stack"
)

// Config is fully resolved after Load.
type Config struct {
	DeployDir  string
	GitHubRepo string
	HostPort   int
}

// Load builds Config from explicit flag values (empty string / zero means “use env / default”).
func Load(deployFlag, repoFlag string, portFlag int) (Config, error) {
	deploy := deployFlag
	if deploy == "" {
		deploy = os.Getenv("ZONA_DEPLOY_DIR")
	}
	if deploy == "" {
		cwd, err := os.Getwd()
		if err != nil {
			return Config{}, err
		}
		deploy = filepath.Join(cwd, DefaultDeployDir)
	}

	repo := repoFlag
	if repo == "" {
		repo = os.Getenv("ZONA_GITHUB_REPO")
	}
	if repo == "" {
		repo = gitremote.GitHubRepoFromOrigin()
	}
	if repo == "" {
		repo = DefaultGitHubRepo
	}

	port := portFlag
	if port <= 0 {
		port = DefaultHostPort
	}

	return Config{
		DeployDir:  filepath.Clean(deploy),
		GitHubRepo: repo,
		HostPort:   port,
	}, nil
}

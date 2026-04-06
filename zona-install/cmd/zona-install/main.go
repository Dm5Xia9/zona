// Command zona-install downloads the latest zona-p2p Linux release, renders Docker templates,
// and runs "docker compose up --detach --build".
package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/Dm5Xia9/zona/zona-install/internal/app"
	"github.com/Dm5Xia9/zona/zona-install/internal/config"
)

func main() {
	deployFlag := flag.String("dir", "", "deploy directory (default: ./zona-node-stack or $ZONA_DEPLOY_DIR)")
	repoFlag := flag.String("repo", "", `GitHub "owner/name" for releases (default: $ZONA_GITHUB_REPO, git origin, or Dm5Xia9/zona)`)
	portFlag := flag.Int("port", config.DefaultHostPort, "host port mapped to container admin HTTP 7701")
	flag.Parse()

	cfg, err := config.Load(*deployFlag, *repoFlag, *portFlag)
	if err != nil {
		fmt.Fprintf(os.Stderr, "zona-install: %v\n", err)
		os.Exit(1)
	}

	if err := app.Run(cfg); err != nil {
		fmt.Fprintf(os.Stderr, "zona-install: %v\n", err)
		os.Exit(1)
	}
}

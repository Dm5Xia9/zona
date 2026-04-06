// Package render expands embedded Dockerfile / docker-compose templates.
package render

import (
	"bytes"
	"embed"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"text/template"
)

//go:embed templates/*.tmpl
var templateFS embed.FS

// DockerData is passed to Dockerfile.tmpl.
type DockerData struct {
	BaseImage          string // e.g. debian:bookworm-slim
	CopyBinaryName     string // filename next to Dockerfile (e.g. zona-p2p)
	ContainerAdminPort int    // EXPOSE (7701)
}

// ComposeData is passed to docker-compose.yml.tmpl.
type ComposeData struct {
	Image            string // docker image name for the built service
	ComposePlatform  string // optional, e.g. linux/amd64; empty omits platform key
	HostPort         int    // host port mapped to 7701
	NodeSeedHex      string
}

const (
	dockerfileTmpl = "templates/Dockerfile.tmpl"
	composeTmpl    = "templates/docker-compose.yml.tmpl"
)

var (
	tmplOnce sync.Once
	tmplRoot *template.Template
	tmplErr  error
)

func rootTemplate() (*template.Template, error) {
	tmplOnce.Do(func() {
		tmplRoot, tmplErr = template.ParseFS(templateFS, dockerfileTmpl, composeTmpl)
	})
	return tmplRoot, tmplErr
}

// WriteDockerfile renders Dockerfile.tmpl into dir/Dockerfile (0644).
func WriteDockerfile(dir string, data DockerData) error {
	tmpl, err := rootTemplate()
	if err != nil {
		return err
	}
	var buf bytes.Buffer
	if err := tmpl.ExecuteTemplate(&buf, dockerfileTmpl, data); err != nil {
		return fmt.Errorf("Dockerfile template: %w", err)
	}
	path := filepath.Join(dir, "Dockerfile")
	return os.WriteFile(path, buf.Bytes(), 0o644)
}

// WriteCompose renders docker-compose.yml.tmpl into dir/docker-compose.yml (0644).
func WriteCompose(dir string, data ComposeData) error {
	tmpl, err := rootTemplate()
	if err != nil {
		return err
	}
	var buf bytes.Buffer
	if err := tmpl.ExecuteTemplate(&buf, composeTmpl, data); err != nil {
		return fmt.Errorf("docker-compose template: %w", err)
	}
	path := filepath.Join(dir, "docker-compose.yml")
	return os.WriteFile(path, buf.Bytes(), 0o644)
}

package archivex

import (
	"archive/tar"
	"compress/gzip"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// ExtractZonaP2PBinary reads a .tar.gz release archive and returns the zona-p2p ELF bytes.
func ExtractZonaP2PBinary(tgzPath string) ([]byte, error) {
	f, err := os.Open(tgzPath)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	gz, err := gzip.NewReader(f)
	if err != nil {
		return nil, err
	}
	defer gz.Close()
	tr := tar.NewReader(gz)
	var fallback []byte
	for {
		h, err := tr.Next()
		if err == io.EOF {
			break
		}
		if err != nil {
			return nil, err
		}
		if h.Typeflag != tar.TypeReg && h.Typeflag != tar.TypeRegA {
			continue
		}
		name := strings.TrimPrefix(filepath.ToSlash(h.Name), "./")
		if filepath.Base(name) != "zona-p2p" {
			continue
		}
		data, err := io.ReadAll(tr)
		if err != nil {
			return nil, err
		}
		if name == "linux-x64/zona-p2p" || strings.HasSuffix(name, "/linux-x64/zona-p2p") {
			return data, nil
		}
		if fallback == nil {
			fallback = data
		}
	}
	if len(fallback) > 0 {
		return fallback, nil
	}
	return nil, errors.New("archive does not contain linux-x64/zona-p2p or zona-p2p")
}

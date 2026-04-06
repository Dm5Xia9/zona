package gitremote

import (
	"os/exec"
	"strings"
)

// GitHubRepoFromOrigin parses owner/repo from `git remote get-url origin` when it points at github.com.
func GitHubRepoFromOrigin() string {
	out, err := exec.Command("git", "config", "--get", "remote.origin.url").Output()
	if err != nil {
		return ""
	}
	url := strings.TrimSpace(string(out))
	if i := strings.Index(url, "github.com"); i >= 0 {
		rest := url[i+len("github.com"):]
		rest = strings.TrimPrefix(rest, ":")
		rest = strings.TrimPrefix(rest, "/")
		rest = strings.TrimSuffix(rest, ".git")
		parts := strings.Split(rest, "/")
		if len(parts) >= 2 {
			return parts[0] + "/" + parts[1]
		}
	}
	return ""
}

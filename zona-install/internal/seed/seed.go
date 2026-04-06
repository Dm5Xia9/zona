package seed

import (
	"crypto/rand"
	"encoding/hex"
)

// RandomNodeSeedHex returns 64 hex characters (32 random bytes) for ZONA_NODE_SEED.
func RandomNodeSeedHex() (string, error) {
	b := make([]byte, 32)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return hex.EncodeToString(b), nil
}

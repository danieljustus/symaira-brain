// Package broker implements the MCP client side of symbrain: spawning child
// servers, initializing, listing and calling tools, and managing the child
// lifecycle (health checks, crash detection, restart with backoff).
package broker

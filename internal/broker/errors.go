package broker

import "fmt"

// RPCError is a JSON-RPC 2.0 error object returned by a child in response to
// a request (e.g. "unknown tool", "invalid params"). It is distinct from a
// tool-level failure, which is reported via CallToolResult.IsError instead
// (see protocol.go).
type RPCError struct {
	Code    int
	Message string
}

func (e *RPCError) Error() string {
	return fmt.Sprintf("mcp child returned JSON-RPC error %d: %s", e.Code, e.Message)
}

// TimeoutError reports that a request's context deadline elapsed before the
// child responded. Op names the request method (e.g. "initialize",
// "tools/call") for diagnostics.
type TimeoutError struct {
	Op string
}

func (e *TimeoutError) Error() string {
	return fmt.Sprintf("mcp broker: %s timed out waiting for child response", e.Op)
}

// ClosedError reports that the child process exited (or its stdio pipes
// closed) before responding, or that the client was closed while a request
// was in flight.
type ClosedError struct {
	Op  string
	Err error
}

func (e *ClosedError) Error() string {
	if e.Err != nil {
		return fmt.Sprintf("mcp broker: %s failed: child connection closed: %v", e.Op, e.Err)
	}
	return fmt.Sprintf("mcp broker: %s failed: child connection closed", e.Op)
}

func (e *ClosedError) Unwrap() error { return e.Err }

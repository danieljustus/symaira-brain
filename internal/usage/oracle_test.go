package usage

import (
	"net/http"
	"sync"
	"testing"
)

func TestOracleTransportRecordsConcurrentRequests(t *testing.T) {
	transport := &oracleTransport{bodies: map[string][]byte{"openrouter": []byte(`{}`)}}
	client := roundTripFixture{transport}
	request, err := http.NewRequest(http.MethodGet, "https://openrouter.ai/fixture", nil)
	if err != nil {
		t.Fatal(err)
	}
	const count = 32
	start := make(chan struct{})
	var workers sync.WaitGroup
	for range count {
		workers.Add(1)
		go func() {
			defer workers.Done()
			<-start
			response, err := client.RoundTrip(request)
			if err != nil {
				t.Error(err)
				return
			}
			_ = response.Body.Close()
		}()
	}
	close(start)
	workers.Wait()
	if len(transport.requests) != count {
		t.Fatalf("recorded %d requests, want %d", len(transport.requests), count)
	}
}

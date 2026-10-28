package usage

import (
	"context"
	"encoding/json"
	"reflect"
	"testing"
)

// Hand-authored offline contract examples, not captured live responses or frozen oracles.
func TestClaudeCurrentFixtures(t *testing.T) {
	var cases []struct {
		Name   string
		Error  bool
		Body   json.RawMessage
		Meters []UsageMeter
	}
	if err := json.Unmarshal(loadFixture("claude-current-cases.json"), &cases); err != nil {
		t.Fatal(err)
	}
	for _, tc := range cases {
		t.Run(tc.Name, func(t *testing.T) {
			client, _ := fakeClient(200, tc.Body, nil)
			snapshot, err := (&claudeOAuthStrategy{accessToken: "fixture", client: client}).Fetch(context.Background())
			if tc.Error {
				if err == nil {
					t.Fatal("expected invalid numeric payload to fail")
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(snapshot.Meters, tc.Meters) {
				t.Fatalf("meters = %+v; expected %+v", snapshot.Meters, tc.Meters)
			}
			if snapshot.Currency == nil || *snapshot.Currency != "USD" {
				t.Fatal("missing currency")
			}
		})
	}
}

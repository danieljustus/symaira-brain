package sequence

import (
	"testing"

	"github.com/danieljustus/symaira-guard/internal/model"
)

func completedEvent(tool string, args any) model.ActionEvent {
	return seqEvent(tool, args, model.ActionCompleted)
}

func failedEvent(tool, args, errMsg string) model.ActionEvent {
	ev := seqEvent(tool, args, model.ActionFailed)
	ev.Call.Error = errMsg
	return ev
}

func TestLedger_Record(t *testing.T) {
	tests := []struct {
		name        string
		events      []model.ActionEvent
		errOn       int // index of the event to attach a Call.Error to (-1: none)
		wantSuccess int
		wantFailed  int
		wantLastErr string
	}{
		{
			name: "completed without error is a success",
			events: []model.ActionEvent{
				completedEvent("read_file", "a"),
				completedEvent("read_file", "a"),
			},
			wantSuccess: 2,
		},
		{
			name:        "failed state records failure and last error",
			events:      []model.ActionEvent{failedEvent("search_web", "q", "timeout")},
			wantFailed:  1,
			wantLastErr: "timeout",
		},
		{
			name: "completed with error counts as failure",
			events: []model.ActionEvent{
				completedEvent("read_file", "a"),
				completedEvent("read_file", "b"),
			},
			errOn:       1,
			wantSuccess: 1,
			wantFailed:  1,
			wantLastErr: "boom",
		},
		{
			name: "non-terminal states are ignored",
			events: []model.ActionEvent{
				seqEvent("read_file", "a", model.ActionRequested),
				seqEvent("read_file", "a", model.ActionApproved),
			},
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if tt.errOn >= 0 {
				ev := tt.events[tt.errOn]
				ev.Call.Error = tt.wantLastErr
				tt.events[tt.errOn] = ev
			}
			l := NewLedger()
			for _, ev := range tt.events {
				l.Record(ev)
			}
			sum := l.Summarize()
			if tt.wantSuccess == 0 && tt.wantFailed == 0 {
				if len(sum) != 0 {
					t.Fatalf("Summarize len = %d, want 0 (%+v)", len(sum), sum)
				}
				return
			}
			if len(sum) != 1 {
				t.Fatalf("Summarize len = %d, want 1", len(sum))
			}
			e := sum[0]
			if e.Success != tt.wantSuccess {
				t.Errorf("Success = %d, want %d", e.Success, tt.wantSuccess)
			}
			if e.Failed != tt.wantFailed {
				t.Errorf("Failed = %d, want %d", e.Failed, tt.wantFailed)
			}
			if e.LastError != tt.wantLastErr {
				t.Errorf("LastError = %q, want %q", e.LastError, tt.wantLastErr)
			}
		})
	}
}

func TestLedger_SummarizeSortedByTool(t *testing.T) {
	l := NewLedger()
	l.Record(completedEvent("read_file", "a"))
	l.Record(failedEvent("search_web", "q", "timeout"))
	l.Record(completedEvent("read_file", "b"))
	l.Record(completedEvent("write_file", "w"))

	sum := l.Summarize()
	if len(sum) != 3 {
		t.Fatalf("Summarize len = %d, want 3", len(sum))
	}
	if sum[0].Tool != "read_file" || sum[0].Success != 2 {
		t.Errorf("sum[0] = %+v, want read_file with 2 successes", sum[0])
	}
	if sum[1].Tool != "search_web" || sum[1].Failed != 1 || sum[1].LastError != "timeout" {
		t.Errorf("sum[1] = %+v, want search_web with 1 failure", sum[1])
	}
	if sum[2].Tool != "write_file" || sum[2].Success != 1 {
		t.Errorf("sum[2] = %+v, want write_file with 1 success", sum[2])
	}
}

func TestEvaluate_RecordsOutcomes(t *testing.T) {
	d := NewDetector(Config{Enabled: true})
	d.Evaluate(completedEvent("read_file", "a"))
	d.Evaluate(failedEvent("read_file", "b", "boom"))

	sum := d.Ledger().Summarize()
	if len(sum) != 1 {
		t.Fatalf("Summarize len = %d, want 1", len(sum))
	}
	if sum[0].Tool != "read_file" || sum[0].Success != 1 || sum[0].Failed != 1 || sum[0].LastError != "boom" {
		t.Errorf("ledger = %+v, want read_file success=1 failed=1 last_error=boom", sum[0])
	}
}

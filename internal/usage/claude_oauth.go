package usage

import "time"

type claudeOAuthUsage struct {
	FiveHour   *claudeUsageWindow  `json:"five_hour"`
	SevenDay   *claudeUsageWindow  `json:"seven_day"`
	Sonnet     *claudeUsageWindow  `json:"seven_day_sonnet"`
	Opus       *claudeUsageWindow  `json:"seven_day_opus"`
	Fable      *claudeUsageWindow  `json:"seven_day_fable"`
	Limits     []claudeScopedLimit `json:"limits"`
	ExtraUsage *claudeExtraUsage   `json:"extra_usage"`
}

type claudeUsageWindow struct {
	Utilization *float64   `json:"utilization"`
	Utilized    *float64   `json:"utilized"`
	Limit       *float64   `json:"limit"`
	ResetsAt    *time.Time `json:"resets_at"`
}

type claudeScopedLimit struct {
	Kind     string     `json:"kind"`
	Percent  *float64   `json:"percent"`
	ResetsAt *time.Time `json:"resets_at"`
	Scope    struct {
		Model struct {
			DisplayName string `json:"display_name"`
		} `json:"model"`
	} `json:"scope"`
}

type claudeExtraUsage struct {
	Used         *float64 `json:"used"`
	Limit        *float64 `json:"limit"`
	Enabled      *bool    `json:"is_enabled"`
	UsedCredits  *float64 `json:"used_credits"`
	MonthlyLimit *float64 `json:"monthly_limit"`
}

func claudeOAuthMeters(p claudeOAuthUsage) []UsageMeter {
	var meters []UsageMeter
	appendWindow := func(label string, w *claudeUsageWindow) {
		if w == nil {
			return
		}
		used, limit := w.Utilized, w.Limit
		if w.Utilization != nil {
			if *w.Utilization < 0 {
				return
			}
			used, limit = w.Utilization, floatPtr(100)
		} else if limit == nil || *limit <= 0 {
			return
		}
		var usedText *string
		if used != nil && *used >= 0 {
			usedText = strPtr(formatAmount(*used))
		}
		meters = append(meters, UsageMeter{Label: label, Used: usedText, Limit: strPtr(formatAmount(*limit)), Unit: "%", ResetsAt: w.ResetsAt})
	}
	appendWindow("five_hour", p.FiveHour)
	appendWindow("seven_day", p.SevenDay)
	scoped := map[string]bool{}
	for _, l := range p.Limits {
		name := l.Scope.Model.DisplayName
		if l.Kind != "weekly_scoped" || (name != "Fable" && name != "Sonnet" && name != "Opus") || scoped[name] || l.Percent == nil || *l.Percent < 0 {
			continue
		}
		meters = append(meters, UsageMeter{Label: name, Used: strPtr(formatAmount(*l.Percent)), Limit: strPtr("100"), Unit: "%", ResetsAt: l.ResetsAt})
		scoped[name] = true
	}
	for _, model := range []struct {
		name   string
		window *claudeUsageWindow
	}{{"Sonnet", p.Sonnet}, {"Opus", p.Opus}, {"Fable", p.Fable}} {
		if !scoped[model.name] {
			appendWindow(model.name, model.window)
		}
	}
	if extra := p.ExtraUsage; extra != nil {
		used, limit := extra.Used, extra.Limit
		if extra.Enabled != nil || extra.UsedCredits != nil || extra.MonthlyLimit != nil {
			if extra.Enabled == nil || !*extra.Enabled {
				return meters
			}
			used, limit = nil, nil
			if extra.UsedCredits != nil && *extra.UsedCredits >= 0 {
				used = floatPtr(*extra.UsedCredits / 100)
			}
			if extra.MonthlyLimit != nil && *extra.MonthlyLimit > 0 {
				limit = floatPtr(*extra.MonthlyLimit / 100)
			}
		}
		var usedText, limitText *string
		if used != nil && *used >= 0 {
			usedText = strPtr(formatAmount(*used))
		}
		if limit != nil && *limit >= 0 {
			limitText = strPtr(formatAmount(*limit))
		}
		if usedText != nil || limitText != nil {
			meters = append(meters, UsageMeter{Label: "Extra usage", Used: usedText, Limit: limitText, Unit: "USD"})
		}
	}
	return meters
}

func floatPtr(v float64) *float64 { return &v }

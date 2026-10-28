package usage

import "strings"

type codexRate struct {
	Input      uint64  `json:"input"`
	Output     uint64  `json:"output"`
	Read       uint64  `json:"read"`
	InputLong  *uint64 `json:"input_long"`
	OutputLong *uint64 `json:"output_long"`
	ReadLong   *uint64 `json:"read_long"`
	Fast2      uint64  `json:"fast2"`
	Ultra2     uint64  `json:"ultra2"`
}
type codexPriceTable struct {
	Snapshot string               `json:"snapshot"`
	Rates    map[string]codexRate `json:"rates"`
	Aliases  map[string]string    `json:"aliases"`
}

func codexEstimate(model string, b codexCounts, tier string, prices codexPriceTable) *uint64 {
	if alias, ok := prices.Aliases[model]; ok {
		model = alias
	}
	fast := strings.HasSuffix(model, "-fast")
	if fast {
		model = strings.TrimSuffix(model, "-fast")
	}
	r, ok := prices.Rates[model]
	if !ok {
		return nil
	}
	rates := []uint64{r.Input, r.Output, r.Read}
	if b[0] > 272000 {
		for i, n := range []*uint64{r.InputLong, r.OutputLong, r.ReadLong} {
			if n != nil {
				rates[i] = *n
			}
		}
	}
	multiplier := uint64(2)
	switch tier {
	case "", "standard", "default":
		if fast {
			multiplier = r.Fast2
		}
	case "fast", "priority":
		multiplier = r.Fast2
	case "ultrafast":
		multiplier = r.Ultra2
	default:
		return nil
	}
	// Every bundled rate times its half-unit multiplier is integral in 10^-10 USD.
	for i := range rates {
		rates[i] = rates[i] * multiplier / 2
	}
	n := (b[0]-b[1])*rates[0] + b[2]*rates[1] + b[1]*rates[2]
	if n > 1_000_000_000_000_000 {
		return nil
	}
	return &n
}

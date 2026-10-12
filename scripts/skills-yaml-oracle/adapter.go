package main

import (
	"encoding/json"
	"fmt"
	"math"
	"strconv"
	"strings"
)

// Adapt JSON numbers to the runtime kinds sent by the Rust public model.
// json.Unmarshal's default float64 would silently change integer inputs.
func adapt(value any) (any, error) {
	switch value := value.(type) {
	case json.Number:
		text := value.String()
		// serde_json's parse_number explicitly treats the lexical -0 as f64.
		if text != "-0" && !strings.ContainsAny(text, ".eE") {
			if number, err := value.Int64(); err == nil {
				return number, nil
			}
			if number, err := strconv.ParseUint(text, 10, 64); err == nil {
				return number, nil
			}
		}
		return value.Float64()
	case map[string]any:
		for key, item := range value {
			converted, err := adapt(item)
			if err != nil {
				return nil, err
			}
			value[key] = converted
		}
	case []any:
		for i, item := range value {
			converted, err := adapt(item)
			if err != nil {
				return nil, err
			}
			value[i] = converted
		}
	}
	return value, nil
}

func numberTypes(value any, path string, output map[string]string) {
	switch value := value.(type) {
	case float64:
		output[path] = fmt.Sprintf("f64:%016x", math.Float64bits(value))
	case int, int64:
		output[path] = "i64"
	case uint64:
		output[path] = "u64"
	case map[string]any:
		for key, item := range value {
			key = strings.ReplaceAll(strings.ReplaceAll(key, "~", "~0"), "/", "~1")
			numberTypes(item, path+"/"+key, output)
		}
	case []any:
		for i, item := range value {
			numberTypes(item, path+"/"+strconv.Itoa(i), output)
		}
	}
}

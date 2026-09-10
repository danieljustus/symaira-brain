package main

type ExpectedServer struct {
	Enabled    bool     `json:"enabled"`
	Mode       string   `json:"mode,omitempty"`
	Command    string   `json:"command,omitempty"`
	Args       []string `json:"args,omitempty"`
	URL        string   `json:"url,omitempty"`
	Access     string   `json:"access,omitempty"`
	ToolsAllow []string `json:"tools_allow,omitempty"`
	ToolsDeny  []string `json:"tools_deny,omitempty"`
	ToolsRead  []string `json:"tools_read,omitempty"`
	ToolsWrite []string `json:"tools_write,omitempty"`
}

type ProfileExpectation struct {
	ID          string                    `json:"id"`
	InputName   string                    `json:"input_name"`
	InputTOML   string                    `json:"input_toml"`
	Success     bool                      `json:"success"`
	ErrorSubstr string                    `json:"error_substr,omitempty"`
	Name        string                    `json:"name,omitempty"`
	Description string                    `json:"description,omitempty"`
	Audit       bool                      `json:"audit,omitempty"`
	Servers     map[string]ExpectedServer `json:"servers,omitempty"`
	Warnings    []string                  `json:"warnings,omitempty"`
}

type ForeignToolCase struct {
	Name         string `json:"name"`
	ReadOnlyHint *bool  `json:"read_only_hint,omitempty"`
}

type PolicyTestCase struct {
	ID           string            `json:"id"`
	Server       string            `json:"server"`
	Config       ExpectedServer    `json:"config"`
	LiveTools    []string          `json:"live_tools,omitempty"`
	ForeignTools []ForeignToolCase `json:"foreign_tools,omitempty"`
	PresetEval   bool              `json:"preset_eval,omitempty"`
	IsForeign    bool              `json:"is_foreign,omitempty"`
}

type ToolExposureExpectation struct {
	Class  string `json:"class"`
	Source string `json:"source"`
}

type PolicyExpectation struct {
	ID                string                             `json:"id"`
	InputServer       string                             `json:"input_server"`
	InputConfig       ExpectedServer                     `json:"input_config"`
	InputLiveTools    []string                           `json:"input_live_tools,omitempty"`
	InputForeignTools []ForeignToolCase                  `json:"input_foreign_tools,omitempty"`
	PresetEval        bool                               `json:"preset_eval,omitempty"`
	IsForeign         bool                               `json:"is_foreign,omitempty"`
	Success           bool                               `json:"success"`
	ErrorSubstr       string                             `json:"error_substr,omitempty"`
	Server            string                             `json:"server,omitempty"`
	Enabled           bool                               `json:"enabled,omitempty"`
	Mode              string                             `json:"mode,omitempty"`
	Exposed           []string                           `json:"exposed,omitempty"`
	Hidden            []string                           `json:"hidden,omitempty"`
	Unknown           []string                           `json:"unknown,omitempty"`
	Exposures         map[string]ToolExposureExpectation `json:"exposures,omitempty"`
}

type OracleExpectations struct {
	ProfileCases []ProfileExpectation `json:"profile_cases"`
	PolicyCases  []PolicyExpectation  `json:"policy_cases"`
}

func boolPtr(b bool) *bool { return &b }

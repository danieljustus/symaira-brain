package main
import("context";"encoding/json";"fmt";"io";"net/http";"os";"strings";"github.com/danieljustus/symaira-brain/internal/usage")
type owned struct{Headers []string}
func(t *owned)RoundTrip(r *http.Request)(*http.Response,error){t.Headers=append(t.Headers,r.Header.Get("Authorization"));return &http.Response{StatusCode:401,Body:io.NopCloser(strings.NewReader("{}")),Header:make(http.Header)},nil}
func main(){t:=&owned{Headers:[]string{}};c:=&http.Client{Transport:t};var p usage.Provider;if os.Args[1]=="copilot"{p=usage.NewCopilotProvider(c)}else{p=usage.NewKimiProvider(c)};a:=p.AuthStatus();configured:=p.IsConfigured();_=usage.BuildReport(context.Background(),[]usage.Provider{p});b,_:=json.Marshal(map[string]any{"configured":configured,"status":a.Status,"source":a.Source,"headers":t.Headers});fmt.Println(string(b))}

package state
import("bytes";"encoding/json";"fmt";"os";"path/filepath";"testing")
type portProbeProviderKey struct{}
func (portProbeProviderKey) Key()([]byte,KeySource,error){return bytes.Repeat([]byte{0xab},32),KeySourceEnv,nil}
func (portProbeProviderKey) Source()(KeySource,error){return KeySourceEnv,nil}
func TestPortProbeFreshSnapshotDowngrade(t *testing.T){root:=t.TempDir(); encrypted,err:=NewStore(StoreOptions{Dir:root,Keys:portProbeProviderKey{}});if err!=nil{t.Fatal(err)};if err=encrypted.Save(&State{Name:"existing"});err!=nil{t.Fatal(err)};before,err:=os.ReadFile(filepath.Join(root,"existing.json"));if err!=nil{t.Fatal(err)};plain,err:=NewStore(StoreOptions{Dir:root});if err!=nil{t.Fatal(err)};saved:=plain.Save(&State{Name:"existing"});after,err:=os.ReadFile(filepath.Join(root,"existing.json"));if err!=nil{t.Fatal(err)};loaded,loadErr:=plain.Load("existing");record:=map[string]any{"fresh_snapshot_save_success":saved==nil,"retained_bytes_unchanged":bytes.Equal(before,after),"no_key_load_success":loadErr==nil,"loaded_key_source":loaded.KeySource};raw,err:=json.Marshal(record);if err!=nil{t.Fatal(err)};fmt.Println("DOWNGRADE_OBSERVATIONS="+string(raw))}

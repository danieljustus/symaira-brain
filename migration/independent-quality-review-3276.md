# Unabhängige Qualitäts- und Sicherheitsprüfung: integrierter Kandidat 3276

**Geprüfter Stand:** `3276b54282ac6a5c696796dbe7b894e1790fa967`  
**Basis:** `2f6880815a4aa65c4a2d7810240ef0cfd1effe4d`  
**Arbeitsbaum vor dem Review:** sauber; keine Produktdatei verändert.  
**Prüfumfang:** Skills-YAML-Encoder und Loader, Startzustand/Privatsphäre, Evidence-Validierung sowie Windows-Render-Lock. Read-only-Review; kein Build, keine Quellkorrektur, kein nativer Windows-Lauf.

## Urteil

**NO-GO für Evidence-/Native-Akzeptanz:** Zwei konkrete Schwächen in den Startup-Evidence-Prüfungen lassen widersprüchliche beziehungsweise gefälschte ACL-/Dateihash-Aussagen durch die echte Vergleichsentrypoint laufen. Der YAML-Encoder und seine Loader-Normalisierung zeigen im geprüften Quell- und Fixture-Umfang keinen neu eingeführten Fehler. Der Windows-Render-Lock ist im Quellcode lifecycle-kohärent; Windows-Runtime- und ACL-Akzeptanz bleibt jedoch ausdrücklich unbewiesen.

Die beiden Validator-Funde betreffen die Verlässlichkeit des Nachweises, nicht den direkten Produktlauf. Sie blockieren eine belastbare Freigabe der damit begründeten Startup-Privatsphäre.

## Befunde

### R1 — Windows-ACL-Felder sind nicht an den aufgezeichneten Security Descriptor gebunden

**Ort:** `scripts/skills-native-oracle/startup_privacy.py:73-99` (`validate_windows`, `private`).

`capture()` speichert rohe PowerShell-Ausgabe in `stdout_hex` und pro Pfad `descriptor_hex`, Owner und ACEs. `validate_windows()` prüft aber allein die separat gespeicherten Felder (`owner_sid`, `user_sid`, DACL-Flags und `aces`). `private()` übergibt diese Felder direkt an den Validator; es dekodiert weder `stdout_hex` noch leitet es Owner/ACE-Aussagen aus `descriptor_hex` ab. Auch Pfadgleichheit der inneren Zeile wird bei der späteren Prüfung nicht erneut verlangt.

**Reproduktion über die Vergleichsentrypoint:** Mit den strukturell erlaubten Owner-/ACE-Feldern, `descriptor_hex="00"` und `stdout_hex="{}"` akzeptierte `compare_pair()` unter dem Windows-Validierungspfad den vollständigen synthetischen MCP-Datensatz. Die beobachteten Rohdaten und die akzeptierten ACL-Felder widersprachen einander.

**Erforderliche Korrektur:** Die Acceptance-Prüfung muss dieselben Rohbytes als Quelle verwenden: `stdout_hex` strikt dekodieren und die `entries` exakt daraus ableiten/abgleichen; `descriptor_hex` syntaktisch strikt prüfen und die bewerteten ACL-Claims an genau diesen Descriptor binden. Fehlende, widersprüchliche und nicht-hexadezimale Descriptoren müssen am realen Validator-Entrypoint scheitern. Der aktuelle positive/negative ACE-Test ändert einzelne ACE-Felder, deckt diese Cross-Binding-Mutation aber nicht ab.

### R2 — Paarkonsistenz ersetzt keine gültige SHA-256-Prüfung

**Ort:** `scripts/skills-native-oracle/startup_controls.py:79-87` (`_private_file`).

Der Validator verlangt nur `len(raw["sha256"]) == 64` und Gleichheit zu `filesystem.files[path].value`. Ein nicht-hexadezimaler Hash ist damit gültig, wenn beide Kopien gemeinsam geändert werden. `raw["bytes"]` wird ebenfalls nicht als exakter Integer-Typ (ohne `bool`) validiert.

**Reproduktion über die Vergleichsentrypoint:** Im synthetischen Go/Rust-MCP-Datensatz wurden beim Go-Memory-Key sowohl `files[path].value` als auch `raw_file_hashes[path].sha256` auf `"z" * 64` gesetzt. `compare_pair()` gab weiterhin `True` zurück. Der vorhandene Einzelfeld-Mutationstest findet nur die Änderung einer Kopie und lässt die koordinierte Mutation ungetestet.

**Erforderliche Korrektur:** Mindestens lowercase-hex-Syntax (`^[0-9a-f]{64}$`) und strikte Typ-/Größenprüfung für `bytes`; außerdem die Hash-Aussage an tatsächlich erhaltene Rohbytes oder einen unabhängig geprüften, unveränderlichen Receipt-Anker binden. Zwei gleich editierbare Reportfelder belegen keinen Dateiinhalt.

## Geprüfte Bereiche ohne gefundenen neuen Quellfehler

### YAML-Backend, Upstream-Abgrenzung und Loader

- `rust/symbrain-skills/vendor/UPSTREAM.md`, Manifeste, Root-/Crate-Lockfiles und Paketquellen wurden verglichen. Die privaten Crate-Archive stimmen mit den Cargo.lock-Prüfsummen überein: `serde_yaml_ng 0.10.0` `7b4db627…abfd41f`; `unsafe-libyaml 0.2.11` `673aac59…32b861`. Beide MIT-Lizenzdateien sind im Fork vorhanden.
- Der Fork ist aus dem Workspace-Mitgliedersatz ausgeschlossen. Der normale Workspace-Parser bleibt `serde_yaml_ng` aus Registry; der Pfad-Fork wird unter `serde_yaml_ng_skill_encoder` ausschließlich von Skills-Encoding und numerischer Loader-Normalisierung referenziert. Kein globales `[patch.crates-io]` gefunden.
- Die geprüften Änderungen am Fork sind auf Emitter-/Scalar-Policy (DoubleQuoted-Stil, vier Leerzeichen, Go-Layout für Ryu-Ziffern), Unicode-/Natural-Key-Sortierung und Nested-Sequence-Einrückung sowie Rust-1.98-Lint-Anpassungen begrenzt. In `unsafe-libyaml` sind die Pointer-Offset-Änderungen positive `wrapping_add`/`wrapping_sub`-Entsprechungen; Cleanup-Stores behalten die `free`- und Reset-Operationen. Die entfernte Supplementary-Codepoint-Printability-Regel wird nur von den beiden Emitterstellen benutzt und bewirkt die dokumentierte Go-kompatible `\U`-Escapierung. Keine vendorbezogene Lint-Unterdrückung gefunden.
- `encode_skill_md` gibt die Encoderbytes ohne YAML-Postpass aus und hängt die Bodybytes unverändert an. Der Loader verwendet weiterhin den unveränderten Registry-Parser. `metadata_decode::normalize` rekursiert ausschließlich numerische `f64`-Werte; Strings bleiben unangetastet. Die Fixtures unterscheiden echtes negatives Nullbitmuster (`f64:8000000000000000`) von numerischen Strings und prüfen integrale Dezimalfloats, wissenschaftliche Zahlen, verschachtelte Sequenzen/Maps, Unicode-Schlüssel, Controls und Opaque Bodies.
- Die zwei dokumentierten Zielabweichungen — 12 führende-LF-Fälle und zwei wörtliche `<<`-Metadatenschlüssel — sind ausdrücklich begrenzt und als verlustfreie Zielsemantik dokumentiert; keine weiteren Fixture-Ausnahmen wurden behauptet.
- Die 38-zeilige Dokument-Fixture entspricht bytegenau beiden aufbewahrten Go-Ausgaben; beide Captures stimmen miteinander überein. Die 28 ursprünglichen Einzel-Dokumenthashes stimmen mit der erhaltenen historischen Beobachtung überein. Die Sidecar-Angaben für Input und Output passen zu den Dateien. **Einschränkung:** Der Sidecar-Status lautet weiterhin `local capture; independent review and trusted anchor pending`; diese Prüfung hat den Generator nicht erneut ausgeführt und erzeugt keinen unabhängigen kryptographischen Oracle-Anker.

**Separater vorbestehender Loader-Rest:** Die dokumentierte YAML-Domainlücke für unquotierte Null-/Nicht-String-Schlüssel (Go akzeptierte die Diagnoseform, der Rust-JSON-Metadatenadapter lehnte sie ab) bleibt offen. Sie ist im bestehenden YAML-Repair-ADR separat festgehalten und keine Folge des lokalen Encoder-Forks.

### Startup-Persistenz und Privacy-Logik

Der genehmigte Startup-Vertrag in `docs/adr/startup-state-opt-in-834.md` ist im geprüften Rust-Pfad erkennbar umgesetzt: Audit wird nur bei Profil-**und** globaler Freigabe geöffnet; die globale TOML-Konfiguration wird vor `SYMBRAIN_AUDIT_ENABLED` ausgewertet, sodass ein gültiger Override einen Dateifehler nicht verdeckt. Die Memory-Store-Erzeugung ist an das aktivierte Profil gebunden; der ausgeschaltete Pfad öffnet weder DB noch Legacy-JWT-Key. Rust schreibt keine Go-Episoden automatisch. Die dokumentierten Differenzen zum eingefrorenen Go-Orakel (historische unbedingte Memory-Initialisierung, OR-Audit und History-Default) sind genehmigte Zieländerungen, keine hier neu erhobenen Paritätsfehler.

`ordinary_startup.py` bindet die normalen MCP-Kontrollläufe an die bekannten Profile, fehlende globale Konfiguration und erlaubte Umgebungsvariablen; die Vergleichsentrypoint erhält vollständige MCP-Antworten und den kompletten Katalog, nicht nur Skills-Tools. Der oben genannte ACL-Raw-Binding-Fehler gilt unabhängig davon.

### Windows-Render-Lock

`materialize.rs:127-128` hält den Lock über den gesamten Materialisierungsvorgang. `materialize_fs.rs:95-153` verwendet im Windows-Zweig die Root-Capability, `create_new`, No-Follow, Share-Read/Write ohne Delete-Sharing, `FILE_FLAG_DELETE_ON_CLOSE` und eine begrenzte 30-Sekunden-Wartezeit. Die gepinnte `cap-primitives 4.0.3`-Implementierung ergänzt für dieses Flag ausdrücklich `DELETE`-Zugriff; damit ist der naheliegende CreateFile-Zugriffsfehler im Quellcode nicht vorhanden. Der Handle-Eigentümer blockiert Unlink/Replacement, und Kernel-Cleanup entfernt den Pfad beim letzten Handle-Close. Der Windows-spezifische Test deckt vorhandenen fremden Lock, Contention, gehaltenes Unlink-Verbot und saubere Freigabe ab.

Das ist **statische Prüfung**, keine Windows-Ausführung. Die erhaltene lokale Lock-Matrix enthält 12 Go/Rust-Läufe: sechs Ziele in jeder der beiden Operationsreihenfolgen. Sie belegt keinen Windows-Kernel-, ACL- oder DELETE_ON_CLOSE-Lauf.

## Verifikationsgrenzen und offene Gates

- Dieses Review führte 9 Python-Strukturtests aus: 9/9 bestanden. Die zwei oben beschriebenen koordinierten Mutationen wurden danach separat durch die echte Python-Vergleichsentrypoint angenommen; sie sind absichtliche **negative Review-Ergebnisse**, keine Produktakzeptanz.
- Der aufbewahrte `integrated-cli-tests`-Receipt ist sauber und exakt an `3276…` gebunden; Exit 0, zwei Suites mit 24 und 4 bestandenen Tests. Der `integrated-skills-tests`-Receipt stammt aus `002cd…` mit acht als dirty markierten Pfaden, aber sein vollständiger Quellhashsatz (1.993 Einträge) stimmt mit dem geprüften Kandidaten überein; Exit 0. Das ist source-bound, aber nicht als clean-HEAD-Lauf zu bezeichnen.
- Kein Build, kein Rust-Testlauf in diesem Review und kein nativer Windows-Lauf. Windows-ACL-Erhebung, der echte Windows-Render-Lock-Test, vollständige macOS/Linux/Windows-Matrix, ein unabhängiger Trusted Anchor für die Go-YAML-Fixtures sowie der dokumentierte unquotierte Nullschlüssel-Rest bleiben separate Gates.

**Änderungen durch diesen Review:** ausschließlich dieser Bericht; keine Produktquellen, Tests, Fixtures, Konfiguration, GitHub- oder Taskdaten verändert.
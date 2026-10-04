# Native Setup-Orchestrierung für bestehende Source-Worker

Datum: 2026-10-03. Bezug: #765, #783, Epic #731 und PB-2026-09-09.

Der Nutzer hat langfristige Entscheidungen delegiert. Dieses Inkrement ersetzt die
Go-CLI-Orchestrierung für `setup --from-source DIR --modules browse,operate,scope`
durch Rust. Es erhält die eingefrorenen Worker-Verträge: Browse wird weiterhin
mit Go, Operate und Scope weiterhin mit SwiftPM gebaut. Damit ist weder Browse
vollständig nach Rust portiert noch die Source-Installation Go-unabhängig.
#765 und #783 bleiben offen; ungültige vollständige Config-Diagnostik bleibt
bis zu ihrem eigenen geprüften Cutover Go-owned.

## Entscheidungen und Gründe

- Die Modulauswahl bleibt explizit oder folgt den vorhandenen globalen,
  projektbezogenen und ENV-Einstellungen. Reihenfolge und Fehlerfortsetzung
  bleiben Browse, Operate, Scope. Plattformfremde Swift-Worker werden sichtbar
  übersprungen; Scope übernimmt keine Cockpit-Hardwarefunktion.
- Git identifiziert den empfangenden Source-Commit vor jedem Build. Unveränderte
  argv, Arbeitsverzeichnisse, Go `-trimpath`/`CGO_ENABLED=0` und SwiftPM-Pfade
  bleiben erhalten. Jeder Prozess wird direkt gestartet, ohne Shell-Auswertung.
- Ein Source-Auftrag schreibt ausschließlich in das vorhandene Managed-Verzeichnis
  `~/.symaira/bin`. Er ersetzt weder Homebrew/PATH-Binaries noch fremde verwaltete
  Einträge. HOME/USERPROFILE folgen dabei dem tatsächlichen Go-UserHomeDir-
  Vertrag, einschließlich früher Fehler vor Source-/Flag-Konflikten; Windows
  fällt hierfür nicht auf einen anderen HOMEDRIVE/HOMEPATH-Besitzer zurück. Die vorhandene atomische Installation veröffentlicht erst das Binary,
  danach den Provenienz-Sidecar. Scheitert der Sidecar, ist das Binary bereits
  installiert und der Auftrag meldet den Fehler, wie der Go-Vertrag.
- `brain-source` wird aus dem installierten Payload, Commit, Modul und tatsächlichen
  Toolchain-Output gestempelt. Der nachfolgende Versionshandshake liest das Managed-
  Binary. Die historisch leere Sidecar-Version wird nicht nachträglich umgeschrieben;
  Release-Reparatur respektiert die Source-Provenienz bis `--force-release`.
- Tool-Metadaten erhalten ein Budget von 30 Sekunden, Builds von 30 Minuten.
  Das ist eine begründete Begrenzung der bisher unbeschränkten Go-Buildprozesse.
  Unix-Prozessgruppen räumen auch Nachkommen auf; SIGINT/SIGTERM führen zur
  kontrollierten Beendigung. Windows startet vor Job-Zuweisung suspendiert und
  beendet das ganze Job-Objekt beim Rückkehr-/Fehler-/Budgetpfad. Dafür wird die
  Windows-only-Abhängigkeit `process-wrap =10.0.1` mit ausschließlich `std` und
  `job-object` eingesetzt: sichere API ohne eigenes unsafe/Win32-FFI und ohne
  taskkill/PID-Rennen. Das bereits im Lockfile vorhandene Windows-only
  `same-file =1.0.6` erhält die Go-Lstat-Dateiidentität für explizite PATH-/CWD-
  Aliase, einschließlich Hardlinks, mit sicher geöffneten Reparse-Handles. Harte Tötung des CLI-Prozesses wird nicht als bestätigter
  Cleanup-Vertrag behauptet.
- Lokale macOS-Builds ohne CI behalten die verlangte externe Volume-/Cache-
  Eigentümerschaft und Ablehnung von ausbrechenden Pfaden. CI und andere Plattformen
  behalten die historische Umgebung; kein internes macOS-Volume wird still benutzt.
- Guard ist kein Build- oder Installations-Subsystem von Brain. Dieser Pfad benötigt
  keine Guard-Integration und ändert keine Entscheidung/Approval/Risiko-Regeln.

## Übergangsvertrag und späterer Rust-Browse-Cutover

Der Aufruf eines Go-Compilers aus Rust ist Übergangskompatibilität. Ein zukünftiger
Brain-owned Rust-Browse-Sourcevertrag muss einen ausdrücklichen, versionierten und
vertrauenswürdig zugeordneten Cargo-Manifestpfad sowie `cargo build --locked`
festlegen und alle Worker-/Direktclient-Verträge belegen. Dieses Inkrement erkennt
keine beliebigen Cargo-Manifeste opportunistisch. Ein fehlgeschlagener oder
abgelehnter nativer Build darf niemals still auf historische Go-Quellen downgraden.

## Belege und Grenzen

`scripts/setup-source-oracle/run.sh` baut den vollständigen unveränderten Go-CLI
von `dcddcef0` und vergleicht echte CLI-Prozesse. Private native Git-/Go-/Swift-
Fixture-Executables erfassen Tool-argv, cwd und CGO. Ein zusätzlicher Browse-Fall
baut mit dem tatsächlichen Go-SDK einen eigenen dependency-free Worker; dessen
installierte Version und Payload-Hash werden geprüft. Der gemeinsame private
Compiler-Cache liegt außerhalb einzelner Fixture-Snapshots und wird am Ende gelöscht;
Go-Telemetrie ist in diesem realen Build-Fixture durch einen privaten
`mode`-Eintrag und `GOTELEMETRYDIR` ausdrücklich ausgeschaltet.

Verglichen werden stdout/stderr/Exit sowie sämtliche Fixture-Dateien, Typen, Rechte
und Hashes. Nur private Root-Pfade, beobachtete zufällige Stage-/Rename-Pfade und
neue, im tatsächlichen Laufzeitfenster geprüfte UTC-Installationszeiten werden
normalisiert. Bestehende Sidecars bleiben unverändert vergleichbar. Zwei echte
native Prozesskontrollen verfälschen Exit bzw. Source-Identität und müssen mit genau
dem erwarteten Feld scheitern. Ein zusätzlicher echter nativer Builder mit lebendem
Nachkommen belegt den Cleanup bei erfolgreicher Rückkehr.

Die ersten tatsächlichen Abweichungen bleiben unter
`migration/evidence/setup-source-765/` erhalten. Ein explorativer Lauf mit während
der Ausführung bearbeitetem Runner ist dort ausdrücklich kein Acceptance-Beleg.
Abnahme verlangt einen sauberen Source-Commit und dessen Datei-/Binary-Hashes.
CI führt den gleichen Oracle auf Linux, macOS und Windows aus und lädt auch die
gezielt roten Kontrollberichte hoch. Linux-Belege bestätigen keine native macOS-
oder Windows-Ausführung, keine echten Swift-Worker, Hardwareberechtigungen oder
Signierung. Die Fixture-Worker sind Instrumentierung, kein Ersatz für den späteren
vollständigen Browse-/Operate-/Scope-Produkt-Cutover.

## Korrektur nach unabhängiger Prüfung von 2454c461

Der ursprüngliche Source-Commit `2454c461` und sein Evidence-Head `5a57f36e`
bleiben unverändert erhalten. Der vollständige unabhängige Bericht, seine
Hash-Bindung und alle tatsächlichen Fehler-/Kontrollbeobachtungen stehen unter
`migration/evidence/setup-source-765/independent-review-2454/`. Die Prüfung
beanstandete drei P2-Verträge und einen P3-Fehlerpfad; grüne Baseline-Vergleiche
ersetzen diese Befunde nicht. Der korrigierte Stand integriert `main` und die
veröffentlichte Doctor-Windows-Testkorrektur durch normale Merge-Commits.

- Setup übernimmt die präzise Go-FlagSet-Grammatik nach einmaliger vorhandener
  Normalisierung. Zusätzliche Bindestriche oder ein leerer Name führen vor Tool-
  und Installationswirkungen zum Syntaxfehler. Der gültige Dreifach-Präfix vor
  der Normalisierung bleibt als tatsächliche positive Kontrolle erhalten.
- Moduloptionen bleiben Byte-Werte; Go-konforme Unicode-Randzeichen werden ohne
  Verlust ungültiger Unix-Bytes entfernt. Diagnostik verwendet die vorhandene
  bytegenaue Go-Quoting-Funktion. Fehlende Source-Pfade und menschliche Managed-
  Zielausgaben behalten rohe OS-Bytes. Erfolgreiche JSON-Pfadidentitäten und
  Toolchain-Provenienz teilen einen kleinen Encoder, der jedes ungültige Byte
  einzeln als `\ufffd` schreibt und gültiges U+FFFD unterscheidet. Dies schützt
  reale Unix-Verzeichnisidentitäten; es ist keine Änderung des Installationsorts.
- Windows berücksichtigt den impliziten aktuellen Ordner weiterhin, sofern
  `NoDefaultCurrentDirectoryInExePath` fehlt. `GODEBUG=execerrdot=0` wählt diesen
  ausdrücklich vor PATH. Ohne Opt-in bleiben ErrDot und die bestehende Lstat-
  Identitätsprüfung erhalten. Drei echte Windows-CLI-Fixtures prüfen unterschiedliche
  CWD-/PATH-Besitzer, ausschließlich CWD und ausdrücklich deaktiviertes CWD;
  Linux und Signatur-Stubs bestätigen diese Windows-Ausführung nicht.
- Eine fehlende oder als Datei belegte Worker-Temp-Wurzel darf den Git-Metadaten-
  Prozess nicht durch einen neuen Capture-Fehler vorzeitig abbrechen. Ausschließlich
  der kurzlebige Kontroll-Capture nutzt den nächsten existierenden Verzeichnis-
  Vorfahren derselben angeforderten Temp-Wurzel. Das erzeugt keine Verzeichnisse
  und ändert weder Worker-Umgebung, Cache, Volume-Auswahl noch Stage-Wurzel.
  Worker-Staging versucht weiterhin den tatsächlich zufällig erzeugten Pfad
  atomisch mit privaten Rechten und Go-konformer Stat-/Mkdir-Fehlerreihenfolge;
  Fehler bleiben pro Modul im JSON-Bericht. Der eigene RAII-Stage-Besitzer räumt
  vollständig auf. Dieses Verfahren erhält dateibasierte kombinierte Ausgabe
  und vermeidet neue Pipe-Nachkommen-Hänger oder eine stille Build-Ausweichwurzel.

Der erweiterte tatsächliche Prozess-Gate umfasst 90 Fälle auf Linux/macOS und
84 auf Windows sowie dieselben zwei echten negativen Prozesskontrollen.
Native Drei-OS-CI und erneute vollständige unabhängige Prüfung bleiben erforderlich.

Eine zusätzliche eigene tatsächliche Go-/Rust-Prüfung des sauberen korrigierten
Zwischenstands `42490d21` fand drei weitere relative Temp-Fälle. Seine grünen
Baselines und vollständigen tatsächlichen Fehlwirkungen bleiben unter
`migration/evidence/setup-source-765/corrected-42490/` erhalten und gelten wegen
dieser Zusatzbefunde nicht als finale Abnahme. Nur die Kontroll-Capture-Wurzel
wird relativ zum CLI-Arbeitsverzeichnis aufgelöst. Die zufällig und atomisch
angelegte Worker-Stage behält dagegen die angeforderte relative Darstellung in
Compiler-argv und Fehlern; ihr Cleanup-Besitzer hält separat den absoluten Pfad.
Das verhindert, dass `tempfile` den historischen relativen Source-Build unbemerkt
in einen erfolgreichen, zusätzlichen Installationsvorgang umwandelt. Eine echte
missing/file/valid-Relative-Temp-Fallgruppe bleibt im gemeinsamen Prozess-Gate.

Ein zusätzlicher Windows-Ziel-Clippy-Durchlauf mit ausdrücklich typisierten
Signatur-Stubs fand vor Veröffentlichung zwei verschachtelte `if`-Formen im
Lookup. Sie werden semantisch gleichwertig als Let-Chains geschrieben. Ein
weiterer Windows-Pedantic-Durchlauf erhielt außerdem die gleichwertige positive
Absolute-Pfad-Abfrage und eine gezielte Windows-only-Lint-Ausnahme für die
plattformübergreifend fallible Context-API: Unix-Signalregistrierung kann
scheitern, weshalb der gemeinsame Aufrufer weiterhin `Result` benötigt. Der
unveränderte vorherige Source `26c16944`, seine vollständige tatsächliche grüne
Linux-Prozessprüfung und der genaue statische Lint-Fehler bleiben unter
`migration/evidence/setup-source-765/corrected-26c169/` erhalten. Diese engere
Ziel-Lint-Prüfung ist weder vollständige CLI-Kompilierung noch Windows-Ausführung;
die echten nativen Drei-OS-Gates bleiben verbindlich.
Der entsprechende Darwin-Ziel-Lint-Durchlauf ordnet die konstante externe
Volume-Angabe vor den Funktionsstatements ein; auch dies ändert kein Verhalten.
Die engen Lint-Durchläufe unterdrücken ausschließlich Lints der ausdrücklich
markierten Stub-Definitionen und toten Fixture-Einstiegspunkte; die eingebundenen
Produkt-Source-Dateien erhalten All-/Pedantic-Lints. Vollständige native CLI-
Kompilierung und Plattformausführung werden weiterhin ausschließlich durch die
verpflichtenden nativen CI-Gates belegt.

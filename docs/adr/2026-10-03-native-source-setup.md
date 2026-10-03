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

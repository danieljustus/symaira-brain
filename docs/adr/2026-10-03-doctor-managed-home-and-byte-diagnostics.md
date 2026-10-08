# Managed-Home-Vertrag und bytegenaue Herkunftsdiagnosen

Datum: 2026-10-03. Bezug: #765, #806, Epic #731. Status: fokussierte
Folgekorrektur; tatsächliche native Windows-/macOS-Annahme steht aus.

Der Nutzer hat Entscheidungen samt langfristiger Begründung delegiert. Die
vollständige unabhängige Prüfung des Quellstands `0b56d4ed` und Beleg-HEADs
`f1623bfb` hat drei P2-Gruppen bestätigt. Originalbericht, Runner, vollständige
Prozessbeobachtungen und Logs bleiben unverändert in
`migration/evidence/doctor-windows-765/independent-review-0b56/`; `retention.json`
bindet alle 35 Dateien an SHA und Kandidaten. Vor der exklusiven Target-
Wiederverwendung wurden 41 tatsächlich geprüfte CLI-/Test-Executables unter
`/workspace/oracles/symaira-doctor765-0b56-binaries/` verlustfrei gzip-archiviert;
`receipt.json` prüft Original- und Dekompressionshashes. Frühere Kandidaten und
der ursprüngliche Windows-Fehllauf bleiben eigenständige historische Belege.

Source-Tool-Identitäten verwenden den vorhandenen bytewertigen Go-TrimSpace-
Helper. Eine insgesamt ungültige UTF-8-Folge darf nicht dazu führen, dass gültige
Unicode-Leerzeichen an den Rändern erhalten bleiben. NBSP vor und nach einer
Identität mit ungültigen Bytes wird entfernt; die inneren Bytes bleiben bis zum
Go-kompatiblen JSON-Encoder erhalten. Dies vermeidet einen zweiten Decoder und
erhält die Herkunftsidentität ohne nachträgliche verlustbehaftete Reparatur.

Doctor bestimmt seinen Managed-Installationsbesitzer direkt nach dem gepinnten
Go-Vertrag: auf Unix ausschließlich das nichtleere `HOME`, auf Windows
ausschließlich das nichtleere `USERPROFILE`. `HOMEDRIVE`/`HOMEPATH` sind dafür
kein Ersatz. Diese lokale Entscheidung ändert keine anderen XDG-Verbraucher.
Fehlendes Home führt zur exakten Managed-Diagnose, bevor Konfigurationsauswertung,
Fallback, Versionsprobe oder Installation beginnen. Die Eligibility-Prüfung
respektiert dieselbe Reihenfolge. Ein tatsächlicher nativer Integrationstest
prüft fehlenden und leeren Besitzer bei fehlerhafter Konfiguration, abwesendem Go
und unverändertem gesamten Dateizustand. Sein Windows-Zweig setzt ausdrücklich
einen existierenden Ersatzbesitzer; native Windows-CI muss ihn tatsächlich
ausführen. Getrennte `.symaira`-/`bin`-Pfadkomponenten erhalten auf Windows die
SDK-Pfadschreibweise auch im bestehenden Setup-Verbraucher.

Doctor schreibt den Abschluss-Pfad als Betriebssystembytes. Der neue kleine
`read_provenance_bytes`-Einstieg verwendet denselben tatsächlichen Leser und
JSON-Decoder wie die bisherige String-Schnittstelle, erhält aber rohe Pfadbytes
in Diagnosewerten. Bestehende Textaufrufer behalten ihre Schnittstelle. Doctor-
Logs verwenden den vorhandenen bytegenauen Go-Quoter; gültiges U+FFFD und jedes
ungültige Byte bleiben unterscheidbar. Installer-Kontextwrapper erhalten
`GoText`, damit ein Force-Release-Publikationsfehler nicht vor der Log-Ausgabe
verlustbehaftet stringifiziert wird. Herkunftslesefehler schützen weiterhin das
vorhandene Binary; ausschließlich ein gültiges ausdrückliches Force-Release
übersteuert diesen Schutz.

Die neuen Prozessfälle erhalten die vollständigen bisherigen Corpusfälle und
Kontrollen. Sie ergänzen Unicode-Ränder mit ungültiger Tool-Ausgabe, rohe Unix-
Home-Pfade mit korrekten, geschützten, korrupten und Verzeichnis-Herkunftsdateien,
echte Force-Release-Publikation sowie fehlende Besitzer vor Konfiguration. Die
Vergleiche lockern keine Ausgabe-/Dateizustandsbehauptung. Linux-Prozesse und
Windows-Typprüfungen belegen keinen tatsächlichen Windows-SDK-Lauf.

Der zusätzliche Force-Release-Verzeichnisfall hat zunächst ausschließlich den
zufälligen temporären Dateinamen im tatsächlichen Rename-Fehler unterschieden.
Der komplette fehlgeschlagene Vorlauf bleibt erhalten. Zwei weitere tatsächliche
Go-Prozesse bestätigen unterschiedliche `CreateTemp`-Namen für denselben Fehler.
Der Doctor-Vergleich normalisiert deshalb ausschließlich den `.provenance-`-
Basename des linken Rename-Operanden. Ursprungsverzeichnis, rohe Pfadbytes,
Zielpfad, Fehler, Ereignisfolge und Dateizustände bleiben exakt. Der vollständige
Wiederholungsbeleg und der Go-SDK-`CreateTemp`-Vertrag begründen diese Ausnahme.

Diese Korrektur ersetzt weder die verbleibende Go-Konfigurationsdiagnostik noch
historische Browse-/Swift-Source-Worker. Rust-Orchestrierung eines Go-Builds ist
keine Go-unabhängige Installation. #765/#783 bleiben bis zu den jeweiligen
vollständigen Cutovers und den echten drei Betriebssystem-Gates offen.

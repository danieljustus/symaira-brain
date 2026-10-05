# Windows-Herkunftsdiagnosen und native Publisher-Testprozesse

Datum: 2026-10-03. Bezug: #765, #806, Epic #731. Status: Korrektur in
isoliertem Arbeitsbaum; erneuter tatsächlicher Windows-CI-Lauf ausstehend.

Der Nutzer hat technische Entscheidungen samt langfristiger Begründung delegiert.
Der Source-Setup-Kandidat `86ea7bfb` und dessen Beleg-HEAD `0fa47514` bleiben
für die unabhängige Prüfung unverändert. Diese neue Lane beginnt bei `0fa47514`.
Go-Produktionscode und eingefrorene Fixtures bleiben unverändert.

Der tatsächliche Windows-Lauf 37151653597, Job 111286659716, hat 248 von 253
Doctor-Prozessvergleichen bestanden. Die Original-ZIP, vollständige Beobachtungen,
alle fünf fehlschlagenden Datensätze einschließlich Dateizuständen und das
Original-CI-Log sind unter `migration/evidence/doctor-windows-765/` erhalten;
`receipt.json` bindet sie an ihre Hashes und die ursprüngliche Kandidatenidentität.
Der berichtete Kandidaten-HEAD ist `0574ecea`, nicht dieser neue Quellstand.

Die Herkunftsdatei darf ein Verzeichnis sein. Go 1.26.7 öffnet schreibgeschützte
Dateien mit `FILE_FLAG_BACKUP_SEMANTICS` und `FILE_SHARE_READ | FILE_SHARE_WRITE`;
ein Verzeichnis öffnet erfolgreich, sein tatsächlicher Leseversuch meldet
`Incorrect function.`. Rusts bisheriges `std::fs::read` meldete bereits beim
Öffnen `Access is denied.`. Die Korrektur übernimmt die Go-Öffnungssemantik über
die sichere Windows-Standardbibliothek und erhält getrennte tatsächliche
Öffnungs-/Lesefehler. Sie prüft weder Metadaten vorab noch ersetzt sie Fehler
durch erfundene Diagnosewerte. Dadurch bleiben Zugriffsschutz und konkurrierende
Dateiänderungen vom Betriebssystem entschieden. Jeder Lesefehler schützt weiterhin
das vorhandene Binary vor unbeabsichtigter Reparatur.

Vier weitere Abweichungen entstehen im zusätzlichen Test-Publisher: Windows
PATHEXT kann `cosign.EXE` liefern, während die Fixture nur `.exe` entfernte und
dadurch fälschlich ihre Versionsprobe ausführte. Die Fixture erkennt ausschließlich
die ausführbare Erweiterung ohne Beachtung der Schreibweise und erhält den
restlichen Pfad. Separat heißt das tatsächliche Vault-Primärarchiv
`symaira-vault_0.22.1_windows_amd64.zip`; die Fixture ordnete bisher nur den
alternativen Präfix `symvault` zu. Beide bekannten Präfixe gehören zum selben
fest gepinnten Test-Publisher. Identity, Issuer, Signaturdatei, Zertifikatdatei
und genau ein exklusiver Aufrufbeleg pro Core bleiben strikt geprüft. Die Fixture
belegt Prozessargumente und Dateiverwendung, keine Produktionsauthentifizierung.

Der Vergleich verwirft weiterhin keine Fehler und wiederholt keine fehlerhaften
Kontrollen, um sie grün zu machen. Die realen Windows-Fehler bleiben unabhängig von
späteren Erfolgen erhalten. Native Windows-Tests prüfen den tatsächlichen
Verzeichnis-Lesefehler und unveränderte Herkunftsdateien. Linux-Prüfungen oder
Cross-Target-Typprüfungen ersetzen keinen tatsächlichen Windows-SDK-Lauf. Der
integrierte Kandidat benötigt erneut Doctor-, Source-Setup- und Release-Setup-
Prozessvergleiche sowie die vorhandenen drei Betriebssystem-CI-Gates. #765 und
#783 bleiben wegen ausstehender Konfigurationsdiagnosen, Worker-Cutover und
plattformbezogener Annahme offen.

Die vollständige unabhängige Prüfung von `86ea7bfb` hat drei weitere P2-Gruppen
bestätigt. Ihr Bericht, alle originalen Prozessbeobachtungen und Runner sind in
`migration/evidence/setup-source-765/independent-review-86ea/` unverändert erhalten.
Vor dem Neubau wurden zusätzlich 40 tatsächlich ausgeführte ursprüngliche CLI-
und Testdateien unter `/workspace/oracles/source765-86ea-binaries/` samt SHA-Beleg
gesichert. Erst nach expliziter Freigabe des unabhängigen Prüfers wird dessen
bisheriger Target-Cache exklusiv von der neuen Lane wiederverwendet.

Doctor übernimmt dieselbe präzise bytewertige Flag-Grammatik wie Source Setup.
Die gemeinsame einmalige Normalisierung bleibt erhalten: drei führende Striche
können gültig sein, vier sind danach weiterhin fehlerhaft. Parser und Eligibility
verwenden dieselbe Prüfung. Dadurch kann `----force-release` weder eine Probe
auslösen noch einen geschützten Source-Build ersetzen. Fehlerhafte Bool-Werte und
unbekannte Flag-Namen behalten ihre Rohbytes und Go-Quoting-Diagnosen.

Source-Fehler behalten Pfad- und Tool-Ausgabebytes durch Staging, Build, Managed-
Publikation und Versionsprobe bis zur Ausgabe. Der gemeinsame kleine Managed-Typ
`GoText` verwendet den vorhandenen bytegenauen Go-JSON-Stringencoder; menschliche
Ausgabe schreibt die tatsächlichen Bytes. Diese Lösung erhält den Unterschied
zwischen jedem ungültigen UTF-8-Byte und einem gültigen U+FFFD-Zeichen, auch in
aggregierten Fehlern. Ein nachträgliches Ersetzen bereits verlustbehafteter Strings
könnte diese Unterscheidung nicht wiederherstellen. Die allgemeine Display-/Error-
Schnittstelle bleibt für bestehende Textaufrufer verfügbar; Source verwendet sie
nicht als Byte-Ausgabegrenze.

Bei leerem oder fehlendem PATH liefert Go `filepath.SplitList` keine Einträge.
Die native Source-Suche übernimmt dies ausdrücklich und erlaubt weiterhin die
Go-konformen leeren Einträge eines nichtleeren Unix-PATH. Windows merkt sich den
ersten abgelehnten relativen Kandidaten und sucht weiter; nur ein späterer
absoluter Name derselben Datei darf ihn ersetzen. Die Identität folgt dem bereits
verwendeten Lstat-/Handle-Vertrag. Ein anderes späteres Programm bleibt abgelehnt,
explizites `execerrdot=0` behält seinen Go-Vertrag. PATHEXT wird wie im gepinnten
Go-SDK vor der Suche kleingeschrieben. Neue tatsächliche Prozessfälle und native
Windows-Fälle behalten jeweils die vollständigen beobachtbaren Wirkungen.

Der neue Rohbyte-Fall für einen unbekannten Doctor-Flag zeigte zusätzlich, dass
der Test-Runner nichtstrukturierte stderr-Zeilen nur als gültiges UTF-8 speichern
konnte. Der ursprüngliche fehlgeschlagene lokale Lauf bleibt erhalten. Der Runner
speichert nun jede verglichene Ereignis-/Abschlusszeile als vollständige Base64-
Bytes. Dies erhält die exakten Bytes und dieselbe erlaubte Zwischen-Core-Reihenfolge;
kein Ereignis, Fehler oder Dateizustand wird verworfen oder abgeschwächt.

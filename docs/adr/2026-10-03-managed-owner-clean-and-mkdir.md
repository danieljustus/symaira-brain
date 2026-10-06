# Lexikalischer Managed-Besitzer und tatsächlicher MkdirAll-Fehlerpfad

Datum: 2026-10-03. Bezug: #765/#806, Epic #731. Status: lokale Korrektur;
vollständige unabhängige und echte native Plattformannahme steht aus.

Der Nutzer hat Entscheidungen und Begründungen delegiert. Die zweite vollständige
unabhängige Prüfung von Quellstand `a3019695` und Beleg-HEAD `9705777b` bestätigt
zwei P2-Gruppen. Alle acht zuvor fehlschlagenden Eingaben sind geschlossen; sieben
neue tatsächliche Boundary-Proben enthalten sechs Abweichungen und eine positive
Rohbyte-Kontrolle. Der vollständige Bericht, alle Prozess-/Dateizustände, Runner
und Logs bleiben unverändert in
`migration/evidence/doctor-windows-765/independent-review-a301/`. `retention.json`
bindet alle 46 Originaldateien. Die 38 tatsächlich geprüften CLI-/Testdateien
wurden vor Target-Wiederverwendung unter
`/workspace/oracles/symaira-doctor765-a301-binaries/` verlustfrei archiviert und
dekomprimiert SHA-verifiziert. Frühere Kandidaten und Fehlbelege bleiben erhalten.
Main `e3dbda6c` mit dem separat angenommenen Profile-Fix wurde regulär integriert.

Der gepinnte Go-Vertrag bestimmt den Managed-Besitzer mit
`filepath.Join(home, ".symaira", "bin")`, einschließlich lexikalischer Bereinigung.
Bei einem eigenen `link -> owner/nested` führt `HOME=link/../home` daher zu
`home/.symaira/bin`; die bisherige Rust-Auswahl ließ den Kernel den Link auflösen
und prüfte stattdessen `owner/home/.symaira/bin`. Beide Prozesse konnten erfolgreich
enden und trotzdem andere Binaries prüfen/installieren. Dateikanonisierung würde
diesen falschen Besitzer beibehalten und außerdem existierende Verzeichnisse
voraussetzen. Sie ist deshalb für diese Grenze ungeeignet.

Der neue kleine private CLI-Helper ist der einzige Managed-Besitzer-Einstieg für
Doctor und beide Setup-Pfade. Er bereinigt die zusammengefügten Pfadbytes vor
Dateizugriffen. Unix-Rohbytes bleiben erhalten; Windows übernimmt die gepinnten
Drive-, UNC-, Device- und postClean-Regeln aus Go 1.26.7. Die Windows-Brücke folgt
der UTF-16-zu-UTF-8-Umgebungsdarstellung des SDK; Unix verwendet keine String-
Projektion. Nichtleeres HOME beziehungsweise ausschließlich USERPROFILE und die
bisherige Fehlerpriorität bleiben erhalten. Andere Core-XDG-Verbraucher und
Provider-/Store-/Exposure-Regeln werden nicht geändert.

Der kleine gemeinsame Managed-Mkdir-Helper übernimmt Go `MkdirAll`: tatsächlicher
Stat-Fastpfad, rekursive Erzeugung des tatsächlichen Elternpfads, Mkdir mit 0755
unter Unix und Lstat-Nachprüfung nach einem fehlgeschlagenen Mkdir. Ein durch Stat
bestätigtes Nichtverzeichnis liefert den ausdrücklich im Go-SDK gewählten
ENOTDIR-Code (Unix20, Windows ERROR_PATH_NOT_FOUND3), statt Rusts späterem EEXIST.
Der Kontext nennt sowohl das gewünschte Managed-Verzeichnis als auch den
tatsächlich fehlschlagenden Blatt-/Elternpfad als Bytes. Das ist keine nachträgliche
Ersetzung eines verlorenen Fehlerstrings. Reale Betriebssystemfehler bleiben
erhalten; Dateirennen folgen dem SDK-Recheck. Release und Local-Source verwenden
denselben Helper; Download/Prüfung/Extraktion vor und Binary-/Sidecar-Publikation
nach Mkdir behalten ihre Reihenfolge.

Setup ist ebenfalls Verbraucher dieser gemeinsamen Grenze. Sein privater Bericht
und die menschliche Fehlerausgabe erhalten `GoText` bis zur tatsächlichen Ausgabe;
sonst würden die neuen bytewertigen Installer-Fehler sofort wieder verloren gehen.
Der bestehende Go-kompatible JSON-Encoder unterscheidet ungültige Einzelbytes von
gültigem U+FFFD. Keine zweite JSON-Decodierung oder breite Ausgabenormalisierung
wird eingeführt. Der Setup-Prozessrunner bindet Dateizustände an den bekannten
eigenen Projekt-Root, statt seinen Belegumfang aus veränderlichem HOME abzuleiten.
Damit bleiben beide Besitzer im Symlink-Fall vollständig sichtbar.

Alle bisherigen 103 Source-, 274 Doctor-, 151 Setup-Fälle, fünf tatsächlichen
Fehlerkontrollen, 399 Tests, ursprünglichen 28 Zusatzpaare und zwei Signale bleiben
als Mindestregression erhalten. Neue Prozessfälle prüfen die gemeinsamen Grenzen
in Source und Release, JSON und menschlicher Ausgabe, sowie Blatt-/Elternfehler,
rohe Unix-Pfade und die tatsächliche Eigentümerwirkung des relativen Symlinks.
Windows-spezifische pure SDK-Regeln und native Dateitests werden auf nativer CI
ausgeführt; Linux-Prozesse und schmale Cross-Target-Typprüfungen ersetzen keinen
echten Windows-/macOS-Lauf. Go-Produktionscode und eingefrorene Fixtures bleiben
unverändert. Historische Go-/Swift-Worker und volle typisierte Konfigurations-
diagnostik bleiben ausstehende Cutovers; #765/#783 werden hierdurch nicht geschlossen.

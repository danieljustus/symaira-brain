# Entscheidungen zur GitHub-Integration und Migration

Status: vom koordinierenden Maintainer entschieden am 4. Oktober 2026.
Der Nutzer hat ausdrücklich die Entscheidung über langfristig sinnvolle Lösungen
und ihre Dokumentation delegiert. Routinekorrekturen benötigen keine erneute
Bestätigung. Die Entscheidungen ersetzen keine technischen Abnahmen.

## Aktuelle Quellen vor Abschluss prüfen

Eine Quellprüfung, ein vorbereiteter Korpus und ein historischer erfolgreicher
Lauf beweisen unterschiedliche Dinge. Tatsächliche Ergebnisse bleiben an ihren
Commit, Prozess, Plattform und Eingaben gebunden. Neue Kombinationen werden
regulär zusammengeführt und neu geprüft. Fehlende Git-Quellen können nicht durch
Sparse-Materialisierung oder fremde Binärdateien ersetzt werden.

Ein normaler Push nach abgeschlossenem Quellreview ermöglicht die tatsächliche
CI. Vor Merge gelten am aktuellen Stand sämtliche geschützten Checks,
betroffenen nativen Prüfungen und aufgelösten Reviews. Es gibt keinen
administrativen Bypass. Vorbereitete, übersprungene oder nicht ausführbare Fälle
werden niemals als bestanden gezählt. Teilimplementierungen schließen keine
umfassenden Migrations-Issues.

## Ganze Ausgabe-Owner binden

Die gemeinsame Skills-Qualifizierung verwendet explizite Profile aller fünf
vollständigen, unveränderlich gebundenen Owner-Dateien. Unbekannte, gemischte,
fehlende oder zusätzliche Owner bleiben verweigert. Der echte Eltern-Fetch,
Archiv-/Binärbuild, Quellvergleich und die Wiederherstellung bleiben erhalten.
Ein beweglicher Branch oder eine Fragmentprojektion ist kein Elternnachweis.

Der fest gebundene Elterncommit wird ohne `--depth` beschafft. Der gemessene
Depth-Versuch machte den Shared-Clone unvollständig; seine Originale bleiben
erhalten. Die Korrektur verändert keinen Go-Vertrag und kein Vergleichskriterium.

## Prüfer und Prozesslebensdauer besitzen

Eigene dynamische Python-Prüfer werden aus vorab gebundenen Quellbytes direkt
kompiliert und ausgeführt. `-B` verhindert Cache-Schreiben, aber kein Cache-Lesen.
Ein timestamp-gültiger ungebundener Cache darf keinen gehashten Owner ersetzen.
Modulidentität, Dateiname und gewöhnliche Fehler bleiben erhalten; die neue
Ladermethode benötigt unabhängige Prüfung und konkrete Gegenkontrollen.

Prozessbereinigung behält die ursprüngliche PID und Startidentität. Eine spätere
Wiederverwendung derselben Nummer darf keinen fremden Prozess zum Ziel machen.
Bestätigte Kernelhandles beziehungsweise geeignete native Prozessreferenzen
bleiben bis zur Bereinigung erhalten. Nach regulärem Stop wird vor dem Entfernen
eigener Arbeitsverzeichnisse auf das tatsächliche Ende gewartet. Erzwungene
Bereinigung bleibt ein Prüfungsfehler; Verzeichnis-Retries oder unterdrückte
Cleanup-Fehler dürfen einen Lebensdauerfehler nicht verdecken.

## Native Grenzen und SQLite sichtbar halten

Nur eine tatsächlich vom eigenen Darwin-Kernel mit EILSEQ92 verweigerte exakte
Rohnamen-Operation darf als nicht ausführbar ausgewiesen werden. Rohbytes,
Operation, Fehler und Cleanup bleiben sichtbar. Zugelassene Fälle und ihre
Kontrollen bleiben verpflichtend; Linux behält den vollständigen Rohpfadkorpus.

Beobachter-SQLite wird getrennt von Go- und Rust-Produktengines gebunden. Vor
Produktläufen müssen tatsächliche geladene Bibliothek, Funktionsadresse,
Source-ID, REAL-DEFAULT-, NULL- und FTS-Semantik sowie echte Phasenkontrollen
stimmen. Eine Interpreter-Versionsnummer oder ein kopierter Header genügt nicht.
Private Provider bleiben Beobachterabhängigkeiten und werden nicht still in
die Produktion importiert.

## Originale erhalten, Ressourcen begrenzen

Originalfehler, Rohberichte, Konfliktstufen und verfügbare ausführbare Bytes
bleiben vollständig erhalten. Archiv und native Metadaten werden getrennt
gebunden; gerundete Tar-Zeitstempel sind keine exakten Nanosekunden. Cachefund
und Archivierung beweisen Erhaltung, keine tatsächliche Ausführung. Fehlende
historische Binärpayloads werden ausdrücklich benannt und nicht durch andere
Quellstände ersetzt.

Schwere lokale Prüfungen erhalten eine exklusive Zuteilung, höchstens zwei
Compilerjobs und eigene temporäre Verzeichnisse. Vor SDK-/Compilerzuteilung und
nach SDK-Entpacken vor dem ersten Compiler gelten mindestens 2 GiB freier Speicher;
während des Laufs mindestens 700 MiB auf jeder betroffenen Dateisystempartition.
Ein Ressourcenabbruch ist keine Produktparität. Historische Zuteilungen gelten
nicht als aktuelle Autorisierung.

## Sichtbare Skills-Abweichung und verbleibender Release-Abschluss

PR #797 wurde mit allen erforderlichen aktuellen Prüfungen regulär gemergt;
main wurde `c74110425d04dd62c7f2c0f09bb86561bd1ac20e`, #621 ist abgeschlossen.
Die RENDER-Spalte gehört zum normalen `skills status`. Verwaltete saubere Links
bleiben in-sync; tatsächliche Änderungen werden sichtbar. Beobachtung bleibt
schreibfrei. Ausbrechende Cache-Symlinks bleiben verweigert. Import,
Force-Overwrite und automatische Base-Änderungen werden nicht zugesagt.

Das Gesamtziel bleibt offen. Tatsächliche Apple-/Geräteprüfungen, die
Darwin-External-Volume-Regression, signierte Releases und die verlangte
mindestens siebentägige Beobachtungszeit benötigen ihre realen Ergebnisse.
Sie werden weder durch Quelltests noch durch Linux-Projektionen geschlossen.

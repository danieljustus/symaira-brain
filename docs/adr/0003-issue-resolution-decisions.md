# Entscheidungen zum vollständigen Issue- und PR-Abschluss

Status: angenommen für die Ausführung des GitHub-Ziels. Stand: 2026-10-03.
Repository: `danieljustus/symaira-brain`. Auftrag: alle Issues tatsächlich
erledigen und alle akzeptierten PRs sauber in `main` integrieren. Der Nutzer
hat technische Entscheidungen und deren langfristige Begründung ausdrücklich
delegiert. Dieses Protokoll ergänzt ADR 0002; es ändert keine Produktgrenzen.

## E-001: Entscheidungen selbst treffen und unabhängig prüfen

Technische Umsetzungsentscheidungen werden ohne erneute Routine-Rückfragen
getroffen. Unabhängige Reviews und Implementierungen laufen in getrennten
Worktrees; die Integration bleibt zentral und erfolgt jeweils nacheinander.
Der Review nennt unveränderliche Basis und Kandidatenrevision. Ein neuer
Codebefund wird behoben und erneut geprüft.

Begründung: Der Auftrag delegiert diese Entscheidungen. Separate Prüfer
reduzieren die Wahrscheinlichkeit, dass Implementierung und Tests denselben
Denkfehler übernehmen. Getrennte Worktrees schützen gleichzeitig die laufenden
Kandidaten und vermeiden gegenseitige Änderungen. Die zusätzlichen Reviews
von #794 und #797 haben bereits bislang ungetestete Fehler nachgewiesen:
veränderte ungültige Bytes in unbekannten Flag-Namen und fehlende
Render-Diagnosen bei unlesbaren, tatsächlich cache-verlinkten Installationen.
Die Befunde bleiben mit ihren ursprünglichen Revisionen dokumentiert.

## E-002: Nach Abhängigkeiten integrieren und vollständige Annahme nachweisen

#794 wird vor dem darauf aufbauenden #797 integriert. #800 und #801 haben
eigene Annahmegates. Nach jedem Wechsel von `main` werden die übrigen
Kandidaten normal aktualisiert und gegen diese Basis geprüft. Vor dem normalen
Squash-Merge müssen die aktuellen erforderlichen Checks, Review-Auflagen und
offenen Review-Threads erfüllt sein. Kein Admin-Bypass und kein Force-Push.

Begründung: Der Schutz von `main` verlangt eine aktuelle Integrationsbasis.
Ein erfolgreich geprüfter früherer Stand beweist keine spätere Kombination.
Ein Teil-PR darf integriert werden, wenn sein eigener Vertrag erfüllt ist;
das umfangreichere Issue bleibt offen. #798 hat deshalb #765 nicht geschlossen.
Ein Issue wird erst nach verifiziertem Merge und vollständiger Erfüllung seiner
Annahmekriterien als erledigt markiert.

## E-003: Native Ports zuerst; eingefrorene Referenz erhalten

Die fehlenden Doctor-, Memory- und Usage-Pfade werden als fokussierte native
Rust-Portierungen bearbeitet. Go-Produktionscode und eingefrorene Fixtures
bleiben während dieser Portierung unverändert. Zusätzliche Vergleiche führen
die tatsächliche unveränderliche Go-Referenz und den tatsächlichen Rust-Prozess
in eigenen HOME/XDG-Verzeichnissen aus; stdout, stderr und Exitstatus werden
geprüft. Plattformabhängige Verträge benötigen native Plattformläufe.

Begründung: Eine stabile Referenz macht Unterschiede erklärbar und schützt
bestehende Daten-, Sicherheits- und CLI-Verträge. Fallbacks entfallen erst,
wenn der jeweilige native Pfad nachgewiesen ist. Null Tests, übersprungene
Tests, Cross-Compilation und eine ungeprüfte Ausgabeprojektion ersetzen keinen
Annahmenachweis. Produktionsdateien werden in fokussierte Module unter
400 Zeilen aufgeteilt, wenn der bearbeitete Bereich sonst einen Monolithen
weiter vergrößern würde.

## E-004: Fachliche Wiederverwendung und Sicherheitsgrenzen erhalten

Doctor-Kompatibilität für Versionen und Herkunft gehört in den bestehenden
Managed-Bereich; sie wird nicht aus Guard importiert. Memorys Grounded-Evidence
wird lokal portiert, weil EvidenceKit nur diesen Verbraucher hat. Usage nutzt
die vorhandene gemeinsame Secret-Reference-Auflösung und die separate Vault-
Schnittstelle. Neue gemeinsame Kit-Abhängigkeiten benötigen echte gemeinsame
Verbraucher. Brain übernimmt weder Vault-Speicher noch Master Keys.

Begründung: Diese Aufteilung entspricht ADR 0002 und hält Fachlogik am
zuständigen Ort. Sie vermeidet eine Kopplung von Brain an Guards Call-Policy,
ein ungenutztes allgemeines Framework und eine zweite Credential-Implementierung.
Beschädigte Herkunft darf Doctor nicht zur Überschreibung einer absichtlichen
Source-Installation veranlassen. Memory-Reparaturen müssen vorhandene Daten
und Transaktionsgrenzen erhalten. Secret-Werte gehören nicht in Nachweise.

## E-005: Fehler der Referenz sichtbar lassen

Ein Fehler im unveränderten Go-Oracle wird mit Revision, Plattform und
Originalnachweis festgehalten. Er wird weder als Rust-Erfolg ausgegeben noch
durch gelockerte Assertions oder geänderte Referenz versteckt. Eine unveränderte
Wiederholung darf untersuchen, ob der Fehler reproduzierbar ist.

Begründung: Der Windows-ARM-Lauf von #801 scheiterte nach erfolgreichen
Rust-Lifecycle-Prüfungen am Shutdown des tatsächlichen Go-Daemons. Das ist
kein vollständiger Paritätsnachweis. Vergleichsfehler und Infrastrukturfehler
brauchen unterschiedliche Korrekturen; beide bleiben überprüfbar.

## E-006: Funktionen und Go erst nach echtem Cutover entfernen

Eine UI-, TUI- oder TLS-Transport-Ablösung wird anhand der tatsächlichen
erreichbaren Funktionen, Verbraucher und Alternativen entschieden und in einem
eigenen Eintrag begründet. Die allgemeinen Entscheidungsbefugnisse sind kein
Grund, eine bislang unterstützte Funktion ohne Migrationsweg zu streichen.
Browse bleibt aktiv und optional, Operate bleibt erhalten. Go wird erst nach
den vollständigen Port-, Fixture-, signierten Release-, Rollback- und
Beobachtungsgates entfernt.

Begründung: Eine schnelle Schließung des Backlogs darf keinen stillen
Funktions- oder Datenverlust erzeugen. #782 verlangt mindestens sieben
tatsächlich verstrichene Beobachtungstage nach dem Release. #409 benötigt
einen verarbeiteten TestFlight-Build und einen realen iPhone-Test; #790 benötigt
eine tatsächliche Darwin-Prüfung auf einem externen Volume. Bis zum realen
Nachweis bleiben diese Kriterien offen. Codearbeiten laufen unabhängig weiter.

## E-007: Gemeinsam geprüfte Credential-Inkremente zusammen integrieren

Die native Secret-Reference-Auflösung und der korrigierte Hermes-/JWT-Dateipfad
werden gemeinsam im bestehenden PR #804 veröffentlicht. Beide liegen in Usage,
benutzen dieselben begrenzten Read-only-Verträge und haben vollständige getrennte
Reviews sowie einen frisch ausgeführten Eltern-Regressionsgate. Der normale
Merge des veröffentlichten Elternstands erhält alle bisherigen Reviewbelege.

Begründung: Zwei überlappende Credential-PRs würden zusätzliche Zwischenstände
mit eigener Plattformabnahme erzeugen. Ein gemeinsamer unveränderlicher Kandidat
mit beiden Verträgen und frischer nativer Drei-OS-CI macht die tatsächliche
Integration prüfbar. Der ursprüngliche Shrink/Regrowth-Fehler bleibt erhalten;
andere Dateifamilien und das Gesamtissue #768 bleiben getrennte Restarbeit.

## E-008: Activity nach erfüllten eigenen Annahmekriterien integrieren

PR #800 wurde regulär mit erwartetem Head `635d98af` per Squash integriert;
tatsächlicher main-Commit ist `31de72294521701fc4b1a0bce39f03cc34d72e7e`.
Die vollständige Haupt-CI `37143614255`, einschließlich der erforderlichen
Checks und tatsächlicher Linux/macOS/Windows-Migrationsprüfungen, war erfolgreich.
Reviews und Review-Threads enthielten keine offenen Auflagen. Die gelöschte
Activity-Fallback-Route und ACT-CLI-001 bis -003 erfüllen #767; GitHub hat das
Issue mit Grund `completed` geschlossen, anschließend unabhängig abgefragt.

Begründung: Der betroffene Vertrag hat eigene erfüllte Annahmegates. Der
separate Browser-Profil-Job auf macOS war noch queued; Browse-Quellen und sein
Manifest änderten sich in diesem PR nicht. Dieser unabhängige Job ist kein
Activity-Annahmekriterium. Es gab keinen Admin-Bypass, keine umbenannten Checks
und keine gelockerten Assertions. #761 bleibt für seine Store-/Import-Verträge
offen. Die übrigen Kandidaten werden normal auf den tatsächlichen neuen main
integriert und benötigen aktuelle Prüfungen ihrer jeweiligen Kombination.

## Pflege

Neue materielle Entscheidungen erhalten eine eigene Kennung mit betroffenen
Issues, gewähltem Verhalten, Begründung, verifiziertem Umfang und verbleibenden
Gates. Kandidatenspezifische Messungen und ursprüngliche Fehlversuche bleiben
unter `migration/evidence/`; sie werden nicht nachträglich zu Erfolgen erklärt.
Historische Handoffs bleiben erhalten. Dieses Dokument ist ein veröffentlichtes
ADR, nicht ein Abschlussnachweis für den gesamten Backlog.

### E009 — Integrate native Memory adapters with the reviewed store

Extend PR803 with the independently reviewed Memory CLI on main31de722.
The store/evidence and adapter implement one database contract and share
acceptance dependencies. Preserve all original concurrency/path/JSON failures,
and require both native Memory workflows on the final integrated head.
Fresh independent590 Memory/265 Activity observations and119 tests, plus
separate59 Memory/Gateway tests and83 Go evidence observations, bind this
combination. Historical333 is not a current-run claim. Full758 and649 remain
open; Doctor must inspect actual schema, not only quick_check/bookkeeping.

### E010 — Keep the reachable Memory web interface; retire the unreachable TUI target

Actual frozen Go1.26.7 dependency inspection finds the embedded web package
reachable through memory serve/Brain HTTPHandler, and no TUI dependency in the
shipped Brain command tree. Twelve actual Go/native CLI pairs also reject
memory tui/web/dashboard/console commands identically. Evidence and exact
source/binary hashes live in migration/evidence/memory-ui-763.

Port the existing reachable browser interface and its auth/API interactions
with the native Memory HTTP server (#759/#762). It provides a cross-platform
interface, including systems without the macOS app, so keeping it preserves
useful access without inventing a separate frontend or database contract.
Retire the unused standalone TUI migration target rather than adding a new
CLI product that the shipped command tree never exposed. The maintained
native app and scriptable Memory CLI remain complementary access paths.
This delegated maintainer decision changes the implementation plan, not the
running frozen Go oracle. #763 stays open until the reachable HTTP UI is
implemented, independently reviewed and tested. No zero-case parity claim.

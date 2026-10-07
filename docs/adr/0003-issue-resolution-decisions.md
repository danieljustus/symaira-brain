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

### E011 — Strip fragments from automatic redirect Referer headers

Retain the native removal of URI fragments when deriving the Referer for a
redirect from the previous URL. RFC9110 section10.1.3 requires Referer values
to exclude URI fragments and userinfo. Fragments are client-side navigation
state and can contain sensitive values; redirecting them to an HTTP server is
not a useful compatibility requirement. This delegated decision deliberately
preserves the safer native behavior rather than reproducing the observed
Go1.26.7 fragment disclosure.

The independent immutable Fetch522/1f82 replay has eight actual HTTP pairs:
seven exact matches and one fragment-derived Referer difference. Preserve its
unmodified Go/native bytes under migration/evidence/fetch-referer-policy-773.
This policy applies to the automatically derived redirect header. An explicit
caller-supplied Referer remains caller-owned and byte-compatible with Go; the
same replay proves that case. Do not generalize this exception to other
headers, request targets, routing or authentication.

The Fetch successor must document the boundary in its ADR and contract matrix
and exercise it as a separate intentional-difference case. Its four additional
proxy absolute-form port mismatches and declared CI-runner failure are defects
requiring correction, not covered by this exception. All historical failures,
frozen Go and existing assertions remain intact. Full #773 and native six-target
acceptance remain open.

## Implementation choices confirmed by independent review (2026-10-03)

These choices repair concrete contracts; they add no new parity exception.
The original observations, failed candidates and actual executables remain
recoverable. A local pass is not native-platform or release acceptance.

- Clean managed and credential path joins lexically, as pinned Go does, before
  selecting files. With link/../home, resolving the symlink or leaving parent
  components can choose another account. Do not use filesystem canonicalization.
  Doctor/Setup and newly admitted Copilot/Kimi file routes need consistent
  probe/read ownership, raw Unix path bytes and native Windows volume rules.
- Preserve raw Unix diagnostic path bytes through human output. Convert to
  JSON with Go's one-invalid-byte replacement rule. Sharing a small Core text
  primitive avoids a Guard dependency on the installer and repeated lossy
  conversions; capability checks, audit denial and error precedence remain.
- Preserve quiet Unix SIGPIPE when an actual process stdout reader closes
  during admitted Memory Set output. The already committed write remains
  committed. Apply the existing signal handler at that actual stdio boundary;
  embedded/custom writers keep ordinary checked errors. Do not reset signals
  globally or treat every output failure as SIGPIPE.
- Derive proxy Basic authentication from retained, percent-decoded username
  and password bytes, including empty-field presence. UTF-8 normalization
  can truncate credentials and cause real407 responses. Reuse already locked
  dependencies and preserve caller/proxy header ordering. This does not approve
  inherited origin-userinfo, HTTP2 or trailer differences.
- Resolve Browse state keys once before IPC and share one keyed store. Provider
  denial, malformed values and timeouts fail closed before operations. For
  destructive cleanup with a configured key, authenticate every v3 body/header
  before acting on its timestamps, including an untrusted key_source=none.
  Preserve the pinned sorted stop order and complete partial-state observation.
  Non-destructive legacy/plaintext metadata reads have their separate contract.
  No cipher reimplementation or blanket redaction bypass is justified.

PR808's owned atomic profile publication was independently approved and then
normally squash-merged as e3dbda6c only after all protected and affected native
Linux/macOS/Windows checks succeeded and no review thread remained open. It
fixes that bounded race; migration epics remain open. Other branches integrate
this main normally and retain fresh checks. PR809's Windows-only readonly test
lint and PR801's Windows named-pipe fixture corrections preserve production
contracts and test assertions, with complete native failure logs retained.


## E-012: WAL-Konfiguration eng begrenzen und ein Wartebudget teilen

Bei parallelen Öffnungen kann die SQLite-WAL-Umschaltung den Busy-Handler
umgehen. Wiederholt wird deshalb ausschließlich diese Konfiguration vor der
Migration, bei typisiertem primärem SQLITE_BUSY5 und Autocommit. SQLite-Warten
und kurze eigene Pausen teilen das bestehende monotone5-Sekunden-Budget.
Migrationen und Anwendungsoperationen werden nicht wiederholt. Fehler,
Transaktionen, Daten und5000ms Busy-Timeout bleiben erhalten.

Begründung: Ein globaler Retry würde teilweise geschriebene Daten erneut ändern;
ein neues volles Budget pro Versuch kann die Laufzeit unkontrolliert verlängern.
Echte Lock- und Mutantenprüfungen belegen diese Unterschiede. PR803 enthält
Quellstand2fe09ea plus vollständige unabhängige327 Tests,590 Lesevergleiche,
Set/Delete-/Output-/Gegenkontrollen und die ursprünglichen Windows-Logs.
Veröffentlichung5dcac5b ist regulär auf main5e232 integriert. Die native
Drei-OS-Prüfung dieses veröffentlichten Stands bleibt erforderlich. Die größere
historische37-Generationen-/Default-Reparatur bleibt getrennt und unkompiliert.

## E-013: Bei Source-Timeouts zuerst den tatsächlichen Fehler erhalten

Die unveränderte25-Sekunden-Grenze bleibt bestehen. Ein atomisches Journal
außerhalb der verglichenen Fixture-Verzeichnisse erhält gestartete und
zurückgekehrte Prozesse, vollständige rohe Streams, fertige Vergleichspaare und
den noch vorhandenen Fehlerzustand vor dem Aufräumen. Live-Snapshots tragen
explizite Grenzen und erfüllen selbst keine Vergleichs- oder Cleanup-Assertion.

Begründung: Der ursprüngliche Windows-Log benennt weder den konkreten Fall noch
die blockierte Phase. Ein größeres Timeout würde dieses Informationsdefizit
verdecken. Ein kalter Go-Cache ist ebenfalls nicht bewiesen: Rust folgt Go mit
demselben eigenen Cache. PR806 veröffentlicht8359c0b nach unabhängigen111
Originalpaaren, zwei wirklichen Mutanten und einem wirklichen25-Sekunden-
Timeout mit erhaltenen NUL/FF-Streams und allen256 Markerbytes. Produktionsbytes
bleiben identisch zu kompiliertem58fe50; dessen überprüftes ELF wird wieder
verwendet, keine neue Rust-Kompilierung behauptet. Der Windows-JobObject-Versuch
ist nur vorbereitet. Ursache und echte native Annahme bleiben offen.

## E-014: Dateieigentümer anhand des SDK-Vertrags auswählen

Bei automatisch gefundenen Providern gelten Gos PATH- und ErrDot-Regeln.
Leere oder relative PATH-Einträge dürfen ohne wirksamen Opt-in keinen lokalen
Credential-Provider starten. PATH-Komponenten werden vor Dateizugriff lexikalisch
bereinigt; eine Symlink-Auflösung wäre ein anderer Vertrag. Insbesondere
symlink/../bin kann sonst das falsche Konto beziehungsweise Programm auswählen.

Begründung: Vier wirkliche Default-CLI-Vergleiche zeigten ursprüngliche unerwartete
lokale Provider-Ausführung. Die erste Korrektur48fc363 hat zwei weitere statisch
belegte Fehler: fehlende lexikalische Bereinigung und abgelehnte gültige
GODEBUG-Suffixe. Beide werden in einem neuen unveränderlichen Nachfolger
korrigiert. Stack-selektive Go-Debug-Matcher, Windows-Raw-argv0, Batch-Dateien,
ACL-/Sharing-Verhalten und native UTF16-Pfade erhalten eigene explizite Gates;
es gibt keine pauschale SDK-Ausnahme oder Annahme aus Cross-Compilation.
Standalone-Runner und ursprüngliche57 Rohbelege bleiben unverändert.

## Integrationsstand am 2026-10-04

PR807 wurde nach vier aktuellen geschützten Checks und echtem macOS-Vertragslauf
regulär nach main5e232700bb9031fc34d4995465a4037837540abb gemerged. Das beweist
keinen echten signierten Release. Noch11 PRs und33 Issues bleiben offen.
PR805 war lokal auf diese Basis integriert (a114f9a), nicht jedoch in GitHub-main.
Am 2026-10-05 wurde PR805 regulär nach main
f6cfc25bb6059c68cd65b448fb3bcccb046b2e07 gemerged. Der genaue Candidate
d9e211849b30225e6ce4ed375ee64627f333eddc bestand die unabhängige Guard-Prüfung,
die nativen Linux/macOS/Windows-Gates und den lokalen strengen Rust-Workspace-Check.
Das schließt weder die vollständige Guard-Migration noch einen Release ab.
PR803 muss vor seinen Memory-MCP/HTTP-/Schema-Nachfolgern integriert werden.

Die nächste kompilierte Prüfgruppe behandelt Skills-Argumente; historische
Schema-Defaults, Provider-Discovery, Usage-Retry-After, alle13 Brain-Konfigurations-
felder und Memory-Sync-Backends laufen in getrennten überprüfbaren Schritten.
Vorbereitete Tests heißen weiterhin vorbereitet. Unabhängig statisch geprüfte
Quellen heißen weiterhin unkompiliert. Apple/TestFlight, externes Darwin-Volume
und signierter Release mit sieben Tagen Beobachtung behalten ihre tatsächlichen
Hardware-/Zeit-/Release-Gates. Nur vollständige verifizierte Issue-Kriterien
rechtfertigen einen Issue-Abschluss.


## E-015: Tests mit tatsächlich ausgelieferten Schemas und vollständigen Plattformvoraussetzungen

Die historische Memory-Prüfung verwendet benannte Grenzen vor den realen SQL-
Migrationen. Ein künstliches Schema wird durch die eingefrorene ausgelieferte
001-SQL ersetzt, wenn seine Defaults und Nullability den tatsächlichen Vertrag
verfälschen. Die strenge184-Default-Prüfung, Rollback-, Callback- und
Gesamtzustandsprüfungen bleiben unverändert. Guard-Tests legen sämtliche
Discovery-Elternverzeichnisse an, wenn sie einen gesunden Zustand mit fehlenden
Dateien prüfen: Windows meldet fehlende Elternverzeichnisse anders als fehlende
Dateien. Die Produktionsklassifizierung bleibt an die Referenz gebunden.

Begründung: Ein Test soll die gemeinte reale Eingangslage erzeugen. Er darf
weder vorhandene Migrationen als noch ausstehend behandeln noch falsche
Defaultwerte akzeptieren. Alle tatsächlichen Fehlversuche, Quellen und
ausgeführten Binärdateien werden vor der Korrektur aufbewahrt. Ein neuer
Quellstand gilt erst nach der ganzen erneuten Prüfung als bestanden.

## E-016: Oracle-Herkunft durch echte Git-Metadaten belegen

Die Skills-Referenz wird aus einem eigenen lokalen Klon mit echtem .git-
Verzeichnis und explizitem -buildvcs=true gebaut. Go1.26.7 erkennt die .git-
Datei eines Linked-Worktrees hier nicht als SDK-VCS-Herkunft. Sämtliche
Assertions zu Revision und unmodifiziertem Quellstand bleiben erhalten.
Der gezielte Builder verwendet private Benutzerkonfiguration, feste offline
Abhängigkeiten und vollständige Blob-, Dateimodus- und Clean-Maps vor und nach
dem Build. Der allgemeine Go-Oracle-Helper bleibt unverändert.

Begründung: Herkunft ist eine Eigenschaft der tatsächlich gebauten Referenz.
Eine Metadatenabweichung durch die Build-Umgebung rechtfertigt keine
Abschwächung der Ausgabe- oder Provenienzvergleiche.

## E-017: Diagnostik an tatsächliche Fehlerphasen binden

Source-Ausgabeprüfungen laden den Logger über seine eigene aufgelöste
Geschwisterdatei. Der vollständige Linux-Fehler des aktuellen PR806 stammt
vom Python-Import vor dem Vergleich; nach der Korrektur bestehen alle42
ursprünglichen Ausgabe-/Writer-Vergleiche und vier reale Kontrollgruppen mit
den archivierten tatsächlichen Binaries. Eine neue Rust-Kompilierung wird
daraus nicht abgeleitet. Der Windows-Fortschrittsnachweis begrenzt den ersten
Timeout auf die Phase nach Go-version-Ausgabe und vor Go-build. Ein natives
Experiment mit eigenem externen Watchdog unterscheidet Polling und Cleanup.
Der bestehende25-Sekunden-Vergleich bleibt unverändert.

Begründung: Vollständige Logs und Originalartefakte erlauben gezielte
Korrekturen. Ein längerer Timeout ohne Ursachenbeleg würde einen möglichen
Prozess-Lebenszyklusfehler verdecken. Auch die aktuelle Windows-Abweichung
bei MCP-Initialize und der10-Sekunden-Timeout der unveränderten Go-Memory-
Referenz bleiben offene Nachweisgates; erfolgreiche Memory-Einzeljobs
ersetzen diese vollständigen Vergleiche nicht.

## E-018: Geprüfte historische Memory-Effekte mit dem Store integrieren

Die historische37-Generationen-Reparatur wird in PR803 integriert. Sie besitzt denselben Konstruktor, dieselben Default-/Dateninvarianten und dieselbe atomare Transaktionsgrenze wie der geprüfte WAL-Öffnungsfix. Zwei überlappende Store-PRs würden zusätzliche Zwischenzustände erzeugen. Der unabhängige63fbd320-Lauf besteht alle17 Stufen mit392 Tests und37 Reparaturen/Wiederöffnungen; original gescheiterte Go-Ergebnisse bleiben als absichtliche Korrekturen sichtbar. Im integrierten a30bba8b bleiben alle549 kompilierten Rust-/Cargo-Dateien identisch; eine separat geprüfte Windows-Diagnose kommt hinzu. Frische native Drei-OS- und geschützte CI bleibt vor Merge erforderlich. Die früheren Vorbereitungsstände oben bleiben historische Angaben.

## E-019: Diagnoseprotokollierung dem bestehenden Zeitbudget zurechnen

Ein langlebiges fsync-Journal darf die Timeoutgrenze nicht verlängern. Die neue Windows-Memory-Diagnose erhält nach erfolgreichem Spawn eine absolute5s-Deadline; unmittelbar nach jedem begin-Ereignis wird das verbleibende Budget neu berechnet. Bei null Restzeit erfolgt kein initialize/ping; die positive Assertion schlägt nach eigener Prozessbereinigung fehl. Der vollständige unabhängige Ursprungsbefund und seine deterministische Quellprojektion bleiben erhalten. Die korrigierte Diagnose ist unabhängig statisch freigegeben und veröffentlicht; tatsächliche Windows-Ausführung und ursprüngliche Fehlerursache bleiben offen.

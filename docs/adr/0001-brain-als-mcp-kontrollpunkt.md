# ADR 0001: Brain wird der MCP-Kontrollpunkt; das `symguard`-Binary entfällt

> **Status**: **Angenommen** — D1 und D6 von Daniel bestätigt am 2026-08-27; die
> übrigen Entscheidungen folgen ihnen. Die drei offenen Fragen bleiben offen und
> werden bei der Umsetzung der jeweiligen Schritte beantwortet.
>
> **Date**: 2026-08-27
> **Scope**: `internal/gateway`, `internal/broker`, `internal/profile`, `internal/policy`, `guard/`; nachgelagert `symaira-browse/internal/policy/symguard.go`
> **Verwandt**: [`docs/ARCHITEKTUR.md`](../ARCHITEKTUR.md) §1 und §4, `../../docs/repo-konsolidierung.md` §12.7, [`guard/README.md`](../../guard/README.md)

## Context

Zwei Fragen sind unabhängig voneinander aufgekommen und lassen sich nicht unabhängig
voneinander beantworten:

1. **Der Feature-Wunsch.** Sichtbarkeit und Kontrolle über *alle* MCP-Server auf der
   Maschine: protokollieren, welcher Harness wann welches Tool benutzt hat, und
   festlegen, welcher Harness welchen Server überhaupt sehen darf — lesend oder auch
   schreibend.
2. **Der Konsolidierungs-Rest.** `symguard` wurde am 2026-08-21 (#268) als nested
   Modul nach `guard/` absorbiert. Eine Prüfung am 2026-08-27 ergab: es wurde
   verschoben, nicht integriert.

Der Zusammenhang: Frage 1 verlangt einen Kontrollpunkt im Datenpfad. Genau das sollte
`symguard proxy` werden. Solange Frage 2 offen ist, ist Frage 1 nicht beantwortbar —
und umgekehrt entscheidet die Antwort auf Frage 1, wozu `guard/` überhaupt noch da ist.

### Constraints (nicht verhandelbar)

1. **ARCHITEKTUR.md §4 „Brain ↔ Guard Boundary"** ist als *„verbatim — do not weaken"*
   markiert. Brain implementiert keine Approval-Prompts, keine Risikoklassifikation,
   kein Schema-Pinning, keine Output-Redaction.
2. **Zero-Stdio-Pollution** (§4 Konsequenz 3): `symbrain mcp` emittiert auf stdout
   ausschließlich JSON-RPC-Frames. Ein eigener CI-Job (`stdio-hygiene`) hält das.
3. **repo-konsolidierung.md §12.7** ist bindend: die MCP-Discovery-Dreifachmodellierung
   wird auf *eine* Stelle reduziert, und diese Stelle ist Brain (brain#326 /
   cockpit#60). Nichts in dieser ADR darf eine vierte Stelle schaffen.
4. **Go-Modulgrenzen.** Ein Root-Modul kann `guard/internal/...` nicht importieren,
   solange `guard/` ein eigenes Modul ist — dieselbe Falle, die AGENTS.md für
   `seek/internal/...` dokumentiert.

## Verified Findings

Alles Folgende wurde am 2026-08-27 am Code geprüft, nicht aus der Dokumentation
übernommen.

### F1 — `gateway` + `broker` sind bereits ein vollständiger MCP-Proxy

| Paket | Rolle |
| :--- | :--- |
| [`internal/gateway`](../../internal/gateway/) | MCP-**Server**-Seite: Handshake mit dem Harness, Katalog-Merging, Tool-Namespacing, Request-Routing, Error-Mapping (643 LoC) |
| [`internal/broker`](../../internal/broker/) | MCP-**Client**-Seite: Kind-Server spawnen, `initialize`, `tools/list`, `tools/call`, Health-Checks, Crash-Detection, Restart mit Backoff |

Harness rein, Kind-Server raus, Policy und Audit dazwischen. Das ist die Bauform eines
MCP-Proxys, und sie läuft produktiv. Die Transportschicht, die `symguard proxy` fehlt,
existiert in Brain seit Monaten.

### F2 — Die Begrenzung auf vier Server ist deklarativ, nicht strukturell

`broker.ServerConfig` (`internal/broker/server.go:50`) kennt `Name`, `BinaryPath`,
`Args`, `InitTimeout`, `CallTimeout`, `MaxRestarts`, `BackoffBase`. **Nichts daran ist
core-spezifisch** — der Broker könnte heute jeden stdio-MCP-Server spawnen.

Die Begrenzung sitzt an genau zwei Stellen darüber:

- `profile.Servers` (`internal/profile/profile.go:57`) ist ein festes Struct mit vier
  Feldern. Der Kommentar sagt warum: *„This is a fixed struct rather than a map —
  symbrain composes exactly these servers."*
- `internal/policy` pflegt versionierte Tool-Listen im Repo (`vaultTools`,
  `memoryTools`, `usageTools`), damit unbekannte Tools default-deny sind.

Beides ist eine Entscheidung, keine technische Schranke.

### F3 — `symguard proxy` existiert nicht, und `symguard` wird nicht ausgeliefert

`guard/README.md:160` führt `proxy` als *„design intent, not yet shipped"*, ebenso
`pin`. Im Code existiert nur die Konfigurationsform (`ProxyConfig`,
`guard/internal/config/config.go:68`), die von nichts gelesen wird.

Ausgeliefert wird das Binary ebenfalls nicht:

| Kanal | Zustand |
| :--- | :--- |
| `.goreleaser.yml` (Root) | baut nur `symbrain` |
| `Makefile` (Root) | erwähnt `guard` nicht |
| `homebrew-tap/Formula/symguard.rb` | `disable!` seit 2026-08-24 |
| `guard/.goreleaser.yml` | vorhanden, released nach `danieljustus/symaira-guard` — **archiviert** |

Letzte öffentlich verfügbare Version: v0.4.1, im archivierten Repo.

### F4 — Eine funktionierende Integration ist dadurch still gestorben

`symaira-browse/internal/policy/symguard.go` delegiert Risikoentscheidungen an das
externe Binary, gefunden per `exec.LookPath`. `symaira-browse/cmd/symbrowse/daemon.go:204`
kommentiert: *„the guard decides when it is present"*. Fehlt es, fällt symbrowse auf
seine lokale Policy zurück — **ohne Fehlermeldung**.

Das Binary ist seit der Absorption nicht mehr installierbar. Die Delegation ist damit
tot, und weil sie fail-soft degradiert, fällt es nicht auf.

### F5 — `guard/` liegt außerhalb sämtlicher Sicherheits-Gates

Alle Root-Jobs nutzen `go-version-file: go.mod` bzw. `./...`; Go ignoriert
Unterverzeichnisse mit eigener `go.mod`:

| Gate | Deckt `guard/` ab? |
| :--- | :--- |
| `govulncheck` (`ci.yml:150`) | **Nein** — guards Deps werden nie auf CVEs geprüft |
| `coverage` (`ci.yml:257`) | **Nein** |
| `mutation.yml` | **Nein** |
| `dependabot.yml` (`directory: "/"`) | **Nein** — keine Dependency-Updates |
| CI-Job `guard` (`ci.yml:96`) | Ja: fmt, vet, build, test |

Ausgerechnet das Sicherheits-Tool ist das einzige Modul ohne CVE-Scanning. Folgeschaden:
corekit steht bei v0.14.0 (guard) gegen v0.15.0 (root), und `bump-core.yml` pflegt nur
das symvault-Manifest, holt die Drift also nie ein.

### F6 — Der Feature-Wunsch ist Brain-Semantik, nicht Guard-Semantik

Sortiert man den Wunsch aus dem Context nach dem Grenzverlauf aus §4:

| Teilwunsch | Modell |
| :--- | :--- |
| loggen, welcher Harness wann was benutzt hat | Brains JSONL-Audit, nur über mehr Server |
| welcher Harness welchen Server sehen darf | Brains `profile` + `policy`, nur über mehr Server |
| lesend vs. schreibend | neu, aber am Handshake entscheidbar → Brain-Zeitpunkt |
| Approval-Prompt, Risikoklasse, Pinning, Redaction | **nicht Teil des Wunsches** |

Es geht nicht darum, fremde Tools *anzubieten*, sondern sie zu *sehen, protokollieren
und beschneiden*.

### F7 — `annotations` sind der naheliegende read/write-Marker, aber nur eine Selbstauskunft

`broker.Tool` (`internal/broker/protocol.go:72`) parst heute `name`, `description`,
`inputSchema` — **keine** `annotations`. Die MCP-Hinweise `readOnlyHint` und
`destructiveHint` wären die naheliegende Quelle für eine read/write-Klassifikation, sind
aber eine Aussage des Servers über sich selbst: für Bequemlichkeit brauchbar, als
Sicherheitsgrenze wertlos.

## Decisions

### D1 — §1 wird präzisiert, nicht gestrichen: Durchleitung ist keine Aggregation

ARCHITEKTUR.md §1 verbietet den „generischen MCP-Hub / Aggregator" mit der Begründung:
*„dafür gibt es bereits genug generische Aggregatoren, und dort läge keine
Differenzierung."*

Diese Begründung trifft **Bündelung als Bequemlichkeit** — Brain als komfortabler
Sammelanschluss, damit der Nutzer weniger Einträge in der Harness-Config hat. Sie
trifft **nicht** die Durchleitung als Kontrollpunkt, bei der der Zweck Sichtbarkeit und
Beschneidung ist und die Tool-Weitergabe nur das Mittel.

§1 wird entsprechend umformuliert: verboten bleibt Aggregation *ohne* Policy-Gewinn.
Erlaubt wird Durchleitung, deren erklärter Zweck Exposure-Kontrolle und Audit ist.

Nach der Regel aus repo-konsolidierung.md §12.7 („ein dritter Durchgang braucht ein
neues Argument, nicht denselben Eindruck") ist F6 dieses neue Argument: der Wunsch fällt
nachweislich auf Brains Seite der §4-Grenze.

### D2 — `profile.Servers` wird eine Map; die vier Cores behalten ihre Presets

Das feste Struct weicht einer Map `map[string]ServerConfig`. Die vier bekannten Aliase
(`vault`, `memory`, `skills`, `usage`) behalten unverändert ihre Modi, Presets und ihr
Default-Deny aus `internal/policy` — für sie ändert sich nichts.

Ein Eintrag mit unbekanntem Alias ist kein Validierungsfehler mehr, sondern ein
**fremder Server**: er braucht `command`/`args` (bzw. `url`) und wird über D3
regiert. Die Herkunft dieser Angaben ist der Harness-Inventar-Pfad aus brain#326
(Schema 2 liefert genau `command`, `args`, `url`, Env-Namen) — es entsteht keine neue
Discovery-Stelle, Constraint 3 bleibt gewahrt.

### D3 — Fremde Server bekommen ein zweites Policy-Modell: allow/deny statt Preset

Brains Default-Deny funktioniert, **weil** die vier Cores ein im Repo gepflegtes
Tool-Universum haben. Für beliebige Server gibt es keine Referenzliste; das Modell trägt
dort nicht und wird nicht künstlich weitergezogen.

Stattdessen, in dieser Reihenfolge ausgewertet:

1. **Server-Ebene**: `enabled` — sieht dieses Profil den Server überhaupt?
2. **Klassen-Ebene**: `access = "read" | "write"` — bei `read` sind nur Tools exponiert,
   die als lesend klassifiziert sind (D4).
3. **Tool-Ebene**: `tools_allow` / `tools_deny`, deny gewinnt — dieselbe Semantik wie
   heute.

Neu gegenüber den Cores ist, dass ein *unbekanntes* Tool eines fremden Servers bei
`access = "write"` und leerem `tools_allow` **exponiert** wird. Das ist eine bewusste
Abweichung vom Core-Default-Deny und muss in der README so benannt werden: für fremde
Server ist Brain ein Filter, kein Whitelist-Wächter.

### D4 — read/write kommt aus `annotations`, aber das Profil gewinnt immer

`broker.Tool` wird um `annotations` erweitert (F7). Die Klassifikation läuft:

1. Ein expliziter Eintrag im Profil (`tools_read` / `tools_write`) entscheidet.
2. Sonst gilt `readOnlyHint: true` als lesend.
3. Sonst gilt das Tool als **schreibend** — die sichere Annahme bei fehlender Angabe.

Die Selbstauskunft des Servers ist damit ein Vorschlag, der die Konfiguration nie
überstimmt. Bei `access = "read"` wird das Ergebnis zusätzlich im Audit vermerkt, damit
sichtbar bleibt, worauf die Einstufung beruhte.

### D5 — §4 bleibt unangetastet

Brain bekommt **keine** Approval-Prompts, **keine** Risikoklassen, **kein**
Schema-Pinning, **keine** Output-Redaction. Der Zeitpunkt bleibt der Handshake bzw. der
Katalogaufbau; das Audit bleibt das leichte JSONL-Log (das bereits `profile`, `server`,
`tool`, `status`, `duration_ms` schreibt und über `corekit/auditkit` hash-gekettet ist).

Wer Call-Time-Enforcement braucht, bekommt sie über D6 — im selben Prozess, aber als
klar getrennte Schicht, nicht durch Aufweichen von §4.

### D6 — Das `symguard`-Binary wird eingestellt; `guard/` wird ins Root-Modul eingeschmolzen

Die Prüfung (F3) hat gezeigt: `symguard` ist bereits de facto eingestellt — deprecated
in Homebrew, ohne Build, ohne Release, seit der Absorption ohne inhaltliche Änderung.
Der Zustand ist nicht entschieden, sondern liegengeblieben. Diese ADR entscheidet ihn.

- Die nested `guard/go.mod` **entfällt**. `guard/` wird ein normales Paketverzeichnis
  des Root-Moduls.
- Das eigenständige `symguard`-Binary entfällt. Sein Kommandosatz wird zu
  `symbrain guard <decide|scan|doctor|grants>` — dasselbe Passthrough-Muster wie
  brain#243 für `vault|memory|skills`.
- `guard/.goreleaser.yml`, `guard/Makefile`, `guard/.golangci.yml`, `guard/.gitignore`
  und die duplizierte Repo-Möblierung (`LICENSE`, `CODE_OF_CONDUCT.md`,
  `CONTRIBUTING.md`, `SECURITY.md`) werden gelöscht; `guard/CHANGELOG.md` wird in den
  Root-CHANGELOG überführt. `guard/README.md` und `guard/AGENTS.md` bleiben als
  Modul-Dokumentation, werden aber auf den neuen Repo-Kontext korrigiert (heute
  beschreiben sie `symaira-guard/` als Repo-Root und verlinken zwei nicht existierende
  Dateien).

Das löst F5 vollständig als Nebeneffekt: mit einem Modul greifen `govulncheck`,
`coverage`, `mutation` und `dependabot` sofort, und die corekit-Drift kann strukturell
nicht wiederkehren.

**Warum kein `guard/api`-Paket mit `replace`-Direktive** (das etablierte Muster für
absorbierte Tools, vgl. `seek/api`): jenes Muster existiert, damit ein Modul
*eigenständig baubar und testbar* bleibt — sinnvoll bei `seek`, das ein eigenes
`cmd/symseek` behält. Für `guard` wird die Eigenständigkeit hier gerade aufgegeben; ein
`replace` auf ein Modul, dessen Pfad auf ein archiviertes Repo zeigt, wäre die
Konservierung genau des Problems, das diese Entscheidung auflöst.

### D7 — symbrowse hängt auf `symbrain guard decide` um und degradiert hörbar

`symaira-browse/internal/policy/symguard.go` sucht künftig `symbrain` statt `symguard`;
das Wire-Protokoll (ein JSON auf stdin, ein JSON auf stdout, fail-closed auf `deny`)
bleibt unverändert — die Prozessgrenze ist weiterhin das ganze Interface.

Zusätzlich: das Fehlen des Entscheiders wird **sichtbar** gemacht (`symbrowse doctor`
plus eine Log-Zeile beim Daemon-Start). F4 konnte nur entstehen, weil der Ausfall
lautlos war.

### D8 — Bis D6 umgesetzt ist, kommt `guard/` unter dieselben Gates

Die Gate-Lücke aus F5 ist ein Sicherheitsbefund und wartet nicht auf den Umbau.
`dependabot.yml` bekommt sofort einen zweiten `gomod`-Eintrag für `/guard`, und
`govulncheck` einen zweiten Lauf mit `guard/go.mod`. Beides ist mit D6 wieder
wegzuwerfen — das ist billiger als ein ungescanntes Sicherheits-Tool.

## Consequences

**Positiv**

- Der Feature-Wunsch wird auf vorhandener, produktiv laufender Infrastruktur erfüllt;
  es entsteht kein zweiter Proxy und kein zweiter Prozess im Datenpfad.
- Ein Binary, ein Modul, eine corekit-Version, ein Satz Gates.
- Guards echte Substanz (`grant`, `capability`, `approval`, `sequence`, `policy/rule`)
  wird erstmals erreichbar, statt in einem nicht ausgelieferten Binary zu liegen.
- symbrowse bekommt seinen Entscheider zurück.

**Kosten und Risiken**

- **Paketkollision.** Nach dem Einschmelzen existieren `guard/internal/audit`,
  `guard/internal/policy`, `guard/internal/config` und `guard/internal/output` neben
  gleichnamigen Root-Paketen. Die Import-Pfade unterscheiden sich, technisch ist das
  kein Konflikt — lesbar ist es nicht. Eine spätere Zusammenführung ist ein eigenes
  Vorhaben und **kein Teil dieser ADR**.
- **§4 wird schwerer zu halten.** Solange Guard ein eigenes Binary war, erzwang die
  Prozessgrenze die Trennung. Im selben Modul wird sie zu einer Konvention, die
  `guard/internal/archguard` (Import-Richtungs-Enforcer) und die AGENTS.md-Regel tragen
  müssen. Das ist der ernsteste Einwand gegen D6.
- **Zwei Policy-Modelle nebeneinander** (Preset für Cores, allow/deny für fremde
  Server). Vertretbar, weil sie unterschiedliche Wissensstände abbilden — aber
  dokumentationspflichtig, sonst wirkt es wie Zufall.
- **Ein Server im Profil wird zum Vertrauensakt.** Wer einen fremden Server einträgt,
  gibt ihm Brains Prozess und Env. Guards `spawn`-Allowlist (deny-by-default) muss
  deshalb vor dem ersten fremden Spawn greifen, nicht danach.
- Zwei Audit-Logs (`~/.local/share/symguard/` und Brains XDG-Audit-Dir) bleiben
  vorerst bestehen; ihre Zusammenführung ist Folgearbeit.

## Non-Goals

- Kein Aggregator aus Bequemlichkeit: Brain nimmt einen fremden Server nur auf, wenn
  Exposure-Kontrolle oder Audit der Zweck ist. Tools ohne Policy-Interesse bindet der
  Nutzer weiter direkt an seinen Harness.
- Kein Approval-Prompt, keine Risikoklassifikation, kein Pinning in den Brain-Paketen
  (D5).
- Keine vierte MCP-Discovery-Stelle (Constraint 3).
- Kein Remote-/Netzwerk-Zugriff; stdio bleibt der Transport.

## Offene Fragen

1. **Wie wird ein fremder Server im Katalog benannt?** Die Cores nutzen `vault_`-Präfix
   bzw. Pass-through. Bei beliebig vielen Servern sind Namenskollisionen wahrscheinlich;
   ein Präfix pro Alias wäre konsistent, bläht aber die Tool-Namen auf.
2. **Wann darf Brain einen fremden Server spawnen?** Lazy beim ersten Aufruf (wie heute)
   ist sparsam, verschiebt Fehler aber in den Call. Beim Katalogaufbau ist ehrlicher,
   kostet aber Startzeit pro Harness-Verbindung.
3. **Bleibt `guard/README.md` ein eigenständiges Produkt-README?** Es beschreibt heute
   ein Produkt mit eigenem Namen, eigener Vision und eigenen Badges. Nach D6 ist es eine
   Schicht — die README muss das entweder abbilden oder verschwinden.

## Implementation Order

1. **D8** — Gates schließen (dependabot, govulncheck). Unabhängig, sofort.
2. **D6** — nested Modul auflösen, `symbrain guard <cmd>`, Möblierung aufräumen.
3. **D7** — symbrowse umhängen, Degradierung hörbar machen.
4. **D1** — ARCHITEKTUR.md §1 umformulieren. *Muss vor Schritt 5 stehen*, sonst wird
   gegen einen gültigen Beschluss gebaut.
5. **D2 + D4** — `profile.Servers` als Map, `annotations` im Broker.
6. **D3** — zweites Policy-Modell, README-Abschnitt zur Semantik fremder Server.

Schritte 1–3 sind auch dann richtig, wenn D1 abgelehnt wird: sie räumen eine
Konsolidierung zu Ende, unabhängig von der Richtungsfrage.

## Related

- `docs/ARCHITEKTUR.md` §1 (Nicht-Ziele), §4 (Brain ↔ Guard)
- `../../docs/repo-konsolidierung.md` §9 (geteilte Discovery), §12.7 (Cockpit-Grenze)
- brain#326 — Harness-Inventar Schema 2, Quelle der Transport-Details für D2
- brain#243 — Passthrough-Muster, Vorbild für `symbrain guard <cmd>`
- symbrowse#52 — die Delegation aus F4

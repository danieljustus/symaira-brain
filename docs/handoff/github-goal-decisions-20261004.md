# Entscheidungen zum GitHub-Ziel

Stand: 4. Oktober 2026. Ziel: offene Issues fachlich abschließen und PRs regulär in main mergen. Das Ziel ist noch nicht abgeschlossen. Die Entscheidungsvollmacht stammt aus der ausdrücklichen Anweisung des Nutzers; Routineentscheidungen werden selbständig getroffen.

## CI verwendet unveränderliche Vergleichsquellen mit vollständiger Historie

Die Windows-Skills-Prüfung benötigt den fest gebundenen Go-Vergleichscommit `01f41906e2e021db3c693ec97617bb701e4dad8a`. Der aktuelle Checkout allein enthält diesen Commit nicht. Vor dem unveränderten hermetischen Builder wird deshalb genau dieser Commit mit `git fetch --no-tags origin <SHA>` beschafft. Ein Fetch mit `--depth=1` verändert die Shallow-Eigenschaft und lässt den anschließenden Shared-Clone wieder ohne den Commit zurück; der unabhängige Gegenversuch reproduziert exit128. Vollständige Historie erhält das erwartete Clone-Verhalten, ohne einen beweglichen Branch als Vergleichsquelle einzuführen.

Der anfängliche Fehler, die verworfene Depth-Variante, alle ursprünglichen Artefakte und der unabhängige Review bleiben erhalten. Der korrigierte Stand verändert keine Produktdatei oder Vergleichsbedingung. Die Entscheidung ist im Repository in `docs/adr/skills-ci-immutable-parent-acquisition.md` dokumentiert und wird nach Quellenreview in alle fünf betroffenen PRs übernommen. Neue native und geschützte Checks am veröffentlichten Stand bleiben erforderlich.

## Build-Verzeichnis wiederverwenden und historische Belege schützen

Für die nächste vollständige Memory-Prüfung wird das vorhandene Brain-Build-Verzeichnis bevorzugt. Ein zweites vollständiges Build-Verzeichnis würde den begrenzten Speicher zusätzlich belasten. Vor dem ersten neuen Cargo-Schreibzugriff werden alle vorhandenen ELF-Bytes, ihre ursprünglichen Dateimetadaten und die zugehörigen Quellen-/Cargo-/Rollenbelege vollständig gebunden und verlustfrei erhalten. Ein Cachefund beweist keine frühere Ausführung.

Regenerierbare Source-Compiler-Zwischenstände und nicht ausführbare Go-Cacheobjekte wurden ausschließlich innerhalb zuvor geprüfter konkreter Grenzen archiviert und danach entfernt. Alle 1.415 rlib/rmeta-Dateien und 5.845 nicht ausführbaren Go-Cacheobjekte besitzen verifizierte Originalarchive und Metadaten. Die 741 Source-ELF-Pfade und neun historischen Go-Cache-ELFs bleiben erhalten. Künftige Go-Builds erhalten einen neuen Cache-Namensraum, damit automatisches Cache-Trimming keine historischen ausführbaren Belege entfernt. Die Rückprüfung der Wiederherstellung schrieb keine Cachedateien. Historische Inodes oder ctime werden nicht als wiederherstellbar ausgegeben.

Nach dem vollständigen Quellenreview wurden die fünf CI-Nachfolger regulär veröffentlicht. Für die Memory-Prüfung wurde anschließend eine weitere konkrete Freigabe durchgeführt: genau 739 ELF-Pfade im bereits freigegebenen Source-Build-Verzeichnis. Alle Originalbytes waren vorher vollständig in vorhandenen Archiven geprüft; alle 674 Hardlink-Gruppen waren vollständig erfasst. Die 6.563 übrigen Dateien, Quellen, SDKs, Rohberichte und außerhalb liegende feste Executables bleiben erhalten. Vorher-/Nachher-Prüfungen und ein fsync-Journal binden jeden einzelnen Unlink. Die freie Speichermenge stieg während dieser Phase von 2.129.633.280 auf 3.722.055.680 Bytes; der beobachtete globale Zuwachs ist keine isolierte Einspargarantie. Die früheren 741 Live-ELF-Beobachtungen bleiben historische Laufzeitbelege und werden nicht als heutige Dateipräsenz ausgegeben.

Die erste Vorprüfung stoppte vor jedem Unlink an einem abweichenden st_blocks-Speicherzähler einer geschützten Datei. Eine spätere vollständige Beobachtung entsprach wieder sämtlichen ursprünglichen Feldern und Bytes; die Ursache der zwischenzeitlichen Speicherzählerabweichung ist unbekannt. Der ursprüngliche Fehler bleibt erhalten. Inhalt, Modus, Eigentümer, mtime und Inode-Identität bleiben verpflichtend; außerhalb des Löschbereichs wird die beobachtete Blockbelegung gesondert dokumentiert. Für die Löschkandidaten wurde die aktuelle Blockbelegung erneut exakt geprüft.

Vor jeder schweren neuen SDK-/Compiler-Zuteilung werden mindestens 2 GiB tatsächlicher freier Speicher geprüft, nochmals nach SDK-Entpacken vor dem ersten Compiler; während der Ausführung gilt die 700-MiB-Untergrenze. Eine exklusive Build-Zuteilung, höchstens zwei Compilerjobs und eigene temporäre Verzeichnisse begrenzen Konkurrenz und halten Abbrüche nachvollziehbar. Beobachtete globale Speicherzuwächse werden nicht als isolierte Einspargarantie dargestellt.

## SQLite-Prüfung bindet die tatsächlich geladene Bibliothek

Eine Python-Versionsnummer allein bindet unter Linux die SQLite-Version nicht. Die offizielle Python-3.13.7-Auslieferung kann weiterhin das problematische Host-SQLite laden. Die Memory-Prüfung verwendet deshalb den unveränderten offiziellen Interpreter mit einem privat gebauten, fest gebundenen SQLite 3.50.4. Vor Produktkindern müssen geladene Bibliothek, Funktionsadressen, Version und Source-ID sowie REAL-DEFAULT-, FTS5- und echte NULL-Beschädigungskontrollen stimmen.

Die private Interpreter-Verlagerung wird zuerst tatsächlich geprüft. Ein expliziter Prozessaufruf über den ELF-Loader soll globale Suchpfadänderungen vermeiden; sein Erfolg wird bis zur Ausführung nicht vorausgesetzt. Der Provider-Selektor wird ausschließlich den drei historischen Prüfer-Interpretern zugestellt und vor Go-/Rust-Produktkindern verbraucht. Die ursprünglichen 17 Prüfgruppen, Fallzahlen, Assertions und inneren Fristen bleiben vollständig erhalten.

## Dateisystemgrenzen bleiben ausdrücklich sichtbar

Ein auf Darwin vom Kernel tatsächlich mit EILSEQ92 zurückgewiesener ungültiger Dateiname kann als nicht ausführbare Domäne ausgewiesen werden. Er wird nicht als bestandene Parität oder Negativkontrolle gezählt. Alle auf dieser Plattform zugelassenen Originalfälle und anwendbaren Kontrollen müssen bestehen; Linux führt weiterhin sämtliche Originalfälle aus. Gültige Unicode-Ersatzzeichen und rohe Git-Pfade bleiben verpflichtend.

Bei Skills-Kontrolldateien bleibt das ursprüngliche No-Follow-Verhalten bestehen. Metadaten einer gewöhnlichen, innerhalb der behaltenen Verzeichniskapazität liegenden Symlink-Zieldatei dürfen nur den exakten ursprünglichen Fehlertext bestimmen. Außerhalb liegende oder unbestätigte Ziele behalten die stärkere Verweigerung. Es gibt keinen Inhaltszugriff auf Symlinkziele, keine ambient canonicalization und kein blockierendes Öffnen von FIFOs oder Geräten.

## Abschluss nur mit überprüfbarem Ergebnis

Quellenfreigabe, vorbereitete Fälle, historische erfolgreiche Läufe und aktuelle native Akzeptanz bleiben getrennte Aussagen. Jede PR wird erst nach erforderlichen aktuellen nativen Jobs, geschützten Checks und aufgelösten Review-Threads regulär gemergt. Teilimplementierungen schließen keine umfassenden Migrations-Issues. Tatsächliche Apple-/Geräteprüfungen, signierte Releases und eine verlangte siebentägige Beobachtungszeit werden mit ihren realen Ergebnissen abgeschlossen.

Der maschinenlesbare Arbeitsstand liegt in `/workspace/goal-status.json`; vollständige ursprüngliche Fassungen bleiben vor jeder Aktualisierung erhalten. Die fachlichen Repository-ADRs gehören jeweils zu den überprüften Änderungen. Diese übergreifende Arbeitsnotiz wird beim nächsten passenden Dokumentationsstand zusammen mit den endgültigen Belegen übernommen.

## Aktueller Lauf und konkrete CI-Folgerungen (20261004-143545 UTC)

Die unabhängig vollständig geprüfte Memory-Prüfmethode erhält eine konkrete exklusive Zuteilung für BrainTarget, Port11434 und höchstens zwei Compilerjobs. Der ursprüngliche Quellstand5a4 und das versiegelte Prüfprogramm bleiben während des Laufs unverändert. Der echte private Python3.13.7 startet korrekt; seine ausführbaren und gemappten Originaldateien werden erhalten. Nach SDK-Entpacken gilt vor dem ersten Compiler weiterhin mindestens2GiB, durchgehend mindestens700MiB. Diese Zuteilung ist keine vorweggenommene Laufzeitfreigabe.

Neue tatsächliche CI-Fehler werden an ihrer ersten Ursache korrigiert und in isolierten Nachfolgeständen geprüft: Source hat eine Windows-Dokumentationswarnung; Usage hat vier konkrete Clippy-Befunde und eine Go-Formatprüfung historischer Belegdateien; Memory verweigert den Skills-Elternvergleich aufgrund des belegten globalen Ausgabeinhabers. Historische Belege werden nicht nachträglich formatiert. Vergleichskriterien und Herkunftsprüfungen bleiben verbindlich; eine ausdrückliche Änderung muss den konkreten Eigentümer und unveränderte Rohkriterien belegen. Nachgelagerte fehlende Artefakte ersetzen keine Ursachenanalyse.

Skills797 und Guard805 haben ihre tatsächlichen vollständigen Windows-Migrationsprüfungen bestanden; noch wartende macOS- und GUI-Prüfungen bleiben Merge-Voraussetzungen. Die Schema-Integrationca583 ist nach vollständigem unabhängigem Quellreview bereit für Laufzeitprüfungen, ohne dass dadurch Issue649 als erledigt gilt.

## Einheitlicher Guard-Workspace nach echtem Build-Abbruch

Der echte private Python3.13.7 und SQLite3504 einschließlich aller sechs vorgesehenen Fehlerkontrollen haben bestanden. Der anschließende Build konnte das verlangte Paket symguard-cli im aktuellen Memory5a4-Workspace nicht finden; es fehlt auch im unveränderlichen Git-Baum und ist keine sparse Checkout-Lücke. Die langfristige Entscheidung ist deshalb eine normale Integration des bereits vollständig quellgeprüften Guard805-Elternstands in einen neuen Memory-Kandidaten. Die ursprünglichen32Elterngates und17Memory-Validatoren bleiben vollständig verbindlich. Ein fremdes Guard-Binary oder das Weglassen des Pakets würde keinen aktuellen gemeinsamen Workspace nachweisen. Der ganze abgebrochene Versuch bleibt erhalten; seine erfolgreiche Providerprüfung ist keine Freigabe eines Memory-Produkts.

## Präzise Ereigniskriterien im Daemon-Prüfwerkzeug

Die vier tatsächlich ausgeführten Linux/Windows-Architekturjobs stoppen im Python-Selbsttest vor den Daemon-Produktfällen. Acht Kindprozesse liefern inzwischen je cli.begin, cli.launch.begin, cli.spawned und cli.end, zusammen mit einem initialen Bindungsdatensatz33Zeilen. Der alte Selbsttest erwartet17 aus dem früheren zweistufigen Vertrag. Die Entscheidung ist eine präzise Aktualisierung des Selbsttests mit allen33Sequenznummern sowie vollständiger Fall-, Phasen-, PID-, Status- und Rohbelegzuordnung. Produktionsjournal, Prozessfristen und native Abnahmekriterien bleiben unverändert; zusätzliche absichtliche Zuordnungs- und Phasenfehler müssen abgewiesen werden. Die vollständigen vier ursprünglichen Fehlerlogs werden vor der Änderung gesichert.

## Prüfkriterien vor Merge, CI-Push nach Review

Der vollständig unabhängig geprüfte Usage-Nachfolger6572 wurde regulär veröffentlicht, um frische CI zu erhalten. Zwei auf einzelne Funktionen begrenzte Lintattribute bewahren exakt transkribierte SDK-Arithmetik; die übrigen Änderungen sind Dokumentationscode-Markierung und Copy für fünf private Skalarfelder. Die vollständigen Rechenroutinen sind unverändert. Make/Release formatieren weiterhin alle802 aktiven Go-Quellen; vier historische .go-Originale unter migration/evidence werden als unveränderliche Belege erhalten. Die bisherigen Formatter-Grenz-, Pfad-, Fehler- und Nichtumschreibekriterien gelten weiter.17 tatsächliche Fake-Formatter-Routingkontrollen beweisen nur die Auswahl, keine echte Go- oder Rust-Laufzeit.

Frische Linux/macOS/Windows-CI setzt einen regulären Kandidatenpush voraus. Deshalb verlangt die ADR alle tatsächlichen Prüfungen vor Merge oder Release und erlaubt den überprüften Push zum Start der CI. Dieser Push ist keine Merge-Freigabe. Beim vollständigen Skills-Review ist zusätzlich der EOF-Zustand des JSON-Scanners als konkret quellbelegter Fehler erkannt worden; alle Originale werden vor einer isolierten Korrektur erhalten, und die exakten Parse-Error-Kriterien bleiben verbindlich.


### 15:36 UTC — Weitere tatsächliche CI-Ursachen

Memorya111 wurde vollständig unabhängig auf Quellenebene freigegeben; alle47Belege wurden auch von Root vollständig gegen SHA, Länge und native Metadaten geprüft. Ein neuer, getrennt geprüfter Laufzeitplan muss die tatsächlichen19Pakete und die unveränderten32+17Prüfgruppen binden. Ressourcen bleiben bis dahin freigegeben.

Skillsfb430 erhält einen isolierten Nachfolger für den einzigen verbliebenen P2: Go prüft am Dateiende den aktiven Scannerzustand mit einem Leerzeichen. Daher brauchen abgeschnittene Zahlen, Literale und Escapes ihre konkrete Fehlermeldung; ein pauschales unerwartetes Ende wäre keine langfristig korrekte Kompatibilität. Alle68vollständigen unabhängigen Belege wurden geprüft; historische Kandidaten und Eingaben bleiben erhalten.

Usage657 scheitert in den echten Linux- und Windows-Läufen jetzt an drei Lints im Testcode. Entscheidung: idiomatische Array-Chunks und eine klare Test-Hilfsfunktion, soweit semantisch gleich; keine Lockerung der strengen Lints, Produktionsalgorithmen oder Kriterien. Ein isolierter Nachfolger wird vor Veröffentlichung vollständig geprüft.

Daemon8a95 führt auf beiden Windows-Architekturen35Anfragen und3Negativkontrollen erfolgreich aus, scheitert danach aber beim Aufräumen eines gesperrten temporären Verzeichnisses. Entscheidung: den Lebenszyklus eigener Prozesse und Handles korrigieren, statt Aufräumfehler zu ignorieren. Dies ist weiterhin keine vollständige native Abnahme.

Source806d07 besteht die strengen Windows-Quellprüfungen, scheitert danach an der zu engen Ganzdatei-Zuordnung des geänderten CLI-Ausgabeowners zum Skills-Elternstand. Entscheidung: nur eine begründete, vollständig gebundene Quellenprofil-Erweiterung prüfen; keine pauschale Paritätsausnahme. Die vollständigen Originalprotokolle aller fünf fehlerhaften Jobs sind unverändert gesichert. macOS- und GUI-Jobs bleiben wartend und werden nicht umgangen.


## Fortsetzung: zusammenhängende Quellen und echte Prozesslebensdauer

Entscheidung: Memory wird regulär mit den bereits geprüften Guard- und Source-Eltern zusammengeführt. Der vollständige Methodenreview des veröffentlichten a111-Stands fand zehn erforderliche Source-Pfade, die im Git-Stand fehlten. Sparse-Materialisierung kann fehlende Git-Quellen nicht herstellen. Deshalb bleiben alle 32 alten Prüfgruppen und 17 Memory-Zusatzgruppen verpflichtend; sie werden weder ausgelassen noch mit fremden Binärdateien ersetzt. Der neue zusammenhängende Stand erhält einen vollständigen unabhängigen Quellreview und einen neu versiegelten Laufplan, bevor Ressourcen zugeteilt werden. Die alte b8ef-Methode wird nicht auf andere Quellen umgedeutet.

Entscheidung: Die gemeinsame Skills-Ausgabequalifizierung verwendet explizite Profile sämtlicher fünf vollständigen, unveränderlich gebundenen Owner-Dateien. Der geprüfte Source-Nachfolger d559 behält den tatsächlichen Eltern-Fetch, Archiv-/Binary-Build, Prozessbeobachtung und alle Vergleichsbedingungen. Ein unbekannter oder gemischter Quellenstand scheitert weiterhin. Root verifizierte alle 30 unabhängigen Review-Proofkörper vollständig; der normale Push nach PR #806 startet frische CI. Diese Veröffentlichung ist keine native Source-Abnahme.

Entscheidung: Der Registry-Harness von PR #801 wartet unter Windows auf das tatsächliche Ende des bestätigten Autostart-Prozesses. Ein behaltenes Kernelhandle verhindert, dass eine wiederverwendete numerische PID einen fremden Prozess zum Bereinigungsziel macht. Nach einem Stopfehler darf ausschließlich dieses bestätigte Objekt begrenzt bereinigt werden; erzwungene Bereinigung bleibt ein Prüfungsfehler. Verzeichnis-Retries oder unterdrückte Cleanup-Fehler würden den Lebensdauerfehler verdecken. Root prüfte sämtliche 44 Autorenbelege, alle 3.701 alten und 3.723 aktuellen Git-Körper und führte die 13 reinen API-Mocktests erneut erfolgreich aus. Der geprüfte Stand 854 wurde normal veröffentlicht; sechs echte native Plattform-/Architekturläufe bleiben verpflichtend.

Archivgrenze des Daemon-Nachweises: Alle verfügbaren Mitglieder der vier historischen nativen ZIPs sind vollständig geprüft und erhalten. Die tatsächlichen PE-/ELF-Produktpayloads sind darin nicht enthalten und besitzen keine separaten Originalarchive. Beobachtete Binary-Hashes werden deshalb nicht als vollständige Binary-Archivierung ausgegeben. Alte Binärdateien anderer Quellstände ersetzen keine fehlenden Originale.

Entscheidung: PR #804 erhält ausschließlich die geprüften strukturellen Änderungen an drei Oracle-Testdateien für striktes Clippy. Zwei vollständige Zweiergruppen-Iterationen und die unveränderte Umgebungsvorbereitung behalten ihre Semantik. Alle übrigen 4.747 alten Dateien bleiben bytegleich. Nach vollständigem Root-Review ist e650 normal veröffentlicht; numerische SDK-Parität und echte Kontrollen werden erst aus aktuellen tatsächlichen Läufen abgenommen.

Entscheidung: Der Skills-EOF-Scanner bildet die aktive Go-Scannerzustandsbehandlung durch ein einzelnes ASCII-Leerzeichen bei erschöpftem Token-Lookup ab. Allgemeine Container- und String-EOF-Fehler behalten ihr bisheriges Verhalten. Der vollständige unabhängige Review des b493-Stands samt allen 27 Proofkörpern ist geprüft. Die 1.112 Unix-/1.070 Windows-Fälle und neun echten Kontrollen sind vorbereitet, noch nicht ausgeführt. Ein Quellbefund ohne Laufzeitnachweis schließt #764 nicht.

Entscheidung: Vor künftiger Wiederverwendung des Root-eigenen Brain-Targets sind jetzt sämtliche 5.951 vorhandenen Dateikörper samt nativen Metadaten frisch überprüft und vollständig archivgebunden. Die 549 zuvor unarchivierten Cachekörper besitzen zusätzliche verlustfreie Archive; die ursprünglichen ELF- und Rollenarchive bleiben maßgeblich. Cachearchivierung belegt Erhaltung, keine frische Kompilierung oder Ausführung. Target, Compiler und Port 11434 bleiben derzeit freigegeben und ohne ausführenden Eigentümer.

Entscheidung: Die Brain-Laufzeitmethode verwendet eine ausdrücklich gebundene Beobachter-Python-Hülle für rekursive Beobachter, während das tatsächliche offizielle SDK-ELF separat nachgewiesen wird. Memory-Provider bleibt Beobachterabhängigkeit und wird nicht still in die Brain-Produktion importiert. Diese neue Methode wird vollständig unabhängig geprüft. Die noch fehlende ganze Skills-Owner-Qualifizierung bleibt eine ausdrückliche Zuteilungssperre.

Aktueller Zielstatus: Kein weiterer Merge und keine Issue-Schließung seit main 9353520a. PR #797 hat inzwischen alle vier geschützten Checks und die drei nativen Migrationsjobs erfolgreich bestanden; die vollständige aktuelle macOS-Rohbelegprüfung läuft vor dem normalen Merge. Weitere PRs besitzen noch offene aktuelle CI-/Abnahmeanforderungen. Das Gesamtziel bleibt offen.


## Veröffentlichung der geprüften Skills-Quellen als Entwurf

Entscheidung: Der exakt geprüfte Skills-Stand b493 wird normal als Entwurfs-PR #813 veröffentlicht, um die vorhandenen drei nativen CI-Plattformen tatsächlich ausführen zu lassen. Der unterschiedliche Autorenreview gab eine Quellvorbereitung frei und erteilte in seinem Umfang keine Veröffentlichungsfreigabe. Root trifft die gesonderte Veröffentlichungsentscheidung innerhalb des vom Nutzer autorisierten Gesamtziels nach vollständiger Bericht-/Proofprüfung. Das Urteil wird nicht als native Abnahme umgedeutet. Vor Merge bleiben alle Kompilierungs-, Prozess-, Kontroll-, Berechtigungs-, Signal- und geschützten Prüfungen verpflichtend.

Die unabhängige Prüfung der Brain-Laufzeitmethode hat zusätzlich eine konkrete PID-Wiederverwendungsgefahr im Beobachter nachgewiesen: Eine bloße PID-Liste darf beim Cleanup nicht mit der Identität eines späteren fremden Prozesses neu gebunden werden. Entscheidung: ursprüngliche PID und Startidentität müssen über die gesamte Beobachtung erhalten bleiben; wiederverwendete Identitäten werden vor jedem Signal abgewiesen. Der eingefrorene alte Methodenstand bleibt erhalten, und ein korrigierter Nachfolger benötigt vollständigen unabhängigen Review. Es gibt weiterhin keine Laufzeitzuteilung.


## Dynamische Python-Prüfer sind vollständige Eingabeeigentümer

Der unabhängige Brain-Methodenreview hat mit einer reinen Gegenprobe nachgewiesen, dass SourceFileLoader trotz `-B` bzw. `dont_write_bytecode` einen ungebundenen, timestamp-gültigen Bytecodecache ausführen kann, während die gehashte Quelldatei unverändert bleibt. Entscheidung: dynamische eigene Prüfer werden aus vorher verifizierten, gebundenen Quellbytes direkt kompiliert und ausgeführt. Modulmetadaten und Importidentität bleiben korrekt; unbekannter Cache wird nicht als Source-Nachweis akzeptiert. Die bestehende Memory-Methode nutzt denselben Loader und erhält dieselbe eng begrenzte methodische Korrektur. Alte Framework-Körper bleiben vollständig erhalten. Die 32 ursprünglichen Stage-Funktionen und 17 Validatorgruppen samt Eingaben, Assertions und Fristen bleiben unverändert. Die neue Loaderbehandlung ist eine ausdrücklich neu zu prüfende Methodendelta, keine behauptete Bytegleichheit des alten Frameworks.


## Skills-Status: Beobachtung statt unbestätigter Reparatur

Der aktuelle vollständige macOS-Belegreview bestätigt die beiden ursprünglichen #621-Wünsche für verwaltete Links: saubere Links bleiben in-sync, Änderungen durch den installierten Hermes-Link werden als Render-Drift/harness-changed sichtbar. Die zusätzliche RENDER-Spalte gehört zum normalen `skills status`; ein `--render`-Flag existiert nicht und die PR-Beschreibung wurde korrigiert. Entscheidung: der Status bleibt eine schreibfreie Beobachtung. Ein neuer Render→Library-Import, force-Overwrite oder veränderter Base-Fortschritt wäre ein eigener schreibender Produktvertrag und wird aus diesen Belegen nicht erfunden. Ausbrechende Cache-Symlinks bleiben sicher verweigert; generelle Reparatur durch solche Links wird nicht zugesagt. Die ursprünglichen Lektionen werden nicht still überschrieben. Der geprüfte Render-Vertrag und die Library als maßgebliche Quelle sind in der mit PR #797 versionierten ADR dokumentiert.

## Guard-Fixture auf Darwin

Die tatsächliche aktuelle macOS-CI von PR #805 scheitert zuerst beim Anlegen des exakten ungültigen UTF-8-Fixturenamens im Warning-Test. Entscheidung: ausschließlich die vom eigenen Darwin-Kernel mit EILSEQ92 verweigerte Rohname-Operation wird als nicht ausgeführt mit Rohbytes, Operation, Fehler und Cleanup protokolliert. Das ist keine bestandene Rohpfad-Parität. Alle zugelassenen Rollen und Linux-Rohpfade bleiben verpflichtend. Der zusammenhängende Fixture-Owner wird korrigiert, auch wenn ein späterer gleicher Ersteller im abgebrochenen alten Lauf noch nicht erreicht wurde; tatsächlicher erster Fehler und quellenabgeleitete Schwesterkorrektur bleiben klar unterschieden. Production, Fristen, ursprüngliche Vergleiche, SDKs und Pins bleiben unverändert. Die gesamte ursprüngliche Jobausgabe bleibt vollständig erhalten.


## Regulärer Merge von PR #797

PR #797 wurde nach vollständiger aktueller Quellen-/Linux-/Windows-/macOS-Belegprüfung, allen vier geschützten Checks, allen drei nativen Migrations-/Init-Jobs und leeren Review-Threads mit erwarteter Head-SHA regulär gesquasht. Neues main: c74110425d04dd62c7f2c0f09bb86561bd1ac20e. GitHub bestätigt #621 als completed geschlossen. Root las den vollständigen letzten Review und verifizierte alle 35 eigenen Proofkörper sowie sechs vollständige verfügbare Originalartefakte samt nativen Metadaten. Die macOS-Artefakte enthalten keine Mach-O-Payloads; deren Archivierung wurde nicht behauptet. Die beiden ursprünglichen Body-Alternativen von #621 sind erfüllt; bestehende Confinement-/Schreibgrenzen bleiben versioniert dokumentiert.

Folgeentscheidung: abhängige Quellstände werden regulär mit diesem neuen main zusammengeführt, bevor sie als aktueller gemeinsamer Stand abgenommen werden. Historische Reviews und echte Läufe behalten ihre ursprüngliche Quellbindung. Es werden keine alten erfolgreichen Ergebnisse auf eine neue Kombination umetikettiert. Der Gesamtstand bleibt offen: 31 Issues und zehn PRs sind aktuell noch offen.


## Portable Memory-Loader-Fixtures

Die aktuellen tatsächlichen Memory-CLI-Jobs auf macOS und Windows scheitern zuerst in den reinen Provider-Fixtures: Der echte Manifestprüfer löst den owned Bibliothekspfad korrekt kanonisch auf, während der Fake-Loader den ursprünglichen lexikalischen temporären Pfad erwartet. Entscheidung: Der Fixture-Eigentümer bindet seine eigene temporäre Wurzel einmal vor dem Ableiten sämtlicher Fake-Bibliothek-/Extension-/Manifestpfade. Die strikten exakten Pfad-/Flag-/Adressprüfungen bleiben erhalten; die Produktion wird nicht gelockert. Echte owned Alias-Gegenproben und gesunde Ausgangsfälle müssen diesen Unterschied prüfen.

Nach diesem frühen Fehler wurde Go noch nicht eingerichtet. Der bisherige always-Diagnoseschritt scheitert dann zusätzlich an `go version`. Entscheidung: fehlendes Go in dieser Diagnose ausdrücklich als nicht verfügbar protokollieren, ohne den tatsächlichen Go-Build oder die nativen Abnahmeschritte zu überspringen. Ist das ausführbare Go vorhanden, bleibt ein tatsächlicher Aufruffehler ein Fehler. Erster Fixturefehler und sekundärer Diagnosefehler werden getrennt benannt. Beide kompletten ursprünglichen Plattformprotokolle bleiben erhalten. Der abgebrochene Windows-Init-Workspacejob gilt ebenfalls nicht als bestanden.


## Vollständig archivierte Cachedateien und strenge Skills-Lints

Entscheidung: Für die nächste reguläre Laufzeitvorbereitung wurden ausschließlich 549 vorher vollständig archivierte, nicht ausführbare Cachedateien (545 Rust-Metadaten und vier statische Bibliotheken) des freigegebenen Root-Targets entfernt. Vorher wurden sämtliche 5.951 lebenden Dateikörper, 4.375 vollständigen Archive und nativen Bindungen frisch geprüft; nachher bleiben alle 5.402 anderen Dateien und sämtliche 721 ELF-Dateien unverändert vollständig verifiziert. Jede einzelne Entfernung besitzt ein fsync-gesichertes Vorher-/Nachherjournal samt vollständiger Restaurationsbindung. Quellen, SDKs, ursprüngliche Archive und der bekannte leere Lease-Marker bleiben geschützt. Originalbytes, Rechte, Eigentümer und atime/mtime sind wiederherstellbar; neue Inodes/ctime/Blocklayout werden nicht als identisch zugesagt. Die beobachtete freie Kapazität stieg von 2.420.416.512 auf 2.754.936.832 Byte; parallele Allokationen verhindern einen behaupteten isolierten Gewinn. Es wurde keine Compiler- oder Produktlaufzeit gestartet.

Der neue Skills-Entwurfsstand b493 scheitert im tatsächlichen macOS-Job111480096141 bereits an 23 strikten Clippy-Befunden. Entscheidung: Namen, Imports, Dokumentation und semantisch gleiche Iteration/Konvertierungen werden im isolierten Nachfolger korrigiert. Kein globales Allow und keine veränderte Rohpfad-/Fehler-/Scannersemantik. Itembezogene begründete Ausnahmen dürfen bestehende unabhängige Bool-Eingaben und gequotete Go-Fehlermeldungen bewahren. Alle 1.112/1.070 ursprünglichen Fälle und neun echten Kontrollen bleiben erforderlich; sie wurden in diesem abgebrochenen Lauf nicht erreicht. Das vollständige ursprüngliche Protokoll ist erhalten.

# Headless-Regeln — LuminaRust auf Maschinen ohne GPU

Verbindliche Arbeitsregeln für die Umsetzung und Verifikation dieses Workspaces
auf **headless** Maschinen: kein Display, kein wgpu-Adapter, gemeinsam
genutzter Build-Host. Ergänzt `Agents.md` (Arbeitsregeln) und `DoD.md`
(normative Verifikation); es ersetzt nichts davon.

**Für wen diese Datei gilt:** jeden Build- oder Verifikations-Agenten auf so
einer Maschine. Die globale `AGENTS.md` verweist für LuminaRust hierher.

---

## 1. Grundsatz: keine GPU-Parität ist kein Grund, nicht zu implementieren

Die zentrale Regel dieser Datei. Auf einer headless Maschine lässt sich der
GPU-Pfad **nicht** prüfen. Das ist eine Aussage über ein **Gate**, nicht über
die Aufgabe.

**Verbindlich:**

1. **Implementieren.** Ein Task wird nicht wegen eines fehlenden Adapters
   zurückgestellt, vereinfacht oder auf eine CPU-Alternative umgebogen.
2. **Kompilieren.** Der Code muss auf der headless Maschine bauen — inklusive
   `#[cfg(feature = "gpu")]`-Pfade, die über `cargo check -p lumina-gui
   --features gpu` mitkompiliert werden. „Kompiliert nicht, weil kein Adapter"
   ist nie eine zulässige Begründung.
3. **Alles Prüfbare prüfen.** CPU-Pfad, Rezept-/Render-/Sidecar-Logik,
   klickbare headless Tests, CLI- und MCP-Pfade laufen vollständig. Diese
   Abdeckung ist echt und wird voll ausgewiesen.
4. **Den GPU-Teil als benanntes Gate führen.** Was nicht gemessen werden
   konnte, wird einzeln aufgeführt: Paint-Schritt (Shader-Kompilierung, MSAA,
   Textur-Sampling, Treiberfehler), VRAM-Pixel, jede CPU↔GPU-Byte-Parität.
5. **Nichts über den Adapter hinaus behaupten.** Weder „läuft auf GPU" noch
   „ist identisch zur CPU" noch „sollte funktionieren". Der Goldensatz lautet
   immer: *auf dem CPU-Pfad gemessen, der GPU-Pfad ungeprüft.*

**Was das praktisch heißt:** ein Task gilt als **halb** abgenommen, nicht als
abgelehnt. Genau diese Trennung erlaubt, auf headless Maschinen echte Arbeit
zu liefern, ohne irgendwo eine grüne Zahl zu erfinden.

---

## 2. Zwei Fehlerklassen, die diese Maschine provoziert

Beide sind gemessen und beide kosten erfahrungsgemäß eine ganze Verifikations-
runde, wenn man sie nicht vorab ausschließt.

### 2.1 „Kompiliert" wird als „verifiziert" gelesen

`cargo check` findet **keine Testfehler**. Gemessen am 2026-09-29: von vier
echten Fehlern in einem Task fand `cargo check` **keinen**. Sie lagen alle in
Testlogik (ein Test prüfte einen Zustand, den er nie erzeugte), im Trace-Format
(eine Formatangabe fehlte) und in Testvakuum (ein Test blieb grün, als die
Produktionsverdrahtung gelöscht war).

**Regel:** Ein Task gilt erst als geprüft, wenn seine Tests **gelaufen** sind.
Kompilieren belegt Ausführbarkeit.

### 2.2 Ein vakuoser Test sieht grün aus wie ein guter

Der häufigste Ausfall: ein Test ruft die Funktion, die er prüfen soll, **selbst
auf**, statt den Produktionspfad zu fahren. Er bleibt dann grün, wenn genau die
Verdrahtung gelöscht wird, die er belegen soll.

**Regel:** Jeder neue Test wird **mutiert** — die Produktionsverdrahtung, die er
behaupten soll, wird entfernt oder verfälscht und der Test muss **rot** werden.
Bleibt er grün, ist er vakuos und wird als solcher benannt, nicht als Deckung
geführt. Das ist `DoD.md` §10 in der Form, die man am häufigsten übersieht.

Merksatz aus `KITT-IGNORED-PANIC-56`: **eine Regel, die einen *Weg* abschließt
statt einer *Klasse*, ist eine Absichtserklärung** (`DoD.md` §9).

---

## 3. Was headless prüfbar ist

| Prüfbar headless | Nicht prüfbar ohne Adapter |
| --- | --- |
| volle Rezept-/Render-/Sidecar-Logik | Shader-Kompilierung, MSAA |
| klickbare GUI-Tests (`egui Context` + `LuminaApp`) | Textur-Sampling, Treiberfehler |
| CLI- und MCP-Parität, Byte-Identität | VRAM-Pixel, Live-Brush-Upload |
| alle `#[cfg(feature = "gpu")]`-Pfade **beim Kompilieren** | CPU↔GPU-Byte-Parität (`--features gpu-adapter-tests`) |
| Fehlerkanäle, Atomizität, CAS-Konflikte | kittest-Goldens gegen echten Adapter |

**Wichtige Unterscheidung, die oft falsch getroffen wird:** `#[ignore]`-Tests
mit `#[cfg(feature = "gpu")]` sind headless **echt lauffähig**, wenn sie keine
Adapterrouten betreten. In diesem Workspace sind die editorialen
GPU-Verweigerungen genau so gebaut und laufen ohne Adapter durch. Zu behaupten,
ein GUI-Test brauche immer Hardware, ist genauso falsch wie das Gegenteil.

**Gegenprobe:** ein Test, der an `CustomNativeAdapterSelectionError("No
adapter found")` scheitert, prüft den Produktpfad **nicht** — nicht weil er
grün ist, sondern weil er nicht gelaufen ist. Er wird als *nicht gelaufen*
geführt, nicht als *grün* und nicht als *rot*.

---

## 4. RAM und Parallelität auf geteilten Hosts

Gemessen am 2026-09-29 auf `code-dev`. **Die Zahlen sind host-spezifisch und
gehen hier bewusst weg** — sie stehen in der globalen `AGENTS.md` und in
`target/headless_env.sh`, nicht in diesem Dokument, weil sie veralten.

Gilt aber projektübergreifend:

- **Ein einziger Arbeitsstrom.** Nie mehrere Agenten und eigene Build-Läufe
  gleichzeitig. Parallele Toolchain-Instanzen vervielfachen den Speicherdruck.
- **Ein Build-Lauf zur Zeit**, und vor jedem davon den Speicher freigeben — der
  Page-Cache eines großen `target/`-Verzeichnisses wird sonst dem Container
  angelastet.
- **Den RAM-Deckel nicht als Antwort auf ein OOM erhöhen.** Die Überschreitung
  auf dem *Host* ist das Risiko, nicht die Belegung im Container.
- **Ein SIGKILL/OOM während des Buildens ist RAM-Druck, kein Defekt im Code.**
  Er wird nach Warten mit weniger Parallelität neu gemessen, nicht durch
  Ändern am Code „behoben". Ein solcher Kill als Befund zu melden ist die
  häufigste Fehlzuschreibung auf dieser Maschine.

Werkzeuge dieses Workspace unter `target/` (gitignored, nicht Teil des
Produkts): `setup_env.sh` (idempotente Umgebung), `headless_env.sh` (nur
sourcen), `free_mem.py` (Page-Cache freigeben), `verify_cpu_tasks.sh`
(reproduzierbarer Verifikationslauf).

---

## 5. Umgebungsresets

Der Container-Dateisystem-Container außerhalb des Projekt-Mounts wird
periodisch verworfen; `~/.cargo` und alles unter `/usr` und `/usr/local` sind
dann weg. **Toolchain und native Bibliotheken gehören auf einen persistenten
Mount** (`target/host-tools/`), nicht nach `$HOME`. Danach kostet ein Reset
Sekunden statt Minuten, und ein `SIGKILL` mitten im Compile ist kein
lostes Arbeitsergebnis, sondern ein Wiederholen.

Fehlersymptom eines Resets: `pkg-config: No such file or directory`,
`libraw_r not found`, `cargo: command not found`. Heilung: Setup-Skript, dann
neu messen — **nicht** den Code ändern.

---

## 6. Berichtspflicht

Ein Abschlussbericht auf dieser Maschine trennt drei Dinge, die leicht
vermischt werden:

1. **gemessen** — mit Testzahl oder Gate-Exitcode
2. **kompiliert, nicht ausgeführt** — zählt als ungeprüft
3. **nicht prüfbar hier** — benanntes Gate mit Begründung

Punkt 3 ist kein Makel, sondern die Information, die den nächsten Lauf
überhaupt möglich macht. Ein Bericht, der 2. als 1. darstellt, ist wertlos —
und zwar nicht nur für diesen Task, weil er die nächste Fehlersuche in die
falsche Richtung schickt.

**Nicht** als Verifikation ausgeben: eine grüne Zahl, die ein Compilieren
erzeugt hat; ein Gate, das nicht gelaufen ist; eine Erwartung, die man auf den
gemessenen Wert nachgezogen hat.

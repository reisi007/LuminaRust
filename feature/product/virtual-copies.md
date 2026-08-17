# Virtuelle Kopien

**Feature:** F-002 Virtuelle Kopien, F-014 Standardkopie-Regeln

## Inhaltsverzeichnis

- [Ziel](#ziel)
- [Datenmodell](#datenmodell)
- [Geteilte Artefakte und Referenzen](#geteilte-artefakte-und-referenzen)
- [Operationen](#operationen)
- [Standardkopie-Regeln (F-014)](#standardkopie-regeln-f-014)
- [Abnahme](#abnahme)

## Ziel

Mehrere Looks desselben Originals werden ohne RAW- oder Pixelduplikation
gespeichert. Jede Kopie ist eine eigenständige Entwicklung und kann separat
angezeigt, bearbeitet und exportiert werden.

## Datenmodell

Jede Kopie besitzt:

- stabile, sidecarweit eindeutige ID;
- Anzeigenamen;
- vollständiges, selbständig auswertbares Rezept;
- eigenen Masken-Layer-Zustand;
- eigenen Crop und eigene Geometrie;
- eigenen Exportstatus und eigene Exporthistorie;
- optional einen Status als aktuelle Standardkopie.

Die Standardkopie wird bei der Sidecar-Erstellung erzeugt und darf nicht
stillschweigend verschwinden. Kopien werden nicht über Arraypositionen
identifiziert.

Implizite Vererbung zwischen Kopien ist in v1 nicht vorgesehen. Eine Kopie kann
aus einer anderen erzeugt werden, erhält dabei aber ein vollständiges Rezept.

## Geteilte Artefakte und Referenzen

Jede virtuelle Kopie besitzt eine eigene Maskenbibliothek. Eine Kopie darf aber
auf stabile Maskenknoten einer anderen Kopie verweisen. Invertierung,
Feathering, Blur, Dichte und lokale Anpassungen liegen in der jeweils
referenzierenden Kopie.

Wird eine Quellkopie gelöscht, werden abhängige Graphdefinitionen automatisch
in die Zielbibliothek materialisiert. Die binären Payloads dürfen über
Content-Hash dedupliziert bleiben. Das Löschen der Quellkopie darf dadurch
keine aktive Zielmaske beschädigen.

Nicht mehr referenzierte Artefakte werden nur über einen expliziten und sicheren
Aufräumvorgang entfernt.

## Operationen

Zu unterstützen sind Erstellen, Duplizieren, Umbenennen, Sortieren, Aktivieren,
Zurücksetzen, Exportieren und Löschen. Jede Operation muss per Sidecar-
Roundtrip über Neustarts hinweg erhalten bleiben.

## Standardkopie-Regeln (F-014)

- In jedem Sidecar existiert jederzeit genau eine als Standardkopie
  gekennzeichnete Kopie. Sie wird beim Erstellen des Sidecars angelegt und darf
  weder durch eine UI-Aktion noch durch einen stillen Bereinigungsvorgang
  gelöscht werden.
- Das Umbenennen ändert ausschließlich den Anzeigenamen. Die stabile,
  sidecarweit eindeutige ID, das Rezept und die History bleiben unverändert.
- Kopien dürfen nach Anzeigename, Erstellzeitpunkt oder einer anderen
  ausdrücklich gewählten Sortierung neu angeordnet werden. Die Arrayposition
  ist niemals Identität; stabile IDs bleiben bei Umbenennung und Sortierung
  unverändert.
- Duplizieren erzeugt eine neue, stabile und sidecarweit eindeutige ID. Die
  duplizierte Kopie erhält ein vollständiges, eigenständig auswertbares Rezept
  sowie eine eigene History und eigene Aktivierungs-/Exportstatuswerte. Eine
  implizite Vererbung vom Duplikat oder von der Quellkopie ist nicht zulässig;
  gemeinsam nutzbare Artefakte müssen ausdrücklich als Referenz persistiert
  werden.
- Die Standardkopie darf nur gelöscht werden, wenn zuvor eine andere Kopie
  ausdrücklich als Standardkopie ausgewählt wurde und diese Übertragung
  erfolgreich persistiert ist. Gibt es keine andere Kopie, ist die Löschung
  verboten. Eine Löschaktion darf die Standardkopie nicht stillschweigend
  zurücksetzen, umbenennen oder durch eine neu erzeugte Kopie ersetzen.
- Das Löschen einer anderen Kopie entfernt sie aus der aktiven Kopienliste und
  schreibt den Vorgang in die persistente Sidecar-History. Abhängige geteilte
  Maskenartefakte werden gemäß den Referenzregeln materialisiert oder erhalten;
  andere Kopien dürfen dadurch nicht beschädigt werden.
- Wiederherstellen ist nur möglich, wenn die persistente History die
  vollständige Kopie einschließlich ID, Rezept, Masken-/Geometriestatus und
  relevanter Exportdaten enthält. Fehlt diese History oder ist sie
  inkompatibel, wird die Kopie nicht still rekonstruiert; die GUI muss die
  Wiederherstellung als nicht verfügbar beziehungsweise als Fehler anzeigen.

## Abnahme

- Zwei Kopien desselben RAW besitzen unterschiedliche Rezepte und Exporte.
- Eine Kopie kann eine Maske verwenden, während eine andere sie deaktiviert.
- Umbenennen und Sortieren verändern keine IDs.
- Das Löschen einer Kopie beschädigt keine andere Kopie und kein geteiltes
  Artefakt.
- Die Tests werden unabhängig auf fachliche Korrektheit und Abdeckung geprüft.

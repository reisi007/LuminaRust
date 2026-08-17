# Virtuelle Kopien

**Feature:** F-002 Virtuelle Kopien

## Inhaltsverzeichnis

- [Ziel](#ziel)
- [Datenmodell](#datenmodell)
- [Geteilte Artefakte und Referenzen](#geteilte-artefakte-und-referenzen)
- [Operationen](#operationen)
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

## Abnahme

- Zwei Kopien desselben RAW besitzen unterschiedliche Rezepte und Exporte.
- Eine Kopie kann eine Maske verwenden, während eine andere sie deaktiviert.
- Umbenennen und Sortieren verändern keine IDs.
- Das Löschen einer Kopie beschädigt keine andere Kopie und kein geteiltes
  Artefakt.
- Die Tests werden unabhängig auf fachliche Korrektheit und Abdeckung geprüft.

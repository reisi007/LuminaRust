# Generic model management — MODEL-MGMT-1

## Ziel und Grenze (Release 2.0)

`lumina-onnx` erhält eine optionale Backend-Infrastruktur zum expliziten
Installieren eines einzelnen, vollständig beschriebenen Modellartefakts über
HTTPS. Dieser erste Slice enthält **keinen** freigegebenen Katalogeintrag,
keine NAFNet-URL und keine Gewichte. Bis ein konkretes Artefakt seine Rechte-,
Provenienz- und Hash-Prüfung bestanden hat, gibt es weder GUI- noch CLI-Oberfläche
und keinen Katalog, der eine leere oder ungeprüfte Auswahl suggeriert.

## Sicherheits- und Lizenzvertrag

- Ein Download geschieht nur nach einer ausdrücklichen, pro Paket erteilten
  Zustimmung. Die Zustimmung muss den SHA-256-Inhalt des exakt angezeigten
  Lizenztexts und den Lizenzbezeichner binden; eine implizite oder pauschale
  Zustimmung genügt nicht. Lizenztext und Quelle werden vor Zustimmung
  offengelegt.
- Jeder Paketdeskriptor muss Modell-ID/Version, sicheren Dateinamen, HTTPS-
  Artefakt-URL, HTTPS-Provenienz-URL, SPDX-/Lizenzlabel, vollständigen
  Lizenztext, Größenobergrenze und einen verifizierten Pin
  `sha256:<64 lowercase hex>` tragen. `pending-integration` ist kein Pin.
- Fehlende Rechte, Provenienz oder Hash-Prüfung blockieren die Aufnahme in
  Katalog und Download-Konfiguration. Installierte Artefakte werden zusammen
  mit lesbaren Lizenz- und Provenienz-Metadaten gespeichert. In diesem Slice
  ist Provenienz ein gespeicherter Quellen-URL-Verweis im JSON-Metadatensatz;
  der Installer lädt keinen Provenienzinhalt. Eine URL ist nur eine Referenz,
  kein unabhängiger Provenienz- oder Signaturnachweis.
- Keine Hintergrund-/ungefragten Downloads. Ein explizit vom Nutzer gewählter
  Download eines einzelnen freigegebenen Modells ist zulässig.
- Tests sind vollständig offline: kein Internet und keine echten Modellgewichte.
  Transport wird injiziert; nur ein zukünftiger freigegebener Katalog darf
  darüber die HTTPS-Implementierung verwenden.

## Abnahme und Integrationsgrenze

`model-download` ist optional und nicht Teil der Default-Features. Die
Installationslogik validiert vor Netzwerkzugriff, streamt in eine benachbarte
Staging-Datei mit harter Größenbegrenzung, prüft den Hash vor Veröffentlichung
und installiert atomar. Redirects werden nicht verfolgt. Bestehende identische
Artefakte sind idempotent; ein belegtes Ziel mit anderen Bytes wird nicht
überschrieben. Fehler hinterlassen kein Teilziel und bewahren eine vorherige
Installation.

Dieser Slice stellt nur Backend-API und Offline-Tests bereit. UI/CLI/Katalog
werden separat integriert, nachdem ein konkretes Artefakt samt Lizenz, Herkunft
und Hash freigegeben ist.

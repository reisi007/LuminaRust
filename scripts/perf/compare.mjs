#!/usr/bin/env node
"use strict";

/**
 * compare.mjs — Criterion-Vergleich gegen Baseline- und Budget-Stores.
 *
 * Skeleton state (F-074-N2, 2026-08-19):
 *   - loads and schema-validates both stores,
 *   - scans --criterion-dir for Criterion estimates.json files,
 *   - prints a clear message when no measurement data exists.
 * The actual comparison logic (median vs. baseline/budget with
 * tolerance_ratio; exit 1 only for violations that belong to the active
 * mode) is implemented in F-074-N5 — see the PLACEHOLDER sections below.
 *
 * Methodology: feature/quality/performance-benchmarks.md (F-074)
 * Decision:    docs/adr/0003-performance-benchmarking.md (ADR 0003)
 *
 * Exit codes:
 *   0  ok (no violation, or no measurement data available)
 *   1  violation (baseline/budget exceeded; gate/warn modes)
 *   2  schema or usage error
 */

import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";

const EXIT_OK = 0;
const EXIT_VIOLATION = 1;
const EXIT_ERROR = 2;

const MODES = new Set(["report", "warn", "gate"]);

const DEFAULTS = {
  mode: "report",
  baseline: "perf/baseline.json",
  budgets: "perf/budgets.json",
  criterionDir: "target/criterion",
};

function printUsage() {
  console.log(`Verwendung: node scripts/perf/compare.mjs [Optionen]

Vergleicht Criterion-Messergebnisse (target/criterion) mit den committeten
Baseline- und Budget-Stores und meldet Performance-Regressionen.

Optionen:
  --mode <report|warn|gate>   Modus (Standard: report)
  --baseline <pfad>           Baseline-Store (Standard: ${DEFAULTS.baseline})
  --budgets <pfad>            Budget-Store (Standard: ${DEFAULTS.budgets})
  --criterion-dir <pfad>      Criterion-Ausgabeverzeichnis (Standard: ${DEFAULTS.criterionDir})
  --help                      Diese Hilfe anzeigen

Modi:
  report  Immer ausführen; erzeugt das Report-Artefakt. Exit 0.
  warn    Budgetierte Überschreitung erzeugt eine Warnung. Exit 0.
  gate    Nur Benchmarks mit "gate": true; Verletzung bricht ab. Exit 1.

Exit-Codes: 0 = ok, 1 = Verletzung, 2 = Schema- oder Nutzungsfehler.`);
}

function usageError(message) {
  process.stderr.write(`Nutzungsfehler: ${message}\n`);
  process.stderr.write("Aufruf mit --help zeigt die Optionen.\n");
  process.exit(EXIT_ERROR);
}

function schemaError(message) {
  process.stderr.write(`Schemafehler: ${message}\n`);
  process.exit(EXIT_ERROR);
}

function parseArgs(argv) {
  const opts = { ...DEFAULTS, help: false };

  // Supports both "--opt value" and "--opt=value".
  const takeValue = (name, inline, index) => {
    if (inline !== undefined) {
      return { value: inline, next: index };
    }
    const next = argv[index + 1];
    if (next === undefined) {
      usageError(`Option --${name} benötigt einen Wert.`);
    }
    return { value: next, next: index + 1 };
  };

  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    let name = arg;
    let inline;
    const eq = arg.indexOf("=");
    if (arg.startsWith("--") && eq !== -1) {
      name = arg.slice(0, eq);
      inline = arg.slice(eq + 1);
    }
    switch (name) {
      case "--help":
      case "-h":
        opts.help = true;
        break;
      case "--mode": {
        const { value, next } = takeValue("mode", inline, i);
        if (!MODES.has(value)) {
          usageError(`Ungültiger Modus "${value}" (erlaubt: report, warn, gate).`);
        }
        opts.mode = value;
        i = next;
        break;
      }
      case "--baseline": {
        const { value, next } = takeValue("baseline", inline, i);
        opts.baseline = value;
        i = next;
        break;
      }
      case "--budgets": {
        const { value, next } = takeValue("budgets", inline, i);
        opts.budgets = value;
        i = next;
        break;
      }
      case "--criterion-dir": {
        const { value, next } = takeValue("criterion-dir", inline, i);
        opts.criterionDir = value;
        i = next;
        break;
      }
      default:
        usageError(`Unbekannte Option "${arg}".`);
    }
  }
  return opts;
}

function loadStore(filePath, label) {
  let raw;
  try {
    raw = readFileSync(filePath, "utf8");
  } catch (err) {
    schemaError(`Store ${label} (${filePath}) kann nicht gelesen werden: ${err.message}`);
  }
  try {
    return JSON.parse(raw);
  } catch (err) {
    schemaError(`Store ${label} (${filePath}) ist kein gültiges JSON: ${err.message}`);
  }
}

function isPlainObject(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function validateBaseline(baseline) {
  if (!isPlainObject(baseline)) {
    schemaError("Baseline-Store muss ein JSON-Objekt sein.");
  }
  if (baseline.schema_version !== 1) {
    schemaError(
      `Baseline-Store: schema_version muss 1 sein (gefunden: ${baseline.schema_version}).`,
    );
  }
  if (baseline.environment !== null && !isPlainObject(baseline.environment)) {
    schemaError("Baseline-Store: environment muss null oder ein Objekt sein.");
  }
  if (!Array.isArray(baseline.benchmarks)) {
    schemaError("Baseline-Store: benchmarks muss ein Array sein.");
  }
  for (const entry of baseline.benchmarks) {
    if (!isPlainObject(entry)) {
      schemaError("Baseline-Store: jeder benchmarks-Eintrag muss ein Objekt sein.");
    }
    if (typeof entry.id !== "string" || entry.id.length === 0) {
      schemaError("Baseline-Store: jeder benchmarks-Eintrag benötigt eine nicht-leere id.");
    }
    if (typeof entry.median_ns !== "number") {
      schemaError(`Baseline-Store: Eintrag "${entry.id}" benötigt median_ns (Zahl).`);
    }
    if (typeof entry.p95_ns !== "number") {
      schemaError(`Baseline-Store: Eintrag "${entry.id}" benötigt p95_ns (Zahl).`);
    }
    if (entry.unit !== "ns") {
      schemaError(`Baseline-Store: Eintrag "${entry.id}" benötigt unit "ns".`);
    }
  }
}

function validateBudgets(budgets) {
  if (!isPlainObject(budgets)) {
    schemaError("Budget-Store muss ein JSON-Objekt sein.");
  }
  if (budgets.schema_version !== 1) {
    schemaError(
      `Budget-Store: schema_version muss 1 sein (gefunden: ${budgets.schema_version}).`,
    );
  }
  if (!Array.isArray(budgets.budgets)) {
    schemaError("Budget-Store: budgets muss ein Array sein.");
  }
  for (const entry of budgets.budgets) {
    if (!isPlainObject(entry)) {
      schemaError("Budget-Store: jeder budgets-Eintrag muss ein Objekt sein.");
    }
    if (typeof entry.id !== "string" || entry.id.length === 0) {
      schemaError("Budget-Store: jeder budgets-Eintrag benötigt eine nicht-leere id.");
    }
    if (typeof entry.budget_ns !== "number") {
      schemaError(`Budget-Store: Eintrag "${entry.id}" benötigt budget_ns (Zahl).`);
    }
    if (typeof entry.tolerance_ratio !== "number") {
      schemaError(`Budget-Store: Eintrag "${entry.id}" benötigt tolerance_ratio (Zahl).`);
    }
    if (typeof entry.gate !== "boolean") {
      schemaError(`Budget-Store: Eintrag "${entry.id}" benötigt gate (true/false).`);
    }
    if (typeof entry.note !== "string") {
      schemaError(`Budget-Store: Eintrag "${entry.id}" benötigt note (Text).`);
    }
  }
}

function findCriterionEstimates(dir) {
  const found = [];
  const walk = (current) => {
    let entries;
    try {
      entries = readdirSync(current, { withFileTypes: true });
    } catch {
      // Directory does not exist yet (no benchmark runs so far).
      return;
    }
    for (const entry of entries) {
      const full = join(current, entry.name);
      if (entry.isDirectory()) {
        walk(full);
      } else if (entry.name === "estimates.json") {
        found.push(full);
      }
    }
  };
  walk(dir);
  return found;
}

function modeDescription(mode) {
  switch (mode) {
    case "report":
      return "Immer ausführen; erzeugt das Report-Artefakt. Exit 0 unabhängig von Überschreitungen.";
    case "warn":
      return "Standard; budgetierte Überschreitung erzeugt eine Warnung, Exit bleibt 0.";
    case "gate":
      return "Nur Benchmarks mit gate:true; Verletzung führt zu Exit 1.";
    default:
      return "(unbekannter Modus)";
  }
}

function main() {
  const opts = parseArgs(process.argv.slice(2));
  if (opts.help) {
    printUsage();
    process.exit(EXIT_OK);
  }

  // (a) Load and schema-validate both stores.
  const baseline = loadStore(opts.baseline, "Baseline");
  const budgets = loadStore(opts.budgets, "Budgets");
  validateBaseline(baseline);
  validateBudgets(budgets);

  // (b) Scan the Criterion output directory for estimates.json.
  const estimates = findCriterionEstimates(opts.criterionDir);

  // (c) No measurement data yet: report clearly, exit 0.
  if (estimates.length === 0) {
    console.log("Keine Messdaten vorhanden – F-074-N3 ausstehend.");
    console.log(`(Criterion-Verzeichnis durchsucht: ${opts.criterionDir})`);
    console.log("Exit 0 – es gibt nichts zu vergleichen.");
    process.exit(EXIT_OK);
  }

  // (d) Measurement data present: print mode semantics and placeholders.
  console.log(
    `Messdaten gefunden: ${estimates.length} estimates.json-Datei(en) unter ${opts.criterionDir}.`,
  );
  console.log(`Modus: ${opts.mode}`);
  console.log(modeDescription(opts.mode));
  console.log("");

  if (baseline.benchmarks.length === 0) {
    console.log(
      "Hinweis: Baseline-Store enthält noch keine Messwerte (perf/baseline.json ist leer).",
    );
    console.log("Ohne Baseline gibt es keinen stillen Fallback – die IDs müssen erst erfasst werden.");
    console.log("");
  }
  if (budgets.budgets.length === 0) {
    console.log(
      "Hinweis: Budget-Store enthält noch keine Einträge (perf/budgets.json ist leer).",
    );
    console.log("");
  }

  // ---------------------------------------------------------------------------
  // PLATZHALTER F-074-N5: Vergleichslogik
  // ---------------------------------------------------------------------------
  // An dieser Stelle vergleicht F-074-N5 für jede gefundene estimates.json
  // den Median (point_estimate) mit dem Baseline-Eintrag (median_ns) und dem
  // Budget (budget_ns * tolerance_ratio) der zur ID gehörenden Registrierung:
  //
  //   - report:  immer ausführen, Exit 0 (Report-Artefakt erzeugen);
  //   - warn:    Überschreitung von Baseline/Budget melden, Exit bleibt 0;
  //   - gate:    nur Budgets mit gate:true prüfen, Verletzung => Exit 1.
  //
  // Die Zuordnung estimates.json -> Benchmark-ID folgt dem Criterion-
  // Verzeichnislayout (`<group>/<function>/<fixture>/estimates.json`).
  // Fehlende Baseline-/Budget-Einträge werden gemeldet, nicht still ersetzt.
  // ---------------------------------------------------------------------------
  console.log("PLATZHALTER: Vergleichslogik wird in F-074-N5 implementiert.");
  console.log("In diesem Gerüst-Stand endet das Skript ohne Verletzung (Exit 0).");

  process.exit(EXIT_OK);
}

main();

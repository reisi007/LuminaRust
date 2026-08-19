#!/usr/bin/env node
"use strict";

/**
 * compare.mjs — Criterion-Vergleich gegen Baseline- und Budget-Stores.
 *
 * Implementiert (F-074-N5, 2026-08-19):
 *   - lädt und schema-validiert beide Stores,
 *   - scannt --criterion-dir nach Criterion-estimates.json-Dateien,
 *   - vergleicht den gemessenen Median (point_estimate) mit dem
 *     Baseline-Eintrag (median_ns) je Benchmark-ID,
 *   - kennt drei Modi: report (immer, Exit 0, Report-Artefakt),
 *     warn (budgetierte Überschreitung = Warnung, Exit 0) und
 *     gate (nur gate:true-Benchmarks, Exit 1 bei Verletzung).
 * Fehlende Baseline wird gemeldet, niemals still ersetzt.
 *
 * Methodology: feature/quality/performance-benchmarks.md (F-074)
 * Decision:    docs/adr/0003-performance-benchmarking.md (ADR 0003)
 *
 * Exit codes:
 *   0  ok (no violation, or no measurement data available)
 *   1  violation (baseline/budget exceeded; gate mode)
 *   2  schema or usage error
 */

import {
  readFileSync,
  readdirSync,
  existsSync,
  mkdirSync,
  writeFileSync,
} from "node:fs";
import { join, dirname, basename } from "node:path";

// Toleranz, die genutzt wird, wenn ein gemessener Benchmark keinen
// Budget-Eintrag besitzt (informative Status-Spalte im report-Modus).
const DEFAULT_TOLERANCE = 1.2;

const EXIT_OK = 0;
const EXIT_VIOLATION = 1;
const EXIT_ERROR = 2;

const MODES = new Set(["report", "warn", "gate"]);

const DEFAULTS = {
  mode: "report",
  baseline: "perf/baseline.json",
  budgets: "perf/budgets.json",
  criterionDir: "target/criterion",
  reportDir: "perf/results",
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
  --report-dir <pfad>         Verzeichnis für das Report-Artefakt (Standard: ${DEFAULTS.reportDir})
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
      case "--report-dir": {
        const { value, next } = takeValue("report-dir", inline, i);
        opts.reportDir = value;
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

/**
 * Extrahiert aus einem estimates.json-Pfad die Benchmark-ID und die
 * Criterion-Laufvariante. Layout:
 *   <criterion-dir>/<group>/<id>/<variant>/estimates.json
 * Die Store-ID ist "<group>/<id>" (z. B. "core/render_frame__2048").
 * variant ist "new" oder "base".
 */
function parseEstimatePath(estimatesPath) {
  const dir = dirname(estimatesPath);
  const variant = basename(dir); // "new" | "base"
  const idDir = dirname(dir);
  const id = basename(idDir);
  const group = basename(dirname(idDir));
  return { storeId: `${group}/${id}`, variant, idDir };
}

/**
 * Liest den Median (point_estimate, Nanosekunden) aus einer estimates.json.
 * Gibt `undefined` zurück, wenn die Datei oder das Feld fehlt/beschädigt ist.
 */
function readEstimateMedian(estimatesPath) {
  let raw;
  try {
    raw = readFileSync(estimatesPath, "utf8");
  } catch {
    return undefined;
  }
  let json;
  try {
    json = JSON.parse(raw);
  } catch {
    return undefined;
  }
  const median = json?.median?.point_estimate;
  return typeof median === "number" ? median : undefined;
}

/**
 * Sammelt alle entdeckten Messungen. Bei doppelten IDs (sowohl "new" als auch
 * "base" vorhanden) wird die "new"-Variante bevorzugt.
 */
function collectMeasures(estimatePaths) {
  const byId = new Map();
  for (const path of estimatePaths) {
    const parsed = parseEstimatePath(path);
    const median = readEstimateMedian(path);
    if (median === undefined) {
      continue;
    }
    const existing = byId.get(parsed.storeId);
    if (!existing || parsed.variant === "new") {
      byId.set(parsed.storeId, {
        storeId: parsed.storeId,
        median,
        variant: parsed.variant,
        path,
      });
    }
  }
  return [...byId.values()];
}

function formatNs(ns) {
  if (ns === null || ns === undefined) {
    return "—";
  }
  return String(Math.round(ns));
}

function buildReportArtifact(reportDir, mode, rows, environment, meta) {
  if (!reportDir) {
    return;
  }
  try {
    mkdirSync(reportDir, { recursive: true });
  } catch {
    // Schreiben ist optional – ein fehlendes Verzeichnis bricht nicht den Lauf.
  }
  const lines = [];
  lines.push("# LuminaRust Performance-Report");
  lines.push("");
  lines.push(`- **Modus:** ${mode}`);
  lines.push(`- **Erzeugt:** ${new Date().toISOString()}`);
  lines.push(`- **Criterion-Verzeichnis:** ${meta.criterionDir}`);
  if (environment) {
    const e = environment;
    lines.push(
      `- **Umgebung:** ${e.host ?? "?"} / ${e.os ?? "?"} / ${e.arch ?? "?"} / ` +
        `rustc ${e.rustc ?? "?"} / libraw ${e.libraw ?? "?"} / erfasst ${e.recorded_at ?? "?"}`,
    );
  }
  if (meta.noData) {
    lines.push("");
    lines.push("Keine Messdaten vorhanden – es gibt nichts zu vergleichen.");
    lines.push("");
    lines.push("Exit 0.");
  } else {
    const total = rows.length;
    const overshoots = rows.filter((r) => r.status === "ÜBERSCHRITTEN").length;
    const violations = rows.filter((r) => r.status === "VERLETZUNG").length;
    const missing = rows.filter((r) => r.status === "KEINE BASELINE").length;
    lines.push("");
    lines.push(
      `Zusammenfassung: ${total} Benchmark(s), ${overshoots} Überschreitung(en), ` +
        `${violations} Verletzung(en), ${missing} ohne Baseline.`,
    );
    lines.push("");
    lines.push("| Benchmark-ID | gemessen (ns) | Baseline (ns) | Delta | Status |");
    lines.push("| --- | ---: | ---: | ---: | --- |");
    for (const r of rows) {
      const delta =
        r.deltaPct === null || r.deltaPct === undefined
          ? "—"
          : `${r.deltaPct >= 0 ? "+" : ""}${r.deltaPct.toFixed(1)}%`;
      lines.push(
        `| ${r.id} | ${formatNs(r.measured)} | ${formatNs(r.baseline)} | ${delta} | ${r.status} |`,
      );
    }
  }
  const file = join(reportDir, "perf-report.md");
  try {
    writeFileSync(file, lines.join("\n") + "\n", "utf8");
    console.log(`\nReport-Artefakt geschrieben: ${file}`);
  } catch (err) {
    console.log(
      `\nHinweis: Report-Artefakt konnte nicht geschrieben werden: ${err.message}`,
    );
  }
}

function printTable(rows) {
  if (rows.length === 0) {
    return;
  }
  const idW = Math.max(20, ...rows.map((r) => r.id.length));
  const num = (n) => (n === null || n === undefined ? "—" : String(Math.round(n)));
  const delta = (r) =>
    r.deltaPct === null || r.deltaPct === undefined
      ? "—"
      : `${r.deltaPct >= 0 ? "+" : ""}${r.deltaPct.toFixed(1)}%`;
  const head = `Benchmark-ID${" ".repeat(idW - 13)} | gemessen (ns) | Baseline (ns) | Delta    | Status`;
  const sep = `${"-".repeat(idW)}-|${"-".repeat(14)}|${"-".repeat(14)}|${"-".repeat(10)}|${"-".repeat(20)}`;
  console.log("");
  console.log(head);
  console.log(sep);
  for (const r of rows) {
    const id = r.id.padEnd(idW);
    const m = num(r.measured).padStart(14);
    const b = num(r.baseline).padStart(14);
    const d = delta(r).padStart(10);
    console.log(`${id} | ${m} | ${b} | ${d} | ${r.status}`);
  }
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

  // (c) No measurement data yet: report clearly, write artifact, exit 0.
  if (estimates.length === 0) {
    console.log("Keine Messdaten vorhanden – F-074-N3 ausstehend.");
    console.log(`(Criterion-Verzeichnis durchsucht: ${opts.criterionDir})`);
    buildReportArtifact(opts.reportDir, opts.mode, [], baseline.environment, {
      noData: true,
      criterionDir: opts.criterionDir,
    });
    console.log("Exit 0 – es gibt nichts zu vergleichen.");
    process.exit(EXIT_OK);
  }

  // (d) Measurement data present. Index stores by ID and collect measures.
  const baselineById = new Map(baseline.benchmarks.map((e) => [e.id, e]));
  const budgetById = new Map(budgets.budgets.map((e) => [e.id, e]));
  const measures = collectMeasures(estimates);

  console.log(
    `Messdaten gefunden: ${estimates.length} estimates.json-Datei(en) unter ${opts.criterionDir}.`,
  );
  console.log(`Modus: ${opts.mode}`);
  console.log(modeDescription(opts.mode));
  console.log("");

  const { rows, violation } = buildComparison(
    opts.mode,
    measures,
    baselineById,
    budgetById,
  );

  printTable(rows);
  buildReportArtifact(opts.reportDir, opts.mode, rows, baseline.environment, {
    criterionDir: opts.criterionDir,
  });

  if (opts.mode === "gate") {
    if (violation) {
      console.log("\nREGRESSIONS-GATE: Verletzung(en) erkannt – Exit 1.");
      process.exit(EXIT_VIOLATION);
    }
    console.log("\nREGRESSIONS-GATE: alle geprüften Budgets bestanden – Exit 0.");
    process.exit(EXIT_OK);
  }

  // report / warn: niemals ein harter Abbruch (Exit 0).
  if (opts.mode === "warn") {
    const warnings = rows.filter((r) => r.status === "ÜBERSCHRITTEN").length;
    console.log(
      `\nwarn-Modus: ${warnings} budgetierte Überschreitung(en) als Warnung gemeldet – Exit 0.`,
    );
  }
  process.exit(EXIT_OK);
}

/**
 * Führt den Modus-spezifischen Vergleich durch und liefert die Tabellenzeilen
 * sowie ein Violation-Flag (nur im gate-Modus relevant).
 *
 * Schwellenwert: gemessener Median > Baseline.median_ns * tolerance_ratio.
 * Fehlt ein Baseline-Eintrag, wird dies ausdrücklich gemeldet – es gibt
 * keinen stillen Fallback (kein erfundener Vergleichswert).
 *
 * - report: alle gemessenen Benchmarks, Exit 0.
 * - warn:   wie report; eine budgetierte Überschreitung erzeugt eine
 *           WARNUNG (nur für IDs mit Budget-Eintrag), Exit bleibt 0.
 * - gate:   nur Budgets mit gate:true; jede Verletzung (Überschreitung oder
 *           fehlende Baseline) führt zu violation = true (Exit 1).
 */
function buildComparison(mode, measures, baselineById, budgetById) {
  const rows = [];
  let violation = false;

  if (mode === "gate") {
    for (const bud of budgetById.values()) {
      if (!bud.gate) {
        continue;
      }
      const base = baselineById.get(bud.id);
      const meas = measures.find((m) => m.storeId === bud.id);
      if (!base) {
        rows.push({
          id: bud.id,
          measured: meas ? meas.median : null,
          baseline: null,
          deltaPct: null,
          status: "KEINE BASELINE",
        });
        console.log(
          `VERLETZUNG (gate): keine Baseline für ${bud.id} – kein stiller Fallback.`,
        );
        violation = true;
        continue;
      }
      if (!meas) {
        rows.push({
          id: bud.id,
          measured: null,
          baseline: base.median_ns,
          deltaPct: null,
          status: "NICHT GEMESSEN",
        });
        console.log(
          `Hinweis (gate): ${bud.id} nicht im Messlauf – für das Gate übersprungen.`,
        );
        continue;
      }
      const threshold = base.median_ns * bud.tolerance_ratio;
      const deltaPct = ((meas.median - base.median_ns) / base.median_ns) * 100;
      if (meas.median > threshold) {
        rows.push({
          id: bud.id,
          measured: meas.median,
          baseline: base.median_ns,
          deltaPct,
          status: "VERLETZUNG",
        });
        console.log(
          `VERLETZUNG (gate): ${bud.id} ${Math.round(meas.median)} ns > ` +
            `Baseline ${base.median_ns} ns * ${bud.tolerance_ratio} ` +
            `= ${Math.round(threshold)} ns (+${deltaPct.toFixed(1)}%).`,
        );
        violation = true;
      } else {
        rows.push({
          id: bud.id,
          measured: meas.median,
          baseline: base.median_ns,
          deltaPct,
          status: "OK (gate)",
        });
      }
    }
    return { rows, violation };
  }

  // report / warn: alle gemessenen Benchmarks auswerten.
  for (const m of measures) {
    const base = baselineById.get(m.storeId);
    const bud = budgetById.get(m.storeId);
    if (!base) {
      rows.push({
        id: m.storeId,
        measured: m.median,
        baseline: null,
        deltaPct: null,
        status: "KEINE BASELINE",
      });
      console.log(
        `Hinweis: keine Baseline für ${m.storeId} (kein stiller Fallback).`,
      );
      continue;
    }
    const tol = bud ? bud.tolerance_ratio : DEFAULT_TOLERANCE;
    const threshold = base.median_ns * tol;
    const deltaPct = ((m.median - base.median_ns) / base.median_ns) * 100;
    if (m.median > threshold) {
      if (mode === "warn" && bud) {
        console.log(
          `WARNUNG: ${m.storeId} überschreitet Baseline um ${deltaPct.toFixed(1)}% ` +
            `(${Math.round(m.median)} ns > ${base.median_ns} ns * ${tol}).`,
        );
      }
      rows.push({
        id: m.storeId,
        measured: m.median,
        baseline: base.median_ns,
        deltaPct,
        status: "ÜBERSCHRITTEN",
      });
    } else {
      rows.push({
        id: m.storeId,
        measured: m.median,
        baseline: base.median_ns,
        deltaPct,
        status: "OK",
      });
    }
  }
  return { rows, violation };
}

main();

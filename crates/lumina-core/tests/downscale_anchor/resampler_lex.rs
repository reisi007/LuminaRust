//! RESAMPLER-GATE-SCOPE-78 — das **Lexikon** des Quelltext-Scanners.
//!
//! Der Scanner besteht aus zwei Teilen: diesem Lexikon (die Frage
//! „ist diese Zeile eine Funktionsdefinition, und wie lautet ihre Signatur?“)
//! und dem Workspace-Scan in `resampler_scan.rs` (die Frage „welche Dateien
//! werden ueberhaupt gelesen?“).
//!
//! Das Lexikon arbeitet **zeilenweise** und kennt genau eine Form einer
//! Definition: `fn <name>`. Es unterscheidet dabei nicht zwischen Code und
//! dem **Inhalt eines Raw-Strings** — beides wird zeilenweise gelesen. Der
//! Gate-Test schliesst seine eigene Datei deshalb aus (siehe
//! `GATE_OWN_FILES`).

/// Die Modifikator-Keywords, die vor `fn` stehen duerfen.
///
/// `pub` und `extern` sind hier **nicht** enthalten, weil sie eine eigene
/// Form haben:
/// * `pub` kann einen Sichtbarkeitspfad in Klammern tragen: `pub(crate)`,
///   `pub(super)`, `pub(self)` und `pub(in crate::a::b)`. Der Pfad zaehlt
///   dabei als **eine** Angabe — `pub(in crate::a::b)` ist ein einziger
///   Modifikator, kein `pub` + `in` + `crate` + `a` + `b`.
/// * `extern` kann ein String-Literal tragen: `extern "C" fn …`.
///
/// Beide Formen werden in [`prefix_is_modifiers_only`] eigens behandelt.
const DEFINITION_MODIFIERS: [&str; 4] = ["const", "async", "unsafe", "default"];

/// Maximale Zeilenzahl, die fuer eine Signatur gelesen wird.
const SIGNATURE_MAX_LINES: usize = 40;

/// Die Identifier (Wortzeichen) eines Textes mit ihren Byte-Positionen.
pub fn tokens_with_offsets(text: &str) -> Vec<(String, usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (index, ch) in text.char_indices() {
        if ch.is_alphanumeric() || ch == '_' {
            if start.is_none() {
                start = Some(index);
            }
        } else if let Some(begin) = start {
            out.push((text[begin..index].to_string(), begin, index));
            start = None;
        }
    }
    if let Some(begin) = start {
        out.push((text[begin..].to_string(), begin, text.len()));
    }
    out
}

/// Die Byte-Bereiche der `#[...]`-Attribute einer Zeile. Token in diesen
/// Bereichen gehoeren zum Attribut, nicht zur Definition.
fn attribute_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'#' && index + 1 < bytes.len() && bytes[index + 1] == b'[' {
            if let Some(close) = text[index + 2..].find(']') {
                let end = index + 2 + close + 1;
                out.push((index, end));
                index = end;
                continue;
            }
        }
        index += 1;
    }
    out
}

/// `keyword` als **eigenes** Token: der Rest darf nicht mit einem
/// Wortzeichen beginnen (sonst waere `const` in `constant` ein Treffer).
fn strip_keyword<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let after = text.strip_prefix(keyword)?;
    if after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return None;
    }
    Some(after)
}

/// Enthaelt der Text vor `fn` **ausschliesslich** Modifikatoren?
///
/// Erlaubt sind (in beliebiger Reihenfolge, beliebig oft):
///
/// * `pub` — optional mit einem Sichtbarkeitspfad in Klammern: `pub`,
///   `pub(crate)`, `pub(super)`, `pub(self)`, `pub(in crate::a::b)`.
///   **Der Pfad in Klammern zaehlt als eine Angabe**, nicht als mehrere
///   Tokens — das ist der Unterschied zur reinen Token-Pruefung.
/// * `const`, `async`, `unsafe`, `default` (jeweils als eigenes Token).
/// * `extern` — optional mit einem String-Literal: `extern "C" fn …`.
///
/// Alles andere (z. B. `let`, ein Typ, ein Attribut) wird abgewiesen.
fn prefix_is_modifiers_only(prefix: &str) -> bool {
    let mut rest = prefix.trim();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return true;
        }
        if let Some(after) = rest.strip_prefix("pub") {
            let after = after.trim_start();
            match after.strip_prefix('(') {
                Some(inner) => match inner.find(')') {
                    Some(end) => rest = &inner[end + 1..],
                    // Eine unbalancierte Klammer ist kein gueltiger Modifikator.
                    None => return false,
                },
                None => rest = after,
            }
            continue;
        }
        if let Some(after) = strip_keyword(rest, "extern") {
            let after = after.trim_start();
            match after.strip_prefix('"') {
                Some(inner) => match inner.find('"') {
                    Some(end) => rest = &inner[end + 1..],
                    // Ein unbalanciertes String-Literal ist kein Modifikator.
                    None => return false,
                },
                None => rest = after,
            }
            continue;
        }
        let mut matched = false;
        for keyword in DEFINITION_MODIFIERS {
            if let Some(after) = strip_keyword(rest, keyword) {
                rest = after;
                matched = true;
                break;
            }
        }
        if !matched {
            return false;
        }
    }
}

/// Der Text vor `fn`, ohne die Attribut-Bereiche.
fn prefix_without_attributes(trimmed: &str, attributes: &[(usize, usize)], upto: usize) -> String {
    let mut out = String::new();
    let mut cursor = 0;
    while cursor < upto {
        match attributes
            .iter()
            .find(|(begin, _)| *begin >= cursor && *begin < upto)
        {
            Some((begin, end)) => {
                out.push_str(&trimmed[cursor..*begin]);
                cursor = (*end).min(upto);
            }
            None => {
                out.push_str(&trimmed[cursor..upto]);
                cursor = upto;
            }
        }
    }
    out
}

/// Der Name der Definition, die diese Zeile eroeffnet, und die Byte-Position
/// der oeffnenden Parameterliste (`None`, wenn die Zeile keine Definition
/// eroeffnet).
pub fn definition_at(line: &str) -> Option<(String, usize)> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Kommentar- und String-Literal-Zeilen sind keine Definitionen.
    if trimmed.starts_with("//")
        || trimmed.starts_with('"')
        || trimmed.starts_with('\'')
        || trimmed.starts_with("r\"")
        || trimmed.starts_with("r#")
    {
        return None;
    }
    let attributes = attribute_ranges(trimmed);
    let tokens = tokens_with_offsets(trimmed);
    let in_attribute = |start: usize| {
        attributes
            .iter()
            .any(|(begin, end)| start >= *begin && start < *end)
    };
    let fn_index = tokens
        .iter()
        .position(|(token, start, _)| token == "fn" && !in_attribute(*start))?;
    // Der Text vor `fn` muss **nur** Modifikatoren enthalten. Geprueft wird
    // der **Text**, nicht die Token-Liste: nur so zaehlt `pub(in crate::x)`
    // als eine Angabe.
    let prefix = prefix_without_attributes(trimmed, &attributes, tokens[fn_index].1);
    if !prefix_is_modifiers_only(&prefix) {
        return None;
    }
    let (name, _, name_end) = tokens.get(fn_index + 1)?.clone();
    let open = trimmed[name_end..].find('(')? + name_end;
    Some((name, open))
}

/// Der Typname eines `impl`-Blocks (`None`, wenn die Zeile kein `impl` ist).
pub fn impl_header(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("impl")?;
    if rest.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let mut rest = rest.trim_start();
    if rest.starts_with('<') {
        let mut depth = 0usize;
        for (index, ch) in rest.char_indices() {
            match ch {
                '<' => depth += 1,
                '>' => {
                    depth -= 1;
                    if depth == 0 {
                        rest = rest[index + 1..].trim_start();
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    // `impl Trait for Type` — der Typ steht hinter `for`.
    if let Some(position) = rest.find(" for ") {
        rest = rest[position + 5..].trim_start();
    }
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == ':')
        .collect();
    let name = name.rsplit("::").next().unwrap_or("").to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Die Signatur einer Definition: (Parameter, Rueckgabetyp).
///
/// Die Parameterliste kann ueber mehrere Zeilen laufen; deshalb wird
/// zeilenweise weitergelesen, bis die Klammer geschlossen ist.
pub fn signature_of(
    lines: &[&str],
    first_line: usize,
    text: &str,
    open: usize,
) -> (String, String) {
    let mut buffer = text.to_string();
    let mut cursor = first_line;
    let mut scanned = open;
    let mut depth = 0i32;
    let mut params_end: Option<usize> = None;
    loop {
        let mut found = false;
        for (index, ch) in buffer[scanned..].char_indices() {
            let position = scanned + index;
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        params_end = Some(position);
                        found = true;
                        break;
                    }
                }
                _ => {}
            }
        }
        if found {
            break;
        }
        if cursor + 1 >= lines.len() || cursor + 1 - first_line >= SIGNATURE_MAX_LINES {
            break;
        }
        cursor += 1;
        buffer.push(' ');
        scanned = buffer.len();
        buffer.push_str(lines[cursor]);
    }
    let Some(end) = params_end else {
        return (String::new(), String::new());
    };
    let params = buffer[open + 1..end].to_string();
    // Der Rueckgabetyp: bis zum Koerper-`{` (Ebene 0) bzw. bis zum `;`
    // (Deklaration ohne Rumpf, z. B. in einem Trait).
    let mut returns = String::new();
    let mut brace_depth = 0i32;
    let mut scanned = end + 1;
    loop {
        let mut terminated = false;
        for ch in buffer[scanned..].chars() {
            match ch {
                '{' => {
                    if brace_depth == 0 {
                        terminated = true;
                        break;
                    }
                    brace_depth += 1;
                }
                '}' => brace_depth = (brace_depth - 1).max(0),
                ';' if brace_depth == 0 => {
                    terminated = true;
                    break;
                }
                _ => {}
            }
            returns.push(ch);
        }
        if terminated || cursor + 1 >= lines.len() {
            break;
        }
        cursor += 1;
        buffer.push(' ');
        scanned = buffer.len();
        buffer.push_str(lines[cursor]);
    }
    (params, returns)
}

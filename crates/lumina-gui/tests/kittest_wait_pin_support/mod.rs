//! The source scanner behind `kittest_wait_pin` (KITT-WAIT-PIN-57).
//!
//! It answers one question about a Rust source: *does it contain a hand-written
//! wait?* — a loop that pumps the harness frame loop while its exit depends on
//! state read from the app. Everything here is textual on purpose: the pin has
//! to work on sources it does not compile, so a suite nobody builds in this run
//! is still covered, and so the answer cannot be changed by a `mod` declaration.
//!
//! What it does **not** decide is whether a found wait may stay: that is the
//! caller's rule, expressed once in `kittest_wait_pin.rs`. The scanner finds
//! shapes; the target names the places they may live.

use std::fs;
use std::path::{Path, PathBuf};

/// The harness frame step, receiver-agnostic: the binding is `harness`, `vram`
/// or `cpu`, and the wait is the *method*, not the receiver.
const STEP_CALLS: [&str; 3] = ["run_steps(", ".run(", ".step("];

/// A hand-written wait loop found in a crate source.
#[derive(Debug, Clone)]
pub struct WaitLoop {
    /// Path relative to the crate root.
    pub file: String,
    pub line: usize,
    /// `for`, `while` or `loop`.
    pub kind: &'static str,
    /// The loop head, whitespace-collapsed: what it iterates or waits on.
    pub condition: String,
    /// The `fn` the loop sits in.
    pub function: String,
    /// How many harness frame steps the body performs.
    pub steps: usize,
}

/// Why a loop that steps the harness is — or is not — a hand-written wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PumpForm {
    /// The loop's exit reads app state: a hand-written wait.
    Wait,
    /// `for _ in 0..N`, no app-state exit. Not a wait — a gesture or a fixed
    /// layout pump — but pinned by name so the class stays visible and cannot
    /// grow unnoticed.
    FrameBudget,
}

/// Every loop that steps the harness frame loop, classified.
///
/// The allowed modules are not filtered here: their one loop has to stay visible
/// to the scanner, otherwise the scanner is blind and a "clean" result proves
/// nothing. Which found loops may stay is the caller's decision.
pub fn pumps(file: &str, code: &str) -> Vec<(WaitLoop, PumpForm)> {
    let walk = walk(code);
    let mut out = Vec::new();
    for candidate in loop_candidates(code) {
        let Some(body_end) = matching_brace(code, candidate.body_brace) else {
            continue;
        };
        let body = &code[candidate.body_brace..body_end];
        let steps = step_calls(body);
        if steps == 0 {
            // Not a frame pump: a worker wait, a data loop, a gesture.
            continue;
        }
        let Some(&(_, body_index)) = walk
            .brace_index
            .iter()
            .find(|(position, _)| *position == candidate.body_brace)
        else {
            continue;
        };
        // A `while` loop's exit *is* its condition, so it carries no
        // break/return: the predicate sits in the loop head.
        let exits_on_app_state = candidate.kind == "while"
            || exits_the_loop(&walk, candidate.body_brace, body_end, body_index);
        let form = if exits_on_app_state {
            PumpForm::Wait
        } else if candidate.kind == "for" && zero_start_range(&candidate.condition) {
            PumpForm::FrameBudget
        } else {
            // A collection iteration that steps the harness once per member.
            continue;
        };
        out.push((
            WaitLoop {
                file: file.to_owned(),
                line: code[..candidate.keyword_position].matches('\n').count() + 1,
                kind: candidate.kind,
                condition: candidate.condition,
                function: enclosing_function(code, candidate.keyword_position),
                steps,
            },
            form,
        ));
    }
    out
}

/// Every `.rs` file under `root`, discovered — never listed.
pub fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_rust_files(root, &mut out);
    out.sort();
    out
}

fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // `target/` is build output. `snapshots/` and `fixtures/` hold
            // goldens and bytes, not sources; a directory without `.rs` files
            // is skipped anyway, so naming the one that must never be walked
            // is enough.
            let is_build_output = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == "target");
            if !is_build_output {
                collect_rust_files(&path, out);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Whether a `for`-loop header iterates a zero-anchored range, e.g.
/// `for _ in 0..N`, `for _i in 0..N` or `for _i in (0..N)`: the token after
/// `in` only has to *contain* `0..` — bare, parenthesised (`rustfmt` keeps
/// those parentheses) and inclusive `0..=N` all count, so no `_i`-spelled
/// binding evades the pin: a blind frame budget, not a collection iteration.
/// Deliberate boundary: `for _i in 1..N` (not starting at zero) is no budget.
pub fn zero_start_range(condition: &str) -> bool {
    let tokens: Vec<&str> = condition.split_whitespace().collect();
    for pair in tokens.windows(2) {
        if pair[0] == "in" && pair[1].contains("0..") {
            return true;
        }
    }
    false
}

/// `path` relative to `root`, with `/` separators.
pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// How many harness frame steps `region` performs.
pub fn step_calls(region: &str) -> usize {
    STEP_CALLS
        .iter()
        .map(|call| region.matches(call).count())
        .sum()
}

/// Blank comments, string literals and char literals, keeping every byte
/// position and every newline so line numbers stay true. Without this the rule
/// would fire on prose: the shared wait's module docs quote the loops it
/// replaced, and the pin's own docs quote the rule.
pub fn strip_noise(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                out.push(b' ');
                i += 1;
            }
            continue;
        }
        if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            out.push(b' ');
            out.push(b' ');
            i += 2;
            let mut depth = 1usize;
            while i < bytes.len() && depth > 0 {
                if bytes[i] == b'/' && bytes.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                } else if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                } else {
                    out.push(blank_of(bytes[i]));
                    i += 1;
                }
            }
            continue;
        }
        // Raw string: `r"…"`, `r#"…"#`, `br#"…"#`. The lensfun XML fixtures
        // carry `"` and `{` inside, so they must be blanked as a whole.
        if c == b'r' || (c == b'b' && bytes.get(i + 1) == Some(&b'r')) {
            let mut j = i + 1;
            if c == b'b' {
                j += 1;
            }
            let mut hashes = 0usize;
            while bytes.get(j) == Some(&b'#') {
                hashes += 1;
                j += 1;
            }
            if bytes.get(j) == Some(&b'"') {
                j += 1;
                while j < bytes.len() {
                    if bytes[j] == b'"' && hashes_at(bytes, j + 1, hashes) {
                        j += 1 + hashes;
                        break;
                    }
                    j += 1;
                }
                while i < j.min(bytes.len()) {
                    out.push(blank_of(bytes[i]));
                    i += 1;
                }
                continue;
            }
        }
        if c == b'"' {
            out.push(b' ');
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' {
                    out.push(b' ');
                    out.push(b' ');
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    out.push(b' ');
                    i += 1;
                    break;
                }
                out.push(blank_of(bytes[i]));
                i += 1;
            }
            continue;
        }
        // Char or byte literal — but never a lifetime (`'a`).
        if c == b'\'' || (c == b'b' && bytes.get(i + 1) == Some(&b'\'')) {
            if let Some(end) = char_literal_end(bytes, i) {
                while i < end {
                    out.push(b' ');
                    i += 1;
                }
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8(out).expect("blanking replaces whole bytes with spaces")
}

/// The blank that keeps the byte count and the line count.
fn blank_of(byte: u8) -> u8 {
    if byte == b'\n' {
        b'\n'
    } else {
        b' '
    }
}

/// Are `count` `#` bytes at `from`?
fn hashes_at(bytes: &[u8], from: usize, count: usize) -> bool {
    from + count <= bytes.len() && bytes[from..from + count].iter().all(|b| *b == b'#')
}

/// End (exclusive) of a char literal at `start`, or `None` when the quote is a
/// lifetime.
fn char_literal_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start;
    if bytes[i] == b'b' {
        i += 1;
    }
    if bytes.get(i) != Some(&b'\'') {
        return None;
    }
    let content = i + 1;
    if bytes.get(content) == Some(&b'\\') {
        let mut j = content + 1;
        while j < bytes.len() && bytes[j] != b'\'' {
            j += 1;
        }
        return Some((j + 1).min(bytes.len()));
    }
    // `'x'` is a literal; `'a` followed by anything else is a lifetime.
    if bytes.get(content + 1) == Some(&b'\'') {
        return Some(content + 2);
    }
    None
}

/// One open `{`, classified while the source is walked.
struct Block {
    is_loop: bool,
    is_closure: bool,
}

/// The walk's result: which stack index each `{` got, and the `break`/`return`
/// tokens that leave the innermost loop around them.
struct Walk {
    brace_index: Vec<(usize, usize)>,
    exits: Vec<(usize, usize, Option<usize>)>,
}

/// A candidate loop: the keyword, its head, and the `{` that opens its body.
struct Candidate {
    kind: &'static str,
    condition: String,
    keyword_position: usize,
    body_brace: usize,
}

/// Every `for`/`while`/`loop` whose body brace can be identified. A head
/// containing `|` is skipped: there a closure's `{` would be mistaken for the
/// body (`for x in it.map(|a| { … })`). None exists in this crate today, and
/// skipping is the conservative direction.
fn loop_candidates(code: &str) -> Vec<Candidate> {
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if is_ident_start(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_ident_continue(bytes[i]) {
                i += 1;
            }
            let kind = match &code[start..i] {
                "for" => "for",
                "while" => "while",
                "loop" => "loop",
                _ => continue,
            };
            let Some(brace) = code[i..].find('{').map(|offset| i + offset) else {
                continue;
            };
            // A `|` in the head means a closure's `{` comes first.
            let head = &code[i..brace];
            if head.contains('|') {
                continue;
            }
            out.push(Candidate {
                kind,
                condition: head.split_whitespace().collect::<Vec<_>>().join(" "),
                keyword_position: start,
                body_brace: brace,
            });
            continue;
        }
        i += 1;
    }
    out
}

/// Position just past the `}` matching the `{` at `open`.
fn matching_brace(code: &str, open: usize) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Walk the source once, recording each brace's stack index and the `break`/
/// `return` tokens that leave the innermost loop around them.
fn walk(code: &str) -> Walk {
    let bytes = code.as_bytes();
    let mut result = Walk {
        brace_index: Vec::new(),
        exits: Vec::new(),
    };
    let mut stack: Vec<Block> = Vec::new();
    let loop_braces: Vec<usize> = loop_candidates(code)
        .into_iter()
        .map(|candidate| candidate.body_brace)
        .collect();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                stack.push(Block {
                    is_loop: loop_braces.contains(&i),
                    is_closure: opens_closure(code, i),
                });
                result.brace_index.push((i, stack.len() - 1));
                i += 1;
            }
            b'}' => {
                stack.pop();
                i += 1;
            }
            _ if is_ident_start(bytes[i]) => {
                let start = i;
                while i < bytes.len() && is_ident_continue(bytes[i]) {
                    i += 1;
                }
                let word = &code[start..i];
                if word == "break" || word == "return" {
                    let innermost_loop = stack.iter().rposition(|block| block.is_loop);
                    let nearest_closure = stack.iter().rposition(|block| block.is_closure);
                    if let Some(loop_index) = innermost_loop {
                        result.exits.push((start, loop_index, nearest_closure));
                    }
                }
            }
            _ => i += 1,
        }
    }
    result
}

/// Does the `{` at `position` open a closure body?
fn opens_closure(code: &str, position: usize) -> bool {
    let trimmed = code[..position].trim_end();
    if trimmed.ends_with('|') {
        return true;
    }
    // `move |a| {`: the token before the brace is the parameter list.
    trimmed.len() > 4
        && trimmed.ends_with("move")
        && trimmed[..trimmed.len() - 4].trim_end().ends_with('|')
}

/// Does `(body_brace, body_end)` contain an exit that leaves **this** loop — not
/// a closure inside it, and not a nested loop's own break?
fn exits_the_loop(walk: &Walk, body_brace: usize, body_end: usize, body_index: usize) -> bool {
    walk.exits.iter().any(|(position, loop_index, closure)| {
        *position > body_brace
            && *position < body_end
            && *loop_index == body_index
            && closure.is_none_or(|index| index < body_index)
    })
}

/// The `fn` a position sits in — the closest one before it, the innermost.
fn enclosing_function(code: &str, position: usize) -> String {
    let head = &code[..position];
    let bytes = head.as_bytes();
    let mut found: Option<String> = None;
    let mut i = 0usize;
    while i < bytes.len() {
        if is_ident_start(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_ident_continue(bytes[i]) {
                i += 1;
            }
            if &head[start..i] == "fn" {
                let mut j = i;
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                let name_start = j;
                while j < bytes.len() && is_ident_continue(bytes[j]) {
                    j += 1;
                }
                if j > name_start {
                    found = Some(head[name_start..j].to_owned());
                }
            }
            continue;
        }
        i += 1;
    }
    found.unwrap_or_else(|| "<free item>".to_owned())
}

fn is_ident_start(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphabetic()
}

fn is_ident_continue(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

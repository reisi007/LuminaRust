function is_excluded(file) {
    return file == root "/scripts/check_golden_serial.sh" ||
           file == root "/scripts/check_golden_serial.awk" ||
           file == root "/scripts/check_golden_serial_test.sh"
}

function is_markdown(file) {
    return file ~ /\.md$/
}

function is_todo(file) {
    return file == root "/Agents.todo.md"
}

function display_path(file) {
    if (index(file, root "/") == 1)
        return substr(file, length(root) + 2)
    return file
}

# Shell comments begin at an unquoted # that starts a token. Check only the
# command portion, so a flag mentioned in a comment cannot satisfy the gate.
function without_shell_comment(text, file,    i, c, quote, escaped) {
    if (file ~ /\.rs$/) return text
    quote = ""
    escaped = 0
    for (i = 1; i <= length(text); i++) {
        c = substr(text, i, 1)
        if (escaped) {
            escaped = 0
            continue
        }
        if (quote == "'") {
            if (c == "'") quote = ""
            continue
        }
        if (quote == "\"") {
            if (c == "\\") escaped = 1
            else if (c == "\"") quote = ""
            continue
        }
        if (c == "'") quote = "'"
        else if (c == "\"") quote = "\""
        else if (c == "\\") escaped = 1
        else if (c == "#" && (i == 1 || substr(text, i - 1, 1) ~ /[ \t]/))
            return substr(text, 1, i - 1)
    }
    return text
}

function is_candidate(text, file,    plain) {
    plain = without_shell_comment(text, file)
    return index(plain, "cargo test") > 0 &&
           index(plain, "-p lumina-gui") > 0 &&
           index(plain, "--ignored") > 0
}

function inspect_invocation(text, file, line, suffix,    plain, marker_text, marker_at, reason, shown) {
    if (is_excluded(file)) return

    plain = without_shell_comment(text, file)
    if (index(plain, "cargo test") == 0 ||
        index(plain, "-p lumina-gui") == 0 ||
        index(plain, "--ignored") == 0)
        return

    invocations++
    shown = display_path(file)
    marker_text = text
    marker_at = index(marker_text, "GOLDEN-SERIAL-HISTORICAL:")
    if (marker_at == 0) {
        marker_text = suffix
        marker_at = index(marker_text, "GOLDEN-SERIAL-HISTORICAL:")
    }
    if (marker_at > 0) {
        reason = substr(marker_text, marker_at + length("GOLDEN-SERIAL-HISTORICAL:"))
        sub(/^[ \t#`]+/, "", reason)
        sub(/[ \t]+$/, "", reason)
        if (length(reason) == 0) {
            violations++
            printf "FEHLER: %s:%d: GOLDEN-SERIAL-HISTORICAL: braucht einen Grund.\n", shown, line > "/dev/stderr"
        } else {
            exemptions++
            printf "historical exemption: %s:%d: GOLDEN-SERIAL-HISTORICAL: %s\n", shown, line, reason
        }
        return
    }

    if (plain !~ /(^|[ \t"'`])--test-threads=1([ \t"'`]|$)/) {
        violations++
        printf "FEHLER: %s:%d: ignorierter lumina-gui cargo test-Aufruf muss literal --test-threads=1 enthalten: %s\n", shown, line, text > "/dev/stderr"
    }
}

# Check each shell command separately: flags in a later &&, ||, ;, or pipe
# segment cannot satisfy the cargo test invocation before it.
function inspect_command(text, file, line, suffix,    i, c, nextc, quote, escaped, start, segment, command_suffix) {
    i = 1
    start = 1
    quote = ""
    escaped = 0
    while (i <= length(text)) {
        c = substr(text, i, 1)
        if (escaped) {
            escaped = 0
            i++
            continue
        }
        if (quote == "'") {
            if (c == "'") quote = ""
            i++
            continue
        }
        if (quote == "\"") {
            if (c == "\\") escaped = 1
            else if (c == "\"") quote = ""
            i++
            continue
        }
        if (c == "'") quote = "'"
        else if (c == "\"") quote = "\""
        else if (c == "\\") escaped = 1
        else if (c == "#" && (i == 1 || substr(text, i - 1, 1) ~ /[ \t]/)) {
            segment = substr(text, start)
            command_suffix = is_candidate(segment, file) ? suffix : ""
            inspect_invocation(segment, file, line, command_suffix)
            return
        } else if (c == ";" || c == "&" || c == "|") {
            segment = substr(text, start, i - start)
            command_suffix = is_candidate(segment, file) ? suffix : ""
            inspect_invocation(segment, file, line, command_suffix)
            nextc = substr(text, i + 1, 1)
            if ((c == "&" && nextc == "&") ||
                (c == "|" && (nextc == "|" || nextc == "&")))
                i += 2
            else
                i++
            start = i
            continue
        }
        i++
    }
    segment = substr(text, start)
    command_suffix = is_candidate(segment, file) ? suffix : ""
    inspect_invocation(segment, file, line, command_suffix)
}

# Extract completed Markdown inline-code spans individually. The suffix up to
# the next code span is considered for a same-command historical marker.
function inspect_markdown(text, file, line,    i, j, run, close_run, close_at, after, next_tick, span, suffix, outside, copy_from, group, group_suffix, have_group, gap) {
    i = 1
    copy_from = 1
    outside = ""
    group = ""
    group_suffix = ""
    have_group = 0
    while (i <= length(text)) {
        if (substr(text, i, 1) != "`") {
            i++
            continue
        }
        run = 1
        while (substr(text, i + run, 1) == "`") run++
        j = i + run
        close_at = 0
        while (j <= length(text)) {
            if (substr(text, j, 1) == "`") {
                close_run = 1
                while (substr(text, j + close_run, 1) == "`") close_run++
                if (close_run == run) {
                    close_at = j
                    break
                }
                j += close_run
            } else {
                j++
            }
        }
        if (close_at == 0) {
            i += run
            continue
        }

        span = substr(text, i + run, close_at - i - run)
        after = close_at + run
        gap = substr(text, copy_from, i - copy_from)
        outside = outside substr(text, copy_from, i - copy_from) " "
        copy_from = after
        next_tick = index(substr(text, after), "`")
        if (next_tick > 0)
            suffix = substr(text, after, next_tick - 1)
        else
            suffix = substr(text, after)
        if (have_group && gap ~ /^[ \t]*$/ &&
            (index(group, "cargo test") > 0 || index(span, "cargo test") > 0)) {
            group = group " " span
            group_suffix = suffix
        } else {
            if (have_group)
                inspect_command(group, file, line, group_suffix)
            group = span
            group_suffix = suffix
            have_group = 1
        }
        i = after
    }
    if (have_group)
        inspect_command(group, file, line, group_suffix)
    outside = outside substr(text, copy_from)
    inspect_command(outside, file, line, "")
}

function update_inline_ticks(text, active,    i, run) {
    i = 1
    while (i <= length(text)) {
        if (substr(text, i, 1) != "`") {
            i++
            continue
        }
        run = 1
        while (substr(text, i + run, 1) == "`") run++
        if (active == 0)
            active = run
        else if (run == active)
            active = 0
        i += run
    }
    return active
}

function strip_blockquote(line,    text) {
    text = line
    while (text ~ /^[ \t]*>/)
        sub(/^[ \t]*>[ \t]?/, "", text)
    sub(/^[ \t]*/, "", text)
    return text
}

function open_fence(line,    text, c, n) {
    text = strip_blockquote(line)
    c = substr(text, 1, 1)
    if (c != "`" && c != "~") return 0
    n = 0
    while (substr(text, n + 1, 1) == c) n++
    if (n < 3) return 0
    in_fence = 1
    fence_char = c
    fence_len = n
    return 1
}

function close_fence(line,    text, n, rest) {
    text = strip_blockquote(line)
    if (substr(text, 1, 1) != fence_char) return 0
    n = 0
    while (substr(text, n + 1, 1) == fence_char) n++
    if (n < fence_len) return 0
    rest = substr(text, n + 1)
    return rest ~ /^[ \t]*$/
}

function is_markdown_heading_or_table(line,    text) {
    text = strip_blockquote(line)
    return text ~ /^#+[ \t]+/ || text ~ /^\|/
}

function is_markdown_list_start(line,    text) {
    text = strip_blockquote(line)
    return text ~ /^[-*+][ \t]+/ || text ~ /^[0-9]+[.)][ \t]+/
}

function flush_markdown(file) {
    if (pending)
        inspect_markdown(logical, file, first_line)
    pending = 0
    logical = ""
    inline_ticks = 0
}

function is_active_todo_line(line) {
    if (line ~ /^- \[ \]/) {
        todo_active = 1
        return 1
    }
    if (line ~ /^[ \t]*$/)
        return todo_active
    if (line ~ /^#/ || line ~ /^[-*] \[[xX ]\]/) {
        todo_active = 0
        return 0
    }
    if (todo_active && (substr(line, 1, 1) == " " || substr(line, 1, 1) == "\t"))
        return 1
    todo_active = 0
    return 0
}

function append_shell_line(line, file, number,    part, trimmed, continued) {
    if (!pending) {
        logical = ""
        first_line = number
    }
    part = line
    trimmed = part
    sub(/[ \t]+$/, "", trimmed)
    continued = length(trimmed) > 0 && substr(trimmed, length(trimmed), 1) == "\\"
    if (continued) {
        sub(/\\$/, "", trimmed)
        logical = logical trimmed " "
        pending = 1
    } else {
        logical = logical part
        inspect_command(logical, file, first_line, "")
        logical = ""
        pending = 0
    }
}

function append_markdown_line(line, file, number,    part, trimmed, continued) {
    if (!pending) {
        logical = ""
        first_line = number
    } else
        logical = logical " "
    part = line
    trimmed = part
    sub(/[ \t]+$/, "", trimmed)
    continued = length(trimmed) > 0 && substr(trimmed, length(trimmed), 1) == "\\"
    if (continued) {
        sub(/\\$/, "", trimmed)
        logical = logical trimmed " "
    } else {
        logical = logical part
    }
    inline_ticks = update_inline_ticks(part, inline_ticks)
    pending = 1
}

FNR == 1 {
    if (NR > 1 && pending) {
        if (is_markdown(previous_file))
            inspect_markdown(logical, previous_file, first_line)
        else
            inspect_command(logical, previous_file, first_line, "")
    }
    pending = 0
    logical = ""
    inline_ticks = 0
    in_fence = 0
    fence_char = ""
    fence_len = 0
    todo_active = 0
    blockquote_active = 0
}
{
    if (is_todo(FILENAME) && !is_active_todo_line($0)) {
        flush_markdown(FILENAME)
        blockquote_active = 0
        next
    }

    if (is_markdown(FILENAME)) {
        if (in_fence) {
            if (close_fence($0)) {
                if (pending) {
                    inspect_command(logical, FILENAME, first_line, "")
                    pending = 0
                    logical = ""
                }
                in_fence = 0
                fence_char = ""
                fence_len = 0
                next
            }
            append_shell_line($0, FILENAME, FNR)
            next
        }
        if (open_fence($0)) {
            flush_markdown(FILENAME)
            blockquote_active = 0
            next
        }
        if ($0 ~ /^[ \t]*$/ && inline_ticks == 0) {
            flush_markdown(FILENAME)
            blockquote_active = 0
            previous_file = FILENAME
            next
        }
        if ($0 ~ /^[ \t]*>/) {
            if (!blockquote_active)
                flush_markdown(FILENAME)
            blockquote_active = 1
        } else if (blockquote_active) {
            flush_markdown(FILENAME)
            blockquote_active = 0
        }
        if (is_markdown_heading_or_table($0)) {
            flush_markdown(FILENAME)
            inspect_markdown($0, FILENAME, FNR)
            previous_file = FILENAME
            next
        }
        if (inline_ticks == 0 && is_markdown_list_start($0))
            flush_markdown(FILENAME)
        append_markdown_line($0, FILENAME, FNR)
        previous_file = FILENAME
        next
    }

    if (FILENAME ~ /\.rs$/) {
        rust_line = $0
        sub(/^[ \t]*/, "", rust_line)
        if (substr(rust_line, 1, 3) != "///" && substr(rust_line, 1, 3) != "//!" &&
            index(rust_line, "#[ignore") == 0) {
            if (pending) {
                inspect_command(logical, FILENAME, first_line, "")
                pending = 0
                logical = ""
            }
            next
        }
    }
    append_shell_line($0, FILENAME, FNR)
    previous_file = FILENAME
}
END {
    if (pending) {
        if (is_markdown(previous_file))
            inspect_markdown(logical, previous_file, first_line)
        else
            inspect_command(logical, previous_file, first_line, "")
    }
    if (file_mode == 0 && invocations == 0) {
        print "FEHLER: der Repo-Scan fand keine ignorierten lumina-gui cargo test-Aufrufe." > "/dev/stderr"
        exit 1
    }
    if (violations > 0) {
        printf "FEHLER: %d ignorierter GUI-Testaufruf/-aufrufe ohne Serienauflage.\n", violations > "/dev/stderr"
        exit 1
    }
    printf "golden_serial OK: %d passende Aufrufe; %d historische Ausnahme(n).\n", invocations + 0, exemptions + 0
}

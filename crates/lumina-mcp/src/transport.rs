//! MCP stdio transport — the newline-delimited JSON-RPC loop with a HARD cap
//! on the length of a single input line.
//!
//! The loop lives here (and not in `lib.rs`) so that the transport edge can
//! be changed without touching the protocol/dispatch layer, and so that the
//! stdio loop can be driven over an arbitrary reader/writer (the seam
//! [`crate::run_stdio_from`]) instead of the process' own stdin/stdout.
//!
//! # Why the cap exists (SEC-1)
//!
//! The loop used to read stdin with `BufRead::lines()`, which buffers an
//! arbitrarily long line in memory before it is parsed: a 256 MiB line made
//! the server hold the line (`String`) plus the parsed `serde_json::Value`
//! tree plus its clone — ~768 MiB of peak RSS for a single frame. Under
//! memory pressure the allocation failed, the process died with SIGABRT
//! (exit 134) and the client saw a broken pipe instead of a JSON-RPC error.
//!
//! # The rule
//!
//! * Every frame is read with a hard cap of
//!   [`MAX_REPAIR_REGION_LINE_BYTES`] (1.25 MiB) — the absolute upper bound
//!   for ANY line. A longer line is **never** buffered: the reader stops at
//!   the cap, the rest of the line is skipped without being held in memory,
//!   and the client gets a JSON-RPC parse error (`-32700`) whose message
//!   names the limit. The server stays alive and answers the next line
//!   normally.
//! * A frame of at most [`MAX_LINE_BYTES`] (1 MiB) is dispatched exactly as
//!   before.
//! * A frame **over** 1 MiB is dispatched **only** if it carries the
//!   repair-region marker (see below). Otherwise it is refused with the same
//!   JSON-RPC error (limit: 1 MiB) and is **never parsed**.
//!
//! # The repair-region exception — one kind of frame, one marker
//!
//! `lumina_dust_removal` carries a full-frame `u16` repair plane in its
//! arguments (`repair_region.region_values`, `width * height` values; the
//! region MUST have the source frame's dimensions). That plane is the only
//! legitimate payload of the crate above 1 MiB, so exactly this one kind of
//! frame may exceed the 1 MiB limit — up to
//! [`MAX_REPAIR_REGION_LINE_BYTES`] (1.25 MiB, a HARD cap).
//!
//! The exception is recognised by the marker `"repair_region"` — the key
//! that introduces the plane. The marker is searched **textually** in the
//! **WHOLE** frame, not only in its first 1 MiB: the scan runs over every
//! byte of the frame the reader delivers (a marker split across two read
//! chunks is found too — the tail window of the scan is taken from the
//! STREAM, not from the chunk alone). The recognition is therefore
//! **independent of the order** of the JSON keys: a frame whose
//! `repair_region` key only appears after 1 MiB of other content is
//! recognised as well.
//!
//! The scan stops at the newline that ends the frame: the marker of the
//! NEXT frame can therefore never classify the current one.
//!
//! The detection is **textual** — it looks for the canonical spelling
//! `"repair_region"`. The escaped spelling `"\u0072epair_region"` is valid
//! JSON and denotes the SAME key after the deserialisation, but it is NOT
//! recognised: such a frame is refused with the 1 MiB limit (fail-safe —
//! the refusal is the safe direction, there is no security impact).
//!
//! # Memory profile of one line
//!
//! * **Buffer:** at most [`MAX_REPAIR_REGION_LINE_BYTES`] (1.25 MiB) for ANY
//!   line — a 256 MiB line costs 1.25 MiB of buffer, never more. (The buffer
//!   has to reach that far because the marker has to be searched in the
//!   whole frame before the frame can be classified.)
//! * **Parse tree:** only for frames that are actually dispatched.
//!   * Without the marker, a frame over 1 MiB is **not parsed at all** → no
//!     parse tree, no amplification.
//!   * With the marker, up to 1.25 MiB are parsed; the peak RSS grows by
//!     ~65x the length of the frame text — the **RSS-DELTA** ratio (peak
//!     RSS minus the baseline RSS, divided by the length of the frame text;
//!     a repair plane is mostly `"0,"`, two bytes per value) → ~91 MiB for
//!     a full 1.25 MiB frame, which still survives the reproducer of the
//!     original finding (`RLIMIT_AS 128 MiB`). See
//!     [`MAX_REPAIR_REGION_LINE_BYTES`] for the measured table.
//!
//! (The ~65x above is NOT a tree/text ratio — that one is 8.00x for
//! five-to-six-digit values and 16.00x for the realistic plane; see
//! [`MAX_REPAIR_REGION_LINE_BYTES`].)
//!
//! # What did NOT change
//!
//! * A frame within the cap is read and dispatched exactly as before.
//! * The answer format (one JSON object per line on stdout) is unchanged.
//! * An invalid UTF-8 byte sequence is repaired **lossily** (invalid bytes
//!   become U+FFFD) and the frame is then parsed: if it is still valid JSON
//!   afterwards it is **executed**; only if the replacement breaks the JSON
//!   syntax does a parse error follow. The session never ends silently any
//!   more (the old `lines()` aborted the loop on a decode error).

use crate::error::parse_error_response;
use crate::Server;
use serde_json::Value;
use std::io::{BufRead, Write};

/// Hard cap for an ordinary frame: 1 MiB.
///
/// The bound is on the TRANSPORT, not on the payload semantics: every
/// ordinary JSON-RPC frame of the crate fits into it by a wide margin (the
/// biggest ones are a path list, a recipe or a metadata draft — a few KiB at
/// most). A frame over the cap is refused with a JSON-RPC error; it is
/// never parsed.
pub const MAX_LINE_BYTES: usize = 1 << 20;

/// Hard cap for the ONE legitimate oversized frame: 1.25 MiB.
///
/// `lumina_dust_removal` carries a full-frame `u16` repair plane
/// (`region_values`, `width * height` values) in its arguments. That plane
/// is the only legitimate payload of the crate above 1 MiB, so exactly this
/// one kind of frame may exceed the 1 MiB limit — up to
/// [`MAX_REPAIR_REGION_LINE_BYTES`], a HARD cap.
///
/// # Why 1.25 MiB (measured, not guessed)
///
/// A frame is parsed into a `serde_json::Value` tree before it is
/// dispatched, and the tree is FAR bigger than its text. The repair plane
/// consists of `u16` values that are mostly zero — in JSON `"0,"`, i.e. two
/// bytes per value — so the tree costs 32 bytes per value (the size of a
/// `Value`) and the peak RSS grows by **~65x the text length** — the
/// **RSS-DELTA** ratio (peak RSS minus the baseline RSS, divided by the
/// length of the frame text; measured on the real binary, repair plane with
/// 2-byte values):
///
/// | frame (repair plane) | peak RSS | exit under `RLIMIT_AS 128 MiB` |
/// | --- | --- | --- |
/// | 256 KiB | 26.2 MiB | 0 |
/// | 1 MiB | 74.6 MiB | 0 |
/// | **1.25 MiB** | **91.1 MiB** | **0** |
/// | 1.5 MiB | 107.5 MiB | -6 (SIGABRT) |
/// | 8 MiB | 518.2 MiB | -6 (SIGABRT) |
///
/// **Measurement condition of the last two rows:** they come from a **probe
/// build with a hypothetically raised cap** (16 MiB). The **shipped** binary
/// refuses those frames **at the cap** (1 310 720 bytes): exit 0, peak
/// ~9.5 MiB. The peak RSS of the last two rows was measured **without** a
/// memory limit, their exit signature **with** `RLIMIT_AS 128 MiB` — the
/// table therefore mixes two conditions, and only the first three rows hold
/// for the shipped binary.
///
/// (The 8x ratio of the original probe is the **BAUM/TEXT** ratio — the
/// heap bytes of the parsed `Value` tree divided by the bytes of the JSON
/// text — and it holds only for five-to-six-digit values: 32 bytes per
/// `Value` ÷ 6 bytes of text ≈ 5.3x, times the growth of the `Vec` ≈ 8x
/// (measured with a tracking allocator: 8 388 608 heap bytes for a
/// 1 048 573-byte text = 8.00x). The realistic plane is mostly `"0,"` —
/// two bytes of text per value — and its BAUM/TEXT ratio is 16.00x (measured:
/// 16 777 216 heap bytes for a 1 048 577-byte text); its RSS-DELTA ratio is
/// the ~65x of the table above.)
///
/// 1.25 MiB is the largest value whose worst case still survives the
/// reproducer of the original finding (`RLIMIT_AS 128 MiB`) — the same
/// memory class as the ordinary 1 MiB cap (74.6 MiB). A bigger cap would
/// re-open exactly the SIGABRT signature this cap exists to close: a frame
/// of 1.5 MiB already aborts the server under that limit.
///
/// The cap is HARD: a repair-region frame over it is refused exactly like
/// any other oversized frame.
///
/// # The real limit of the inline plane
///
/// The 1.25 MiB cap allows a repair plane of ~655 000 values (~0.65
/// megapixels) — enough for a region-sized repair, not for a full-frame
/// plane of a 24-megapixel photo (that would need ~48 MiB of JSON and
/// ~3 GiB of `Value` tree). A full-frame plane over MCP therefore needs
/// the plane to travel as a compact encoding (base64) or as a file
/// reference; that is a change of the tool contract and NOT part of this
/// transport fix.
pub const MAX_REPAIR_REGION_LINE_BYTES: usize = 1_310_720;

/// The key that introduces the repair-region plane. It is the marker for the
/// one legitimate oversized frame (see [`MAX_REPAIR_REGION_LINE_BYTES`]).
const REPAIR_REGION_MARKER: &[u8] = b"\"repair_region\"";

/// The outcome of one capped read.
#[derive(Debug)]
enum FrameRead {
    /// The frame was read completely (at most `limit` bytes, the trailing
    /// newline consumed). `marker` says whether the repair-region marker
    /// occurs **anywhere** in the frame.
    Complete { text: String, marker: bool },
    /// The frame is longer than the cap: the first `limit` bytes were
    /// buffered, the rest of the line was not.
    TooLong { limit: usize },
    /// End of input (or a read error — the loop treats both like EOF).
    Eof,
}

/// Runs the newline-delimited stdio loop over an arbitrary reader/writer.
///
/// Reads JSON-RPC lines from `reader`, dispatches them through
/// [`Server::handle_line`] and writes one JSON object per response line to
/// `writer`. Notifications receive no response. When the loop ends (EOF,
/// read error or closed stdout) the preview files of the session are removed
/// ([`Server::cleanup_previews`]).
pub fn run_loop<R: BufRead, W: Write>(mut reader: R, mut writer: W, server: &mut Server) {
    loop {
        let frame = match read_frame(&mut reader, MAX_REPAIR_REGION_LINE_BYTES) {
            FrameRead::Eof => break,
            FrameRead::TooLong { limit } => {
                // The frame is over the absolute cap: it was never fully
                // buffered. The client learns about the refusal FIRST; the
                // skip below only resynchronizes the input stream.
                if !write_response(&mut writer, &too_long_response(limit)) {
                    break;
                }
                skip_line(&mut reader);
                continue;
            }
            FrameRead::Complete { text, marker } => {
                if text.len() > MAX_LINE_BYTES && !marker {
                    // An ordinary frame over the 1 MiB limit: refused and
                    // NEVER parsed (no parse tree, no amplification).
                    if !write_response(&mut writer, &too_long_response(MAX_LINE_BYTES)) {
                        break;
                    }
                    continue;
                }
                text
            }
        };
        let trimmed = frame.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(response) = server.handle_line(trimmed) {
            if !write_response(&mut writer, &response) {
                break;
            }
        }
    }

    // Shutdown cleanup (REVIEW R2-MCP-05): never let preview files from this
    // session accumulate in $TMPDIR/lumina-previews/.
    server.cleanup_previews();
}

/// Reads one frame from `reader`, buffering at most `limit` bytes.
///
/// The limit is enforced by the READER, not by a check afterwards: the bytes
/// beyond the cap are never copied into the buffer — they are left in the
/// reader and skipped by [`skip_line`]. A frame that is longer than the cap
/// therefore costs `limit` bytes of memory, no matter how long it is.
///
/// The scan for the repair-region marker runs over EVERY byte of THIS frame
/// that the reader delivers (a marker split across two read chunks is found
/// too), so the classification of a frame does not depend on where the
/// marker sits in it. The scan stops at the newline that ends the frame:
/// the bytes of the NEXT frame never classify this one.
fn read_frame<R: BufRead>(reader: &mut R, limit: usize) -> FrameRead {
    let mut buffer: Vec<u8> = Vec::new();
    let mut tail: Vec<u8> = Vec::new();
    let mut marker = false;
    loop {
        let available = match reader.fill_buf() {
            Ok(bytes) => bytes,
            Err(_) => return FrameRead::Eof,
        };
        if available.is_empty() {
            // EOF: an unterminated last frame is still a frame.
            return if buffer.is_empty() {
                FrameRead::Eof
            } else {
                FrameRead::Complete {
                    text: into_text(buffer),
                    marker,
                }
            };
        }
        // The frame ends at the FIRST newline; the bytes after it belong to
        // the NEXT frame and must not take part in THIS frame's scan: the
        // marker of the next frame must not classify this one (F2).
        let newline = available.iter().position(|byte| *byte == b'\n');
        let frame_bytes = match newline {
            Some(index) => &available[..index],
            None => available,
        };
        if !marker {
            marker = scan_for_marker(&mut tail, frame_bytes);
        }
        match newline {
            Some(index) => {
                if buffer.len() + index > limit {
                    let keep = limit - buffer.len();
                    buffer.extend_from_slice(&available[..keep]);
                    reader.consume(keep);
                    return FrameRead::TooLong { limit };
                }
                buffer.extend_from_slice(&available[..index]);
                reader.consume(index + 1);
                return FrameRead::Complete {
                    text: into_text(buffer),
                    marker,
                };
            }
            None => {
                let length = available.len();
                if buffer.len() + length > limit {
                    let keep = limit - buffer.len();
                    buffer.extend_from_slice(&available[..keep]);
                    reader.consume(keep);
                    return FrameRead::TooLong { limit };
                }
                buffer.extend_from_slice(available);
                reader.consume(length);
            }
        }
    }
}

/// Skips the remainder of the current line (up to and including the next
/// newline) WITHOUT buffering it — the resynchronization after a refused
/// frame.
fn skip_line<R: BufRead>(reader: &mut R) {
    loop {
        let available = match reader.fill_buf() {
            Ok(bytes) => bytes,
            Err(_) => return,
        };
        if available.is_empty() {
            return;
        }
        match available.iter().position(|byte| *byte == b'\n') {
            Some(index) => {
                reader.consume(index + 1);
                return;
            }
            None => {
                let length = available.len();
                reader.consume(length);
            }
        }
    }
}

/// Writes one response as a single JSON line. Returns `false` when the sink
/// is closed (the loop then ends).
fn write_response<W: Write>(writer: &mut W, response: &Value) -> bool {
    let serialized = serde_json::to_string(response).unwrap_or_else(|_| "{}".to_string());
    if writer.write_all(serialized.as_bytes()).is_err() {
        return false;
    }
    let _ = writer.write_all(b"\n");
    let _ = writer.flush();
    true
}

/// Builds the JSON-RPC error for a frame that exceeded the transport cap.
///
/// `-32700` (parse error) is the honest code here: the frame was refused at
/// the transport layer BEFORE it was parsed, so no request object (and no
/// id) exists. `-32600` (invalid request) would claim that the frame was
/// parsed into a request — exactly what the cap forbids.
fn too_long_response(limit: usize) -> Value {
    parse_error_response(format!(
        "line too long: the frame exceeds the limit of {limit} bytes"
    ))
}

/// Converts the raw frame bytes into a `String` without copying them when the
/// bytes are valid UTF-8 (the normal case). Invalid bytes are repaired
/// **lossily** (they become U+FFFD), so such a frame still reaches the JSON
/// parser: if it is valid JSON afterwards it is executed, and only if the
/// replacement breaks the JSON syntax does a parse error follow. The session
/// never ends silently any more.
fn into_text(buffer: Vec<u8>) -> String {
    match String::from_utf8(buffer) {
        Ok(text) => text,
        Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
    }
}

/// Scans a chunk for the repair-region marker and returns whether it was
/// found.
///
/// `tail` carries the last `REPAIR_REGION_MARKER.len() - 1` bytes of the
/// STREAM (the bytes before `chunk`), so a marker that straddles a chunk
/// boundary is found as well — no matter how short the chunks are.
fn scan_for_marker(tail: &mut Vec<u8>, chunk: &[u8]) -> bool {
    let marker = REPAIR_REGION_MARKER;
    let keep = marker.len() - 1;
    // The marker may straddle the chunk boundary: probe the tail plus the
    // first `keep` bytes of the chunk.
    let overlap = keep.min(chunk.len());
    let mut probe = Vec::with_capacity(tail.len() + overlap);
    probe.extend_from_slice(tail);
    probe.extend_from_slice(&chunk[..overlap]);
    if probe.windows(marker.len()).any(|window| window == marker) {
        return true;
    }
    if chunk.windows(marker.len()).any(|window| window == marker) {
        return true;
    }
    // Carry the last `keep` bytes of the STREAM (`tail ++ chunk`) over to the
    // next chunk.
    //
    // The window is taken from the STREAM, not from the chunk alone: with a
    // chunk that is shorter than the marker the chunk-only window would be
    // too short to ever match, and a marker that straddles two chunks would
    // be missed (the frame would then be refused although it carries the
    // marker).
    let tail_len = tail.len();
    let chunk_len = chunk.len();
    let mut carried: Vec<u8> = Vec::with_capacity(keep);
    if chunk_len >= keep {
        carried.extend_from_slice(&chunk[chunk_len - keep..]);
    } else if tail_len + chunk_len >= keep {
        let from_tail = keep - chunk_len;
        carried.extend_from_slice(&tail[tail_len - from_tail..]);
        carried.extend_from_slice(chunk);
    } else {
        carried.extend_from_slice(tail);
        carried.extend_from_slice(chunk);
    }
    tail.clear();
    tail.extend_from_slice(&carried);
    false
}

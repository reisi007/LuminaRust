//! SEC-1 (Sicherheits-Review-Welle 1): the MCP stdio transport must refuse
//! an oversized line with a JSON-RPC error instead of buffering it.
//!
//! # The defect
//!
//! `crates/lumina-mcp/src/lib.rs` → `run_stdio` read stdin with
//! `BufRead::lines()` — WITHOUT any length limit. A 256 MiB line was
//! buffered completely (line `String` + parsed `serde_json::Value` tree +
//! its clone: ~768 MiB peak RSS for one frame). Under memory pressure the
//! allocation failed → SIGABRT (exit 134) → the client saw a broken pipe and
//! NO JSON-RPC error at all.
//!
//! # What this test pins
//!
//! 1. **The capping and the exception are separate.** An ordinary frame over
//!    the 1 MiB limit ([`MAX_LINE_BYTES`]) is refused with a JSON-RPC error
//!    that names THAT limit — and it is never parsed (no result, no tool
//!    call).
//! 2. **The one exception is hard-capped.** A frame over
//!    [`MAX_REPAIR_REGION_LINE_BYTES`] (1,25 MiB = 1 310 720 Byte) is refused
//!    with a JSON-RPC error that names THAT limit — and it is not read in
//!    full: when the first answer is written, the reader must have delivered
//!    at most the cap plus its own read-ahead, NOT the whole frame. This is
//!    the proof that no frame can reach the SIGABRT signature any more.
//! 3. **The server lives.** The line AFTER a refused frame is answered
//!    normally (a `result`, not an error).
//! 4. **The one legitimate outlier still works.** A `lumina_dust_removal`
//!    call carries a full-frame `u16` repair plane and therefore exceeds the
//!    1 MiB cap; within the 1,25-MiB cap it must reach the TOOL instead of
//!    being refused at the transport.
//! 5. **The marker is searched in the WHOLE frame** — behind more than 1 MiB
//!    of other content, across the chunk boundary of a reader that delivers
//!    7 bytes at a time — and ONLY in the frame that carries it: the marker
//!    of the NEXT frame must not classify the current one.
//! 6. **The shutdown cleanup still runs.** A preview the transport created
//!    (via the tool) is removed when the transport ends.
//!
//! The seam under test is [`lumina_mcp::run_stdio_from`] — the same loop the
//! real `run_stdio` runs, with an injected reader/writer (no process spawn,
//! no environment variable).

use lumina_core::{ImageFileFormat, ImageFrame};
use lumina_mcp::{
    run_stdio_from, run_stdio_with, Server, MAX_LINE_BYTES, MAX_REPAIR_REGION_LINE_BYTES,
};
use serde_json::{json, Value};
use std::cell::Cell;
use std::fs;
use std::io::{BufReader, Cursor, Read, Write};
use std::path::PathBuf;
use std::rc::Rc;

/// Read-ahead of the injected reader. The transport sees the frame in chunks
/// of this size; the proofs allow for exactly one such chunk of read-ahead
/// beyond the cap.
const READ_AHEAD: usize = 64 * 1024;

/// Length of a frame that is over the 1 MiB ordinary limit but still below
/// the 1.25 MiB absolute cap.
const OVER_LIMIT_FRAME: usize = 1_100_000;

/// Length of the frame that carries the repair-region marker and is over the
/// absolute cap — the regression against a textually triggerable exception.
const OVERSIZED_MARKER_FRAME: usize = 16 * 1024 * 1024;

/// The reader side: counts how many bytes were handed to the transport.
struct CountingReader {
    inner: Cursor<Vec<u8>>,
    delivered: Rc<Cell<usize>>,
}

impl Read for CountingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buffer)?;
        self.delivered.set(self.delivered.get() + read);
        Ok(read)
    }
}

/// The writer side: keeps the answers and records how many bytes the reader
/// had delivered when the FIRST answer was written.
struct RecordingWriter {
    output: Vec<u8>,
    delivered: Rc<Cell<usize>>,
    delivered_at_first_write: usize,
    writes: usize,
    /// Called before every write — the hook that observes the file system
    /// WHILE the transport is running.
    observer: Option<Box<dyn FnMut()>>,
}

impl Write for RecordingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self.writes == 0 {
            self.delivered_at_first_write = self.delivered.get();
        }
        self.writes += 1;
        if let Some(observer) = &mut self.observer {
            observer();
        }
        self.output.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// What one transport run produced.
struct TransportRun {
    /// The raw answer lines (one JSON object per line).
    answers: Vec<String>,
    /// How many bytes the reader had delivered when the first answer was
    /// written.
    delivered_at_first_write: usize,
}

/// Runs the stdio loop over `input` with a fresh server and collects the
/// answers.
///
/// `chunk` is the capacity of the reader's buffer and therefore the size of
/// the chunks the transport sees — the chunk boundaries are reproducible,
/// not a matter of pipe timing.
fn run_transport(input: Vec<u8>, chunk: usize) -> TransportRun {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::with_preview_dir(dir.path().join("previews"));
    let delivered = Rc::new(Cell::new(0));
    let reader = BufReader::with_capacity(
        chunk,
        CountingReader {
            inner: Cursor::new(input),
            delivered: Rc::clone(&delivered),
        },
    );
    let mut writer = RecordingWriter {
        output: Vec::new(),
        delivered: Rc::clone(&delivered),
        delivered_at_first_write: 0,
        writes: 0,
        observer: None,
    };
    run_stdio_from(reader, &mut writer, server);
    let answers = String::from_utf8(writer.output.clone())
        .expect("the answers must be UTF-8")
        .lines()
        .map(str::to_owned)
        .collect();
    TransportRun {
        answers,
        delivered_at_first_write: writer.delivered_at_first_write,
    }
}

/// Runs the stdio loop over `input` against an existing server.
fn run_transport_with_server(
    input: Vec<u8>,
    writer: &mut RecordingWriter,
    server: &mut Server,
) -> TransportRun {
    let delivered = Rc::clone(&writer.delivered);
    let reader = BufReader::with_capacity(
        READ_AHEAD,
        CountingReader {
            inner: Cursor::new(input),
            delivered,
        },
    );
    run_stdio_with(reader, &mut *writer, server);
    let answers = String::from_utf8(writer.output.clone())
        .expect("the answers must be UTF-8")
        .lines()
        .map(str::to_owned)
        .collect();
    TransportRun {
        answers,
        delivered_at_first_write: writer.delivered_at_first_write,
    }
}

/// A syntactically valid JSON-RPC frame whose `params.padding` is `size`
/// bytes long. It carries NO repair-region marker.
fn oversized_frame(size: usize) -> Vec<u8> {
    let mut frame = Vec::with_capacity(size + 128);
    frame.extend_from_slice(br#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"padding":""#);
    frame.resize(frame.len() + size, b'x');
    frame.extend_from_slice(br#""}}"#);
    frame
}

/// A `lumina_dust_removal` call whose repair region carries `size` bytes of
/// padding — the one legitimate oversized frame (it carries the marker).
fn repair_region_frame(size: usize) -> Vec<u8> {
    let mut frame = Vec::with_capacity(size + 512);
    frame.extend_from_slice(
        br#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"lumina_dust_removal","arguments":{"input":"/does/not/exist.png","repair_region":{"id":"spot-1","kind":"dust-removal","region_width":2,"region_height":2,"region_values":[0,0,0,0],"replacement_path":"/does/not/exist.png","padding":""#,
    );
    frame.resize(frame.len() + size, b'y');
    frame.extend_from_slice(br#""}}}}"#);
    frame
}

/// The offset of the repair-region marker inside a frame built by
/// [`marker_frame_after`].
fn marker_offset(frame: &[u8]) -> usize {
    frame
        .windows(15)
        .position(|window| window == b"\"repair_region\"")
        .expect("the frame carries the marker")
}

/// A `lumina_dust_removal` call whose repair-region KEY comes after
/// `padding` bytes of other content — the marker therefore sits at
/// [`marker_offset`] inside the frame.
fn marker_frame_after(padding: usize) -> Vec<u8> {
    let mut frame = Vec::with_capacity(padding + 512);
    frame.extend_from_slice(
        br#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"lumina_dust_removal","arguments":{"input":"/does/not/exist.png","padding":""#,
    );
    frame.resize(frame.len() + padding, b'p');
    frame.extend_from_slice(
        br#"","repair_region":{"id":"spot-1","kind":"dust-removal","region_width":2,"region_height":2,"region_values":[0,0,0,0],"replacement_path":"/does/not/exist.png"}}}}"#,
    );
    frame
}

/// Asserts that the answer is the transport's "line too long" refusal and
/// that its message names `limit`.
fn assert_too_long(answer: &str, limit: usize) {
    let response: Value = serde_json::from_str(answer).expect("the refusal must be valid JSON");
    assert_eq!(response["jsonrpc"], "2.0");
    assert!(
        response["id"].is_null(),
        "a frame that was never parsed has no id: {response}"
    );
    assert_eq!(response["error"]["code"], -32700);
    let message = response["error"]["message"]
        .as_str()
        .expect("the error must carry a message");
    assert!(
        message.contains("line too long"),
        "the message must name the defect: {message}"
    );
    assert!(
        message.contains(&limit.to_string()),
        "the message must name the limit {limit}: {message}"
    );
}

#[test]
fn oversized_stdin_line_is_rejected_with_a_protocol_error() {
    // (1) The ordinary cap: a frame over 1 MiB WITHOUT the repair-region
    //     marker is refused with the 1 MiB limit — and never parsed.
    let mut input = oversized_frame(OVER_LIMIT_FRAME);
    input.push(b'\n');
    input.extend_from_slice(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n");

    let run = run_transport(input, READ_AHEAD);
    assert_eq!(
        run.answers.len(),
        2,
        "expected the refusal and the answer of the following line, got: {:?}",
        run.answers
    );
    assert_too_long(&run.answers[0], MAX_LINE_BYTES);

    // (3) The server lives: the line after the refused one is answered
    //     normally.
    let answer: Value =
        serde_json::from_str(&run.answers[1]).expect("the answer must be valid JSON");
    assert_eq!(answer["id"], 2);
    assert_eq!(answer["result"], json!({}));
    assert!(
        answer.get("error").is_none(),
        "the ping must not fail: {answer}"
    );

    // (2) The hard cap of the one exception: a frame over the absolute cap
    //     WITH the marker is refused with that cap — and it is NOT read in
    //     full: at the first answer the reader has delivered at most the
    //     cap plus one read-ahead chunk, NOT the whole frame. This is the
    //     proof that no frame reaches the SIGABRT signature any more.
    let mut input = repair_region_frame(OVERSIZED_MARKER_FRAME);
    input.push(b'\n');
    let run = run_transport(input, READ_AHEAD);
    assert_eq!(
        run.answers.len(),
        1,
        "the oversized marker frame must be answered exactly once: {:?}",
        run.answers
    );
    assert_too_long(&run.answers[0], MAX_REPAIR_REGION_LINE_BYTES);
    let read_before_answer = run.delivered_at_first_write;
    assert!(
        read_before_answer <= MAX_REPAIR_REGION_LINE_BYTES + 2 * READ_AHEAD,
        "the transport read {read_before_answer} bytes before it answered the oversized \
         marker frame — it must stop at the cap of {MAX_REPAIR_REGION_LINE_BYTES} bytes"
    );

    // (4) The one legitimate oversized frame still reaches the TOOL.
    let mut input = repair_region_frame(OVER_LIMIT_FRAME);
    input.push(b'\n');
    let run = run_transport(input, READ_AHEAD);
    assert_eq!(
        run.answers.len(),
        1,
        "the repair-region frame must be answered exactly once: {:?}",
        run.answers
    );
    let answer: Value =
        serde_json::from_str(&run.answers[0]).expect("the answer must be valid JSON");
    assert_eq!(answer["id"], 3);
    assert!(
        answer.get("error").is_none(),
        "the repair-region frame must not be refused at the transport: {answer}"
    );
    assert_eq!(
        answer["result"]["structuredContent"]["error"], "FileNotFound",
        "the frame must have reached the tool: {answer}"
    );
}

#[test]
fn the_transport_removes_the_preview_files_it_created() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("photo.png");
    let preview_dir: PathBuf = dir.path().join("previews");

    // A tiny synthetic source frame (2x2, opaque).
    let frame = ImageFrame::new(
        2,
        2,
        vec![
            10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
        ],
    )
    .unwrap();
    fs::write(&source, frame.encode(ImageFileFormat::Png).unwrap()).unwrap();

    let mut server = Server::with_preview_dir(preview_dir.clone());

    // Step 1: load the image into the session.
    let load_frame = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{{\"name\":\"lumina_load\",\"arguments\":{{\"path\":{:?}}}}}}}\n",
        source.to_string_lossy()
    );
    let mut writer = RecordingWriter {
        output: Vec::new(),
        delivered: Rc::new(Cell::new(0)),
        delivered_at_first_write: 0,
        writes: 0,
        observer: None,
    };
    let load_run = run_transport_with_server(load_frame.into_bytes(), &mut writer, &mut server);
    assert_eq!(load_run.answers.len(), 1, "{:?}", load_run.answers);
    let load_answer: Value = serde_json::from_str(&load_run.answers[0]).expect("valid JSON");
    let image_id = load_answer["result"]["structuredContent"]["image_id"]
        .as_str()
        .expect("the load call must report an image_id")
        .to_owned();

    // Step 2: the preview tool writes a preview into the preview directory.
    // The observer checks the directory WHILE the transport is running.
    let observed = Rc::new(Cell::new(false));
    let observed_preview_dir = preview_dir.clone();
    let observer_observed = Rc::clone(&observed);
    let observer = move || {
        let entries = fs::read_dir(&observed_preview_dir)
            .map(|entries| entries.count())
            .unwrap_or(0);
        if entries > 0 {
            observer_observed.set(true);
        }
    };
    let preview_frame = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{{\"name\":\"lumina_preview\",\"arguments\":{{\"image_id\":{:?}}}}}}}\n",
        image_id
    );
    let mut writer = RecordingWriter {
        output: Vec::new(),
        delivered: Rc::new(Cell::new(0)),
        delivered_at_first_write: 0,
        writes: 0,
        observer: Some(Box::new(observer)),
    };
    let preview_run =
        run_transport_with_server(preview_frame.into_bytes(), &mut writer, &mut server);
    assert_eq!(preview_run.answers.len(), 1, "{:?}", preview_run.answers);
    let preview_answer: Value = serde_json::from_str(&preview_run.answers[0]).expect("valid JSON");
    assert_eq!(
        preview_answer["result"]["structuredContent"]["ok"], true,
        "the preview must have been written: {preview_answer}"
    );

    // The preview file existed WHILE the transport was running ...
    assert!(
        observed.get(),
        "the preview file must exist while the transport is running"
    );
    // ... and is gone after the transport ended: the shutdown cleanup
    // removed it.
    let remaining = fs::read_dir(&preview_dir)
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(
        remaining, 0,
        "the preview directory must be empty after the transport ended"
    );
}

/// SEC-1, Fix-Runde 3 (a): the marker is searched in the WHOLE frame — a
/// `repair_region` key that only appears BEHIND more than 1 MiB of other
/// content is recognised, so the frame reaches the tool instead of being
/// refused at the transport.
#[test]
fn the_marker_behind_more_than_one_mib_is_found() {
    let frame = marker_frame_after(OVER_LIMIT_FRAME);
    assert!(
        marker_offset(&frame) > MAX_LINE_BYTES,
        "the marker must sit behind 1 MiB"
    );
    assert!(
        frame.len() <= MAX_REPAIR_REGION_LINE_BYTES,
        "the frame must stay under the hard cap"
    );
    let mut input = frame;
    input.push(b'\n');
    let run = run_transport(input, READ_AHEAD);
    assert_eq!(run.answers.len(), 1, "{:?}", run.answers);
    let answer: Value =
        serde_json::from_str(&run.answers[0]).expect("the answer must be valid JSON");
    assert_eq!(
        answer["id"], 3,
        "the marker behind 1 MiB must be found and the frame dispatched: {answer}"
    );
    assert_eq!(
        answer["result"]["structuredContent"]["error"], "FileNotFound",
        "the frame must have reached the tool: {answer}"
    );
}

/// SEC-1, Fix-Runde 3 (b): a marker that STRADDLES a chunk boundary is
/// found — the reader delivers only 7 bytes at a time, so the tail window
/// of the scan must not be cut to the chunk length.
#[test]
fn a_marker_across_a_chunk_boundary_is_found() {
    const CHUNK: usize = 7;
    // Put the marker at offset 3 inside its 7-byte chunk: it then spans
    // three chunk boundaries.
    let offset = marker_offset(&marker_frame_after(OVER_LIMIT_FRAME));
    let padding = OVER_LIMIT_FRAME - (offset + CHUNK - 3).rem_euclid(CHUNK);
    let frame = marker_frame_after(padding);
    assert_eq!(marker_offset(&frame) % CHUNK, 3, "the marker must straddle");
    assert!(marker_offset(&frame) > MAX_LINE_BYTES, "behind 1 MiB");
    let mut input = frame;
    input.push(b'\n');
    let run = run_transport(input, CHUNK);
    assert_eq!(run.answers.len(), 1, "{:?}", run.answers);
    let answer: Value =
        serde_json::from_str(&run.answers[0]).expect("the answer must be valid JSON");
    assert_eq!(
        answer["id"], 3,
        "the marker across the chunk boundary must be found: {answer}"
    );
    assert_eq!(
        answer["result"]["structuredContent"]["error"], "FileNotFound",
        "the frame must have reached the tool: {answer}"
    );
}

/// SEC-1, Fix-Runde 3 (c): the marker scan stops at the newline that ends
/// the frame — the marker of the NEXT frame (delivered in the SAME chunk)
/// must not classify the current one: the 1.1 MiB frame without a marker
/// is refused with the 1 MiB limit, the next frame is dispatched.
#[test]
fn the_marker_of_the_next_frame_does_not_classify_this_frame() {
    let mut input = oversized_frame(OVER_LIMIT_FRAME);
    input.push(b'\n');
    input.extend_from_slice(&marker_frame_after(OVER_LIMIT_FRAME));
    input.push(b'\n');
    // ONE chunk of 4 MiB: both frames arrive in a single read.
    let run = run_transport(input, 4 * 1024 * 1024);
    assert_eq!(run.answers.len(), 2, "{:?}", run.answers);
    assert_too_long(&run.answers[0], MAX_LINE_BYTES);
    let answer: Value =
        serde_json::from_str(&run.answers[1]).expect("the answer must be valid JSON");
    assert_eq!(
        answer["id"], 3,
        "the second frame must be dispatched: {answer}"
    );
    assert_eq!(
        answer["result"]["structuredContent"]["error"], "FileNotFound",
        "the second frame must have reached the tool: {answer}"
    );
}

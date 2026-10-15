//! MCP-PARITY-C: the MCP surface of the two path-based merge tools
//! `lumina_merge_hdr` / `lumina_merge_pano` — registry and schema
//! (SOLL §8, phase 1; T1, T3, T4). The argument/validation half (phase 2:
//! T5–T10 and the §5 payload contract) lives in `merge_tools_validation.rs`,
//! the byte identity against the real `lumina-cli` binary in the later
//! parity slice.

use lumina_mcp::tools::merge_hdr;
use serde_json::json;

#[path = "merge_tools_common/mod.rs"]
mod support;
use support::*;

/// Both merge tools are listed, dispatchable and known — and a listed-but-dead
/// tool (a missing `dispatch_tool` arm) must fail here, so the
/// `MethodNotFound` sentinel is named explicitly (the measured mutation from
/// `library_tools.rs`: deleting the dispatch arm leaves the weaker form green).
#[test]
fn t1_both_merge_tools_are_registered_in_list_dispatch_and_is_known_tool() {
    let definitions = lumina_mcp::tools::list_tool_definitions();
    let listed: Vec<&str> = definitions
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    for name in MERGE_TOOLS {
        assert!(
            listed.contains(name),
            "{name} is missing from list_tool_definitions (tools/list): {listed:?}"
        );
        assert!(
            lumina_mcp::tools::is_known_tool(name),
            "{name} is not accepted by is_known_tool"
        );
    }
    // The real `tools/list` seam: both names visible, no duplicates.
    let response = server()
        .handle_message(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .unwrap();
    let tools = response["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    for name in MERGE_TOOLS {
        assert!(
            names.contains(name),
            "tools/list is missing {name}: {names:?}"
        );
    }
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), names.len(), "tools/list reports a duplicate");
    // `dispatch_tool` must know both names: a call with empty arguments fails
    // the argument shape gate (a tool error inside the result) and never
    // reaches the fall-through `MethodNotFound`.
    for name in MERGE_TOOLS {
        let response = call(name, json!({}));
        assert!(
            response.get("error").is_none(),
            "{name} is listed but dispatch_tool does not know it: {response}"
        );
        assert_eq!(
            response["result"]["isError"], true,
            "{name} with no arguments must be refused, not answered: {response}"
        );
        assert_ne!(
            response["result"]["structuredContent"]["error"],
            json!("MethodNotFound"),
            "{name} has no dispatch_tool arm (a listed-but-dead tool): {response}"
        );
    }
}

// -------------------------------------------------------------- T3: schema

/// `tools/list` publishes both schemas with the §2 contract. The
/// `minItems`/`maximum` keywords are *description* for the client — the
/// handler does not enforce them (T5–T7 prove the merge domain does) — and the
/// documented defaults (`<ref-stem>-HDR.dng` / `-Pano.dng`, 16, 64) match the
/// CLI's clap defaults.
#[test]
fn t3_tools_list_publishes_both_schemas_with_the_soll_contract() {
    let response = server()
        .handle_message(json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .unwrap();
    let tools = response["result"]["tools"].as_array().unwrap();
    for name in MERGE_TOOLS {
        let tool = tools
            .iter()
            .find(|tool| tool["name"].as_str() == Some(*name))
            .expect("tool");
        assert!(
            !tool["description"].as_str().unwrap().is_empty(),
            "{name}: no description"
        );
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], json!("object"), "{name}");
        assert_eq!(
            schema["additionalProperties"],
            json!(false),
            "{name}: the schema must forbid unknown fields"
        );
        let required = schema["required"].as_array().unwrap();
        assert_eq!(
            required,
            &vec![json!("inputs")],
            "{name}: `inputs` is the only required field"
        );
        // `inputs`: string array, count documented 2..=256 (descriptive).
        let inputs = &schema["properties"]["inputs"];
        assert_eq!(inputs["type"], json!("array"), "{name}");
        assert_eq!(inputs["items"]["type"], json!("string"), "{name}");
        assert_eq!(inputs["minItems"], json!(2), "{name}");
        assert_eq!(inputs["maxItems"], json!(256), "{name}");
        // `output`: optional, `null` is the documented default target.
        let output = &schema["properties"]["output"];
        assert_eq!(output["type"], json!(["string", "null"]), "{name}");
        let default_target = if *name == "lumina_merge_hdr" {
            "-HDR.dng"
        } else {
            "-Pano.dng"
        };
        assert!(
            output["description"]
                .as_str()
                .unwrap()
                .contains(default_target),
            "{name}: the default target must be documented"
        );
        // The three explicit exposure lists: number / integer / number items.
        for key in ["exposure_times", "f_numbers"] {
            assert_eq!(
                schema["properties"][key]["items"]["type"],
                json!("number"),
                "{name}.{key}"
            );
        }
        assert_eq!(
            schema["properties"]["isos"]["items"]["type"],
            json!("integer"),
            "{name}.isos: JSON integers, no float cast"
        );
        // `max_shift_px`: integer, `minimum: 0`, NO documented upper limit
        // (SOLL §13 Punkt 4 — the code has none).
        let shift = &schema["properties"]["max_shift_px"];
        assert_eq!(shift["type"], json!("integer"), "{name}");
        assert_eq!(shift["minimum"], json!(0), "{name}");
        assert!(
            shift.get("maximum").is_none(),
            "{name}: no undocumented upper limit for max_shift_px"
        );
        assert!(shift["description"].as_str().unwrap().contains('1'));
        // `blend_width_px`: integer with the documented 0..=65536 range.
        let blend = &schema["properties"]["blend_width_px"];
        assert_eq!(blend["type"], json!("integer"), "{name}");
        assert_eq!(blend["minimum"], json!(0), "{name}");
        assert_eq!(blend["maximum"], json!(65536), "{name}");
        // `force`: boolean.
        assert_eq!(
            schema["properties"]["force"]["type"],
            json!("boolean"),
            "{name}"
        );
    }
}

// ------------------------------------------------------- T4: one core, two names

/// The two tools are one implementation: their schemas are identical except
/// for the descriptions. Slice A adds the one shared `enum` vocabulary
/// (`gain_compensation`: `off` | `overlap-median`) — identical on both
/// modes, so still "one schema modulo description". `MergeMode` is the only
/// variable.
#[test]
fn t4_both_schemas_are_one_core_modulo_description() {
    let hdr = merge_hdr::schema();
    let pano = lumina_mcp::tools::merge_pano::schema();
    // They DO differ (the documented default target name) …
    assert_ne!(
        hdr, pano,
        "the two modes must document their own default target"
    );
    // … but only in the descriptions: strip them and both trees are equal.
    let hdr_stripped = strip_descriptions(hdr);
    let pano_stripped = strip_descriptions(pano);
    // The single shared enum vocabulary (Slice A): present on both modes
    // with the same values — a per-mode vocabulary would fail the equality
    // below.
    for stripped in [&hdr_stripped, &pano_stripped] {
        assert_eq!(
            stripped["properties"]["gain_compensation"]["enum"],
            json!(["off", "overlap-median"]),
            "the gain vocabulary is shared, not per-mode: {stripped}"
        );
    }
    assert_eq!(
        hdr_stripped, pano_stripped,
        "one shared core must publish one schema modulo description"
    );
}

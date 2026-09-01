// Standalone generator for the hash-pinned ONNX behavior fixture.
// Reproduces EXACTLY the crafted ReduceMax protobuf graph used by
// crates/lumina-onnx/tests/ort_backend.rs, so the committed binary fixture
// matches the documented source-of-truth encoder.

use std::io::Write;

const W: u32 = 8;
const H: u32 = 8;
const INPUT_NAME: &str = "x";
const OUTPUT_NAME: &str = "y";

fn push_varint(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
}

fn push_tag(out: &mut Vec<u8>, field: u32, wire_type: u64) {
    push_varint(out, ((field as u64) << 3) | wire_type);
}

fn push_len_delimited(out: &mut Vec<u8>, field: u32, payload: &[u8]) {
    push_tag(out, field, 2);
    push_varint(out, payload.len() as u64);
    out.extend_from_slice(payload);
}

fn push_string(out: &mut Vec<u8>, field: u32, value: &str) {
    push_len_delimited(out, field, value.as_bytes());
}

fn push_varint_field(out: &mut Vec<u8>, field: u32, value: u64) {
    push_tag(out, field, 0);
    push_varint(out, value);
}

fn dimension(value: i64) -> Vec<u8> {
    let mut out = Vec::new();
    push_varint_field(&mut out, 1, value as u64);
    out
}

fn shape_proto(dims: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    for dim in dims {
        push_len_delimited(&mut out, 1, &dimension(*dim));
    }
    out
}

fn tensor_type(dims: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    push_varint_field(&mut out, 1, 1); // FLOAT
    push_len_delimited(&mut out, 2, &shape_proto(dims));
    out
}

fn type_proto(dims: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    push_len_delimited(&mut out, 1, &tensor_type(dims));
    out
}

fn value_info(name: &str, dims: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    push_string(&mut out, 1, name);
    push_len_delimited(&mut out, 2, &type_proto(dims));
    out
}

fn axes_attribute(axes: &[i64]) -> Vec<u8> {
    let mut out = Vec::new();
    push_string(&mut out, 1, "axes");
    push_varint_field(&mut out, 20, 7); // INTS
    let mut packed = Vec::new();
    for axis in axes {
        push_varint(&mut packed, *axis as u64);
    }
    push_len_delimited(&mut out, 8, &packed);
    out
}

fn keepdims_attribute() -> Vec<u8> {
    let mut out = Vec::new();
    push_string(&mut out, 1, "keepdims");
    push_varint_field(&mut out, 20, 2); // INT
    push_varint_field(&mut out, 3, 1); // keepdims = true
    out
}

fn reduce_max_node() -> Vec<u8> {
    let mut out = Vec::new();
    push_string(&mut out, 1, INPUT_NAME);
    push_string(&mut out, 2, OUTPUT_NAME);
    push_string(&mut out, 4, "ReduceMax");
    push_len_delimited(&mut out, 5, &axes_attribute(&[1]));
    push_len_delimited(&mut out, 5, &keepdims_attribute());
    out
}

fn graph_proto() -> Vec<u8> {
    let mut out = Vec::new();
    push_len_delimited(&mut out, 1, &reduce_max_node());
    push_string(&mut out, 2, "lumina-crafted-graph");
    push_len_delimited(
        &mut out,
        11,
        &value_info(INPUT_NAME, &[1, 3, H as i64, W as i64]),
    );
    push_len_delimited(
        &mut out,
        12,
        &value_info(OUTPUT_NAME, &[1, 1, H as i64, W as i64]),
    );
    out
}

fn crafted_onnx_bytes() -> Vec<u8> {
    let mut opset = Vec::new();
    push_varint_field(&mut opset, 2, 13);
    let mut out = Vec::new();
    push_varint_field(&mut out, 1, 8); // ir_version 8
    push_len_delimited(&mut out, 7, &graph_proto());
    push_len_delimited(&mut out, 8, &opset);
    out
}

fn main() {
    let bytes = crafted_onnx_bytes();
    let out_path = std::env::args().nth(1).expect("output path");
    let mut f = std::fs::File::create(&out_path).expect("create file");
    f.write_all(&bytes).expect("write bytes");
    // SHA-256 hex (hand-rolled, no deps).
    let mut hasher = sha256();
    update_sha256(&mut hasher, &bytes);
    let digest = finish_sha256(&mut hasher);
    println!("wrote {} bytes", bytes.len());
    println!("sha256={}", to_hex(&digest));
}

// Minimal SHA-256 implementation (pure std) for the one-shot generation.
fn sha256() -> [u32; 8] {
    [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
        0x1f83d9ab, 0x5be0cd19,
    ]
}

fn update_sha256(state: &mut [u32; 8], data: &[u8]) {
    // We only need a single small message (< 64 bytes likely), so handle the
    // one-block case simply. For robustness just do a full correct impl:
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    process_blocks(state, &msg);
}

fn process_blocks(state: &mut [u32; 8], msg: &[u8]) {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, chunk) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }
}

fn finish_sha256(state: &mut [u32; 8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, word) in state.iter().enumerate() {
        let be = word.to_be_bytes();
        out[i * 4..i * 4 + 4].copy_from_slice(&be);
    }
    out
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

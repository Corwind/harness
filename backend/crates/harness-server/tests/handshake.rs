//! Behavior test: handshake JSON shape.
//!
//! The Swift parent reads exactly one line from stdout and parses it as
//! `{"port": <u16>, "token": <string>}`. T1.A guarantees that line is a
//! single-line JSON object with exactly those two keys.

use harness_server::Handshake;
use pretty_assertions::assert_eq;

#[test]
fn handshake_json_line_is_single_line_with_port_and_token() {
    let h = Handshake::new(54321, "abc123");
    let line = h.to_json_line();

    assert!(
        !line.contains('\n'),
        "handshake must be single-line: {line:?}"
    );

    let parsed: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
    let obj = parsed.as_object().expect("object");
    assert_eq!(obj.len(), 2, "exactly two keys: {obj:?}");
    assert_eq!(obj.get("port").and_then(|v| v.as_u64()), Some(54321));
    assert_eq!(obj.get("token").and_then(|v| v.as_str()), Some("abc123"));
}

#[test]
fn handshake_round_trips_through_json() {
    let original = Handshake::new(8080, "deadbeefcafef00d");
    let line = original.to_json_line();
    let decoded: Handshake = serde_json::from_str(&line).expect("decode");
    assert_eq!(decoded, original);
}

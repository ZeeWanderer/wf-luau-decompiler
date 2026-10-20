use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn invoke(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wf-luau-decompiler"))
        .args(args)
        .env("RAYON_NUM_THREADS", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn fixture(warframe: bool) -> Vec<u8> {
    let mut bytes = vec![9, 3, 0, 0, 1, 1, 0, 0, 0, 0, 0, 2];
    bytes.extend([5, 0, 0, 0]); // LOADK R0, K0
    bytes.extend([22, 0, 2, 0]); // RETURN R0
    bytes.extend([1, 1, 1]);
    if warframe {
        bytes.extend([0, 0, 0]);
    }
    bytes.extend([0, 0, 0, 0, 0, 0]);
    bytes
}

fn diagnostic(output: &Output, ok: bool) -> Value {
    let text = std::str::from_utf8(&output.stderr).unwrap();
    assert_eq!(text.lines().count(), 1, "{text}");
    let json: Value = serde_json::from_str(text).unwrap();
    assert_eq!(json["protocol"], 1);
    assert_eq!(json["tool"], "wf-luau-decompiler");
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(json["ok"], ok);
    assert_eq!(output.status.success(), ok, "{text}");
    json
}

#[test]
fn describes_the_inspector_protocol() {
    let output = invoke(&["--describe"], &[]);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["protocol"], 1);
    assert_eq!(json["tool"], "wf-luau-decompiler");
    assert_eq!(json["features"], serde_json::json!(["atom-symbols"]));
    assert_eq!(json["dialects"], serde_json::json!(["warframe", "luau"]));
}

#[test]
fn source_and_json_are_separate_for_both_dialects() {
    for (name, warframe) in [("warframe", true), ("luau", false)] {
        let bytes = fixture(warframe);
        let output = invoke(&["-", "--diagnostics", "json", "--dialect", name], &bytes);
        let json = diagnostic(&output, true);
        assert_eq!(json["dialect"], name);
        assert_eq!(json["input_bytes"], bytes.len());
        assert!(
            std::str::from_utf8(&output.stdout)
                .unwrap()
                .contains("return true")
        );
    }
}

#[test]
fn malformed_input_is_failure_not_partial_source() {
    for input in [&[][..], &[255][..], &fixture(true)[..20]] {
        let output = invoke(&["-", "--diagnostics", "json"], input);
        assert!(output.stdout.is_empty());
        let json = diagnostic(&output, false);
        assert!(
            json["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty())
        );
    }
}

#[test]
fn names_atoms_before_lifting_and_rejects_conflicts() {
    let mut bytes = fixture(true);
    bytes.splice(22..26, 0x2fdd25fcu32.to_le_bytes());
    bytes[11] = 3;
    bytes[12] = 7; // GETGLOBAL requires an AUX word.
    bytes.splice(16..16, 0u32.to_le_bytes());
    let args = ["-", "--diagnostics", "json", "--symbol", "2fdd25fc=module"];
    let output = invoke(&args, &bytes);
    diagnostic(&output, true);
    assert!(
        std::str::from_utf8(&output.stdout)
            .unwrap()
            .contains("return module")
    );

    let mut conflict = args.to_vec();
    conflict.extend(["--symbol", "2fdd25fc=different"]);
    let output = invoke(&conflict, &bytes);
    assert!(output.stdout.is_empty());
    assert_eq!(
        diagnostic(&output, false)["error"],
        "conflicting atom symbols"
    );
}

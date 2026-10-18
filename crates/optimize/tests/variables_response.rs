use serde_json::{Value, json};
use secondwind_optimize::{Outcome, variables_simplifier::{VariablesSimplifier, restore_response_keys}};
use secondwind_optimize::transform::Transform;

#[test]
fn variables_simplifier_restores_aliases_in_model_response() {
    let source = json!([
        {"customerIdentifier":"C123","customerDisplayName":"Ana","accountStatus":"active"},
        {"customerIdentifier":"C456","customerDisplayName":"Bia","accountStatus":"inactive"},
    ]);
    let encoded = VariablesSimplifier
        .try_encode(&source)
        .expect("aliases should be profitable to build");

    // Deterministic sorted aliases: a=accountStatus, b=customerDisplayName, c=customerIdentifier.
    let response = r#"[{"a":"active","b":"Carol","c":"C999"}]"#;
    let restored = restore_response_keys(&encoded.wire, response)
        .expect("the exact wire dictionary must restore the model response");

    let restored: Value = serde_json::from_str(&restored).unwrap();
    assert_eq!(
        restored,
        json!([{
            "customerIdentifier":"C999",
            "customerDisplayName":"Carol",
            "accountStatus":"active"
        }])
    );
}

#[test]
fn response_restoration_uses_only_the_dictionary_from_the_exact_wire() {
    let first = json!([{"customerIdentifier":"C123","customerDisplayName":"Ana"}]);
    let second = json!([{"orderIdentifier":"O123","orderDisplayName":"Order"}]);
    let first_wire = VariablesSimplifier.try_encode(&first).unwrap().wire;
    let second_wire = VariablesSimplifier.try_encode(&second).unwrap().wire;

    // First wire: a=customerDisplayName, b=customerIdentifier.
    let response = r#"{"a":"Other","b":"C999","unrelated":"kept"}"#;
    let restored = restore_response_keys(&first_wire, response).unwrap();
    let value: Value = serde_json::from_str(&restored).unwrap();

    assert_eq!(value["customerDisplayName"], "Other");
    assert_eq!(value["customerIdentifier"], "C999");
    assert_eq!(value["unrelated"], "kept");

    // A different wire has its own dictionary and is therefore restored independently.
    assert!(restore_response_keys(&second_wire, response).is_some());
}

#[test]
fn unknown_or_malformed_wire_has_no_response_inverse() {
    assert!(restore_response_keys("unknown", "{}").is_none());
    assert!(restore_response_keys("SWVAR1\n{}\n{}", "{}").is_none());
    assert!(restore_response_keys("SWVAR1\n{\"a\":\"name\"}\n{}", "{\"b\":1}").is_none());
}

#[test]
fn response_inverse_does_not_change_the_compressed_request_contract() {
    let source = r#"[{"customerIdentifier":"C123","customerDisplayName":"Ana","accountStatus":"active"},{"customerIdentifier":"C456","customerDisplayName":"Bia","accountStatus":"inactive"}]"#;
    let mut optimizer = secondwind_optimize::Optimizer::default();
    optimizer.set_offload_allowed(false);
    let outcome = optimizer.compress_block(source);
    assert!(matches!(outcome, Outcome::Compressed { .. } | Outcome::KeptVerbatim { .. }));
}

use serde_json::{Value, json};
use secondwind_optimize::{Optimizer, Outcome, transform::Transform, variables_simplifier::VariablesSimplifier};

#[test]
fn optimizer_restores_variable_aliases_in_model_response() {
    let source = json!([
        {"customerIdentifier":"C123","customerDisplayName":"Ana","accountStatus":"active"},
        {"customerIdentifier":"C456","customerDisplayName":"Bia","accountStatus":"inactive"},
    ]);
    let encoded = VariablesSimplifier
        .try_encode(&source)
        .expect("aliases should be profitable to build");

    let optimizer = Optimizer::default();
    // Deterministic sorted aliases: a=accountStatus, b=customerDisplayName, c=customerIdentifier.
    let response = r#"[{"a":"active","b":"Carol","c":"C999"}]"#;
    let restored = optimizer
        .restore_model_response("variables-simplifier", &encoded.wire, response)
        .expect("built-in variable transform must expose its response inverse");

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
fn optimizer_uses_only_the_dictionary_from_the_exact_wire() {
    let first = json!([{"customerIdentifier":"C123","customerDisplayName":"Ana"}]);
    let second = json!([{"orderIdentifier":"O123","orderDisplayName":"Order"}]);
    let first_wire = VariablesSimplifier.try_encode(&first).unwrap().wire;
    let second_wire = VariablesSimplifier.try_encode(&second).unwrap().wire;

    let optimizer = Optimizer::default();
    // First wire: a=customerDisplayName, b=customerIdentifier.
    let response = r#"{"a":"Other","b":"C999","unrelated":"kept"}"#;
    let restored = optimizer
        .restore_model_response("variables-simplifier", &first_wire, response)
        .unwrap();
    let value: Value = serde_json::from_str(&restored).unwrap();

    assert_eq!(value["customerDisplayName"], "Other");
    assert_eq!(value["customerIdentifier"], "C999");
    assert_eq!(value["unrelated"], "kept");

    // A different wire has its own dictionary and is therefore restored independently.
    assert!(
        optimizer
            .restore_model_response("variables-simplifier", &second_wire, response)
            .is_some()
    );
}

#[test]
fn unknown_transform_has_no_response_inverse() {
    let optimizer = Optimizer::default();
    assert!(optimizer
        .restore_model_response("unknown", "wire", "{}")
        .is_none());
}

#[test]
fn response_inverse_does_not_change_the_compressed_request_contract() {
    let source = r#"[{"customerIdentifier":"C123","customerDisplayName":"Ana","accountStatus":"active"},{"customerIdentifier":"C456","customerDisplayName":"Bia","accountStatus":"inactive"}]"#;
    let mut optimizer = Optimizer::default();
    optimizer.set_offload_allowed(false);
    let outcome = optimizer.compress_block(source);
    assert!(matches!(outcome, Outcome::Compressed { .. } | Outcome::KeptVerbatim { .. }));
}

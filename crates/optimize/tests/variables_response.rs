use secondwind_optimize::{Optimizer, Outcome, variables_simplifier::VariablesSimplifier};
use serde_json::{Value, json};
use secondwind_optimize::transform::Transform;

#[test]
fn optimizer_restores_variable_aliases_in_model_response() {
    let source = json!([
        {"customerIdentifier":"C123","customerDisplayName":"Ana","accountStatus":"active"},
        {"customerIdentifier":"C456","customerDisplayName":"Bia","accountStatus":"inactive"},
    ]);
    let encoded = VariablesSimplifier.try_encode(&source).expect("aliases should be profitable to build");

    let optimizer = Optimizer::default();
    let response = r#"[{"a":"C999","b":"Carol","c":"active"}]"#;
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
fn optimizer_does_not_restore_aliases_belonging_to_another_wire() {
    let first = json!([{"customerIdentifier":"C123","customerDisplayName":"Ana"}]);
    let second = json!([{"orderIdentifier":"O123","orderDisplayName":"Order"}]);
    let first_wire = VariablesSimplifier.try_encode(&first).unwrap().wire;
    let second_wire = VariablesSimplifier.try_encode(&second).unwrap().wire;

    let optimizer = Optimizer::default();
    let response = r#"{"a":"O999","b":"Other","unrelated":"kept"}"#;
    let restored = optimizer
        .restore_model_response("variables-simplifier", &first_wire, response)
        .unwrap();
    let value: Value = serde_json::from_str(&restored).unwrap();

    assert_eq!(value["a"], "C999" );
    assert_eq!(value["unrelated"], "kept");
    assert!(
        optimizer
            .restore_model_response("variables-simplifier", &second_wire, response)
            .is_some()
    );
}

#[test]
fn unknown_transform_has_no_response_inverse() {
    let optimizer = Optimizer::default();
    assert!(optimizer.restore_model_response("unknown", "wire", "{} ").is_none());
}

#[test]
fn response_inverse_does_not_change_the_compressed_request_contract() {
    let source = r#"[{"customerIdentifier":"C123","customerDisplayName":"Ana","accountStatus":"active"},{"customerIdentifier":"C456","customerDisplayName":"Bia","accountStatus":"inactive"}]"#;
    let mut optimizer = Optimizer::default();
    optimizer.set_offload_allowed(false);
    let outcome = optimizer.compress_block(source);
    assert!(matches!(outcome, Outcome::Compressed { .. } | Outcome::KeptVerbatim { .. }));
}

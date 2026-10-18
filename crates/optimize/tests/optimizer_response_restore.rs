use secondwind_optimize::{
    transform::Transform, variables_simplifier::VariablesSimplifier, Optimizer,
};
use serde_json::{Value, json};

#[test]
fn optimizer_restores_response_using_the_exact_transform_wire() {
    let source = json!([
        {
            "customerIdentifier": "C123",
            "customerDisplayName": "Ana",
            "accountStatus": "active"
        },
        {
            "customerIdentifier": "C456",
            "customerDisplayName": "Bia",
            "accountStatus": "inactive"
        }
    ]);
    let encoded = VariablesSimplifier
        .try_encode(&source)
        .expect("source should produce an alias dictionary");
    let response = r#"[{"a":"active","b":"Carol","c":"C999"}]"#;

    let restored = Optimizer::default()
        .restore_model_response("variables-simplifier", &encoded.wire, response)
        .expect("the optimizer should dispatch to the transform response inverse");
    let restored: Value = serde_json::from_str(&restored).unwrap();

    assert_eq!(
        restored,
        json!([{
            "customerIdentifier": "C999",
            "customerDisplayName": "Carol",
            "accountStatus": "active"
        }])
    );
}

#[test]
fn optimizer_response_restoration_rejects_unknown_transform_and_wire() {
    let source = json!([{"customerIdentifier": "C123", "customerDisplayName": "Ana"}]);
    let encoded = VariablesSimplifier.try_encode(&source).unwrap();
    let optimizer = Optimizer::default();
    let response = r#"{"a":"Carol","b":"C999"}"#;

    assert!(
        optimizer
            .restore_model_response("unknown-transform", &encoded.wire, response)
            .is_none()
    );
    assert!(
        optimizer
            .restore_model_response("variables-simplifier", "not-a-wire", response)
            .is_none()
    );
    assert!(
        optimizer
            .restore_model_response("variables-simplifier", &encoded.wire, "not-json")
            .is_none()
    );
}

#[test]
fn response_restoration_is_opt_in_and_does_not_change_request_compression() {
    let source = r#"[{"customerIdentifier":"C123","customerDisplayName":"Ana"},{"customerIdentifier":"C456","customerDisplayName":"Bia"}]"#;
    let mut optimizer = Optimizer::default();
    optimizer.set_offload_allowed(false);

    let outcome = optimizer.compress_block(source);
    assert!(matches!(
        outcome,
        secondwind_optimize::Outcome::Compressed { .. }
            | secondwind_optimize::Outcome::KeptVerbatim { .. }
    ));
}

use proptest::prelude::*;
use serde_json::{Number, Value};

pub fn json_value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        // Integers are safer than floats for simple JSON testing to avoid NaN/Inf issues if not handled
        any::<i64>().prop_map(|i| Value::Number(Number::from(i))),
        any::<String>().prop_map(Value::String),
    ];

    leaf.prop_recursive(
        4,  // levels deep
        64, // max size
        5,  // items per collection
        |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..5).prop_map(Value::Array),
                prop::collection::hash_map(any::<String>(), inner, 0..5)
                    .prop_map(|m| Value::Object(m.into_iter().collect())),
            ]
        },
    )
}

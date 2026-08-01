use serde_json::Value;

pub fn compatibility_score(schema: Option<&Value>, body: Option<&Value>) -> (f32, bool) {
    let Some(schema) = schema else {
        return (1.0, true);
    };
    let Some(body) = body else {
        return (0.0, false);
    };

    let mut checks = 0.0f32;
    let mut passed = 0.0f32;

    if let Some(expected) = schema.get("type").and_then(Value::as_str) {
        checks += 1.0;
        if matches_type(body, expected) {
            passed += 1.0;
        } else {
            return (0.0, false);
        }
    }

    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        let object = body.as_object();
        for field in required.iter().filter_map(Value::as_str) {
            checks += 1.0;
            if object.is_some_and(|map| map.contains_key(field)) {
                passed += 1.0;
            } else {
                return ((passed / checks).clamp(0.0, 1.0), false);
            }
        }
    }

    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        if let Some(object) = body.as_object() {
            for (name, property) in properties {
                if let Some(value) = object.get(name) {
                    if let Some(expected) = property.get("type").and_then(Value::as_str) {
                        checks += 1.0;
                        if matches_type(value, expected) {
                            passed += 1.0;
                        }
                    }
                }
            }
        }
    }

    if checks <= f32::EPSILON {
        (1.0, true)
    } else {
        ((passed / checks).clamp(0.0, 1.0), true)
    }
}

fn matches_type(value: &Value, expected: &str) -> bool {
    match expected {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.is_number(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn required_field_rejects_incompatible_body() {
        let schema = serde_json::json!({"type":"object","required":["invoice_number"]});
        assert!(!compatibility_score(Some(&schema), Some(&serde_json::json!({"text":"x"}))).1);
    }
}

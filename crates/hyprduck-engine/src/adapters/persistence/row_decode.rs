//! Internal helpers extracted from the engine facade module.

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use std::collections::HashMap;

use graphqlite::{Row, Value};

pub(super) fn object_string(properties: &HashMap<String, Value>, key: &str) -> String {
    match properties.get(key) {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Integer(value)) => value.to_string(),
        Some(Value::Float(value)) => value.to_string(),
        _ => String::new(),
    }
}

pub(super) fn object_i64(properties: &HashMap<String, Value>, key: &str) -> i64 {
    match properties.get(key) {
        Some(Value::Integer(value)) => *value,
        Some(Value::Float(value)) => *value as i64,
        Some(Value::String(value)) => value.parse::<i64>().unwrap_or_default(),
        _ => 0,
    }
}

pub(super) fn object_optional_f32(properties: &HashMap<String, Value>, key: &str) -> Option<f32> {
    match properties.get(key) {
        Some(Value::Float(value)) => Some(*value as f32),
        Some(Value::Integer(value)) => Some(*value as f32),
        Some(Value::String(value)) if value.is_empty() => None,
        Some(Value::String(value)) => value.parse::<f32>().ok(),
        _ => None,
    }
}

pub(super) fn object_string_array(properties: &HashMap<String, Value>, key: &str) -> Vec<String> {
    match properties.get(key) {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(|value| match value {
                Value::String(value) => Some(value.clone()),
                _ => None,
            })
            .collect(),
        Some(Value::String(value)) => {
            serde_json::from_str::<Vec<String>>(value).unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

pub(super) fn non_empty_string(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub(super) fn row_string(row: &Row, column: &str) -> Result<String> {
    match row.get_value(column) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(Value::Integer(value)) => Ok(value.to_string()),
        Some(Value::Float(value)) => Ok(value.to_string()),
        Some(Value::Bool(value)) => Ok(value.to_string()),
        Some(Value::Null) | None => Ok(String::new()),
        Some(other) => Err(anyhow!("expected scalar column {column}, got {other:?}")),
    }
}

pub(super) fn row_i64(row: &Row, column: &str) -> Result<i64> {
    match row.get_value(column) {
        Some(Value::Integer(value)) => Ok(*value),
        Some(Value::Float(value)) => Ok(*value as i64),
        Some(Value::String(value)) if value.trim().is_empty() => Ok(0),
        Some(Value::String(value)) => value
            .parse::<i64>()
            .with_context(|| format!("failed parsing integer column {column}")),
        Some(Value::Null) | None => Ok(0),
        Some(other) => Err(anyhow!("expected integer column {column}, got {other:?}")),
    }
}

pub(super) fn row_string_array(row: &Row, column: &str) -> Result<Vec<String>> {
    match row.get_value(column) {
        Some(Value::Array(values)) => Ok(values
            .iter()
            .filter_map(|value| match value {
                Value::String(value) => Some(value.clone()),
                _ => None,
            })
            .collect()),
        Some(Value::String(value)) => {
            Ok(serde_json::from_str::<Vec<String>>(value).unwrap_or_default())
        }
        Some(Value::Null) | None => Ok(Vec::new()),
        Some(other) => Err(anyhow!(
            "expected string array column {column}, got {other:?}"
        )),
    }
}

pub(super) fn json_string_slug<T: Serialize>(value: &T) -> Result<String> {
    let value = serde_json::to_value(value).context("failed encoding slug value")?;
    value
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("slug value did not encode as a JSON string"))
}

pub(super) fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(super) fn sql_optional_literal(value: Option<&str>) -> String {
    value.map(sql_literal).unwrap_or_else(|| "NULL".into())
}

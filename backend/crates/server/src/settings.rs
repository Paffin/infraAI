//! Runtime settings: a typed registry (`defaults/settings.json`) + values in PostgreSQL.
//! The admin panel renders its forms from the registry, so adding a setting never needs UI work.
//! Secrets are encrypted at rest and never returned by the API.

use crate::crypto::Keys;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingDef {
    pub key: String,
    pub group: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub default: Value,
    pub label: Value,
    #[serde(default)]
    pub help: Option<Value>,
    #[serde(default)]
    pub options: Option<Vec<String>>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
}

pub const SETTINGS_JSON: &str = include_str!("../defaults/settings.json");

pub fn registry() -> &'static Vec<SettingDef> {
    static R: OnceLock<Vec<SettingDef>> = OnceLock::new();
    R.get_or_init(|| serde_json::from_str(SETTINGS_JSON).expect("embedded settings.json is valid"))
}

pub fn def(key: &str) -> Option<&'static SettingDef> {
    registry().iter().find(|d| d.key == key)
}

/// Validate and normalise a value for a setting. Returns the value to store.
pub fn validate(def: &SettingDef, v: &Value) -> Result<Value, String> {
    match def.ty.as_str() {
        "string" | "model" | "secret" => match v {
            Value::String(s) => Ok(Value::String(s.trim().to_string())),
            _ => Err("expected a string".into()),
        },
        "enum" => {
            let s = v.as_str().ok_or("expected a string")?;
            if def.options.as_ref().map(|o| o.iter().any(|x| x == s)).unwrap_or(false) {
                Ok(json!(s))
            } else {
                Err(format!("must be one of: {}", def.options.clone().unwrap_or_default().join(", ")))
            }
        }
        "number" => {
            let n = match v {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.trim().parse::<f64>().ok(),
                _ => None,
            }
            .ok_or("expected a number")?;
            if let Some(min) = def.min {
                if n < min {
                    return Err(format!("must be ≥ {min}"));
                }
            }
            if let Some(max) = def.max {
                if n > max {
                    return Err(format!("must be ≤ {max}"));
                }
            }
            Ok(json!(n))
        }
        "boolean" => v.as_bool().map(|b| json!(b)).ok_or_else(|| "expected true or false".to_string()),
        "string_list" => {
            let a = v.as_array().ok_or("expected a list of strings")?;
            let mut out = vec![];
            for x in a {
                let s = x.as_str().ok_or("expected a list of strings")?.trim();
                if !s.is_empty() {
                    out.push(json!(s));
                }
            }
            Ok(Value::Array(out))
        }
        "json" => {
            if v.is_object() || v.is_array() {
                Ok(v.clone())
            } else {
                Err("expected a JSON object".into())
            }
        }
        other => Err(format!("unknown setting type {other}")),
    }
}

#[derive(Clone, Default)]
pub struct Settings {
    values: HashMap<String, Value>,
    /// keys whose value comes from the database (vs. default / environment)
    from_db: HashMap<String, bool>,
}

impl Settings {
    /// The shipped defaults of every setting – no database involved.
    pub fn defaults() -> Self {
        Settings { values: registry().iter().map(|d| (d.key.clone(), d.default.clone())).collect(), from_db: HashMap::new() }
    }

    /// The same settings with `key` set to `value` (validated like an administrator's change).
    pub fn with(mut self, key: &str, value: Value) -> Result<Self, String> {
        let d = def(key).ok_or_else(|| format!("unknown setting '{key}'"))?;
        let v = validate(d, &value).map_err(|e| format!("{key}: {e}"))?;
        self.values.insert(key.to_string(), v);
        self.from_db.insert(key.to_string(), true);
        Ok(self)
    }

    pub async fn load(pool: &PgPool, keys: &Keys) -> Result<Self, sqlx::Error> {
        let rows: Vec<(String, Value, bool)> = sqlx::query_as("select key, value, encrypted from settings").fetch_all(pool).await?;
        let mut values: HashMap<String, Value> = registry().iter().map(|d| (d.key.clone(), d.default.clone())).collect();
        let mut from_db = HashMap::new();
        for (k, v, enc) in rows {
            if def(&k).is_none() {
                continue; // removed setting
            }
            let v = if enc {
                match v.as_str().and_then(|s| keys.decrypt(s)) {
                    Some(plain) => Value::String(plain),
                    None => {
                        tracing::warn!(key = %k, "cannot decrypt secret setting (APP_SECRET changed?)");
                        continue;
                    }
                }
            } else {
                v
            };
            values.insert(k.clone(), v);
            from_db.insert(k, true);
        }
        Ok(Settings { values, from_db })
    }

    pub fn get(&self, key: &str) -> Value {
        self.values.get(key).cloned().or_else(|| def(key).map(|d| d.default.clone())).unwrap_or(Value::Null)
    }
    pub fn str(&self, key: &str) -> String {
        self.get(key).as_str().unwrap_or("").to_string()
    }
    pub fn num(&self, key: &str) -> f64 {
        self.get(key).as_f64().unwrap_or(0.0)
    }
    pub fn boolean(&self, key: &str) -> bool {
        self.get(key).as_bool().unwrap_or(false)
    }
    pub fn list(&self, key: &str) -> Vec<String> {
        self.get(key).as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
    }
    pub fn is_from_db(&self, key: &str) -> bool {
        self.from_db.get(key).copied().unwrap_or(false)
    }

    /// Settings as shown to administrators: secrets are masked.
    pub fn for_admin(&self, env_secret_present: &dyn Fn(&str) -> bool) -> Vec<Value> {
        registry()
            .iter()
            .map(|d| {
                let mut o = serde_json::to_value(d).unwrap_or(Value::Null);
                if d.ty == "secret" {
                    let set_db = self.is_from_db(&d.key) && !self.str(&d.key).is_empty();
                    o["value"] = Value::Null;
                    o["secretSet"] = json!(set_db);
                    o["secretSource"] = json!(if set_db {
                        "database"
                    } else if env_secret_present(&d.key) {
                        "environment"
                    } else {
                        "none"
                    });
                } else {
                    o["value"] = self.get(&d.key);
                    o["modified"] = json!(self.is_from_db(&d.key));
                }
                o
            })
            .collect()
    }
}

/// Integral floats are stored as integers (`12.0` → `12`) so that a value equal to its default is recognised.
fn canonical(v: Value) -> Value {
    match v.as_f64() {
        Some(f) if v.is_f64() && f.fract() == 0.0 && f.abs() < 9e15 => json!(f as i64),
        _ => v,
    }
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// Persist changes. Secrets are encrypted; an empty secret removes the stored value.
pub async fn save(pool: &PgPool, keys: &Keys, changes: &serde_json::Map<String, Value>, actor: &str) -> Result<Vec<String>, String> {
    let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
    let mut changed = vec![];
    for (k, v) in changes {
        let d = def(k).ok_or_else(|| format!("unknown setting '{k}'"))?;
        let norm = canonical(validate(d, v).map_err(|e| format!("{k}: {e}"))?);
        if d.ty == "secret" {
            let s = norm.as_str().unwrap_or("");
            if s.is_empty() {
                sqlx::query("delete from settings where key = $1").bind(k).execute(&mut *tx).await.map_err(|e| e.to_string())?;
            } else {
                sqlx::query("insert into settings (key, value, encrypted, updated_by) values ($1, $2, true, $3) on conflict (key) do update set value = $2, encrypted = true, updated_by = $3, updated_at = now()")
                    .bind(k)
                    .bind(json!(keys.encrypt(s)))
                    .bind(actor)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        } else if same_value(&norm, &d.default) {
            sqlx::query("delete from settings where key = $1").bind(k).execute(&mut *tx).await.map_err(|e| e.to_string())?;
        } else {
            sqlx::query("insert into settings (key, value, encrypted, updated_by) values ($1, $2, false, $3) on conflict (key) do update set value = $2, encrypted = false, updated_by = $3, updated_at = now()")
                .bind(k)
                .bind(&norm)
                .bind(actor)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
        }
        changed.push(k.clone());
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_valid_and_defaults_validate() {
        let mut keys = std::collections::HashSet::new();
        for d in registry() {
            assert!(keys.insert(d.key.clone()), "duplicate key {}", d.key);
            if d.ty != "secret" {
                validate(d, &d.default).unwrap_or_else(|e| panic!("default of {} invalid: {e}", d.key));
            }
        }
    }

    #[test]
    fn number_bounds_and_enums_are_enforced() {
        let d = def("auth.sessionTtlHours").unwrap();
        assert!(validate(d, &json!(0)).is_err());
        assert!(validate(d, &json!("8")).is_ok());
        let e = def("llm.structuredOutput").unwrap();
        assert!(validate(e, &json!("nope")).is_err());
        assert!(validate(e, &json!("json_object")).is_ok());
    }
}

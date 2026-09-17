//! Settings schema subset (see "Esquema de ajustes" in `docs/plugins.md`).

use std::collections::HashSet;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

use crate::manifest::Issue;

const MAX_PROPERTY_KEY_LEN: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PropertyType {
    String,
    Number,
    Integer,
    Boolean,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    Url,
    Email,
    Password,
    Multiline,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SchemaProperty {
    #[serde(rename = "type")]
    pub kind: PropertyType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enum_labels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<Format>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub secret: bool,
}

impl SchemaProperty {
    /// `secret: true` or `format: "password"`.
    pub fn is_secret(&self) -> bool {
        self.secret || self.format == Some(Format::Password)
    }

    /// Validates one (non-null) value against this property. Returns `(code, message)` on error.
    fn check_value(&self, value: &Value) -> std::result::Result<(), (&'static str, String)> {
        match self.kind {
            PropertyType::String => {
                let Some(text) = value.as_str() else {
                    return Err(("type", "must be a string".into()));
                };
                let chars = text.chars().count();
                if let Some(min) = self.min_length
                    && chars < min as usize
                {
                    return Err(("min_length", format!("must have at least {min} characters")));
                }
                if let Some(max) = self.max_length
                    && chars > max as usize
                {
                    return Err(("max_length", format!("must have at most {max} characters")));
                }
                if let Some(pattern) = &self.pattern {
                    match regex::Regex::new(pattern) {
                        Ok(re) if re.is_match(text) => {}
                        Ok(_) => return Err(("pattern", format!("must match the pattern {pattern}"))),
                        Err(_) => return Err(("pattern", "the schema pattern is invalid".into())),
                    }
                }
                match self.format {
                    Some(Format::Url) if !is_http_url(text) => {
                        return Err(("format", "must be an http:// or https:// URL".into()));
                    }
                    Some(Format::Email) if !is_email(text) => {
                        return Err(("format", "must be an email address".into()));
                    }
                    _ => {}
                }
            }
            PropertyType::Number | PropertyType::Integer => {
                let Some(number) = value.as_f64().filter(|_| value.is_number()) else {
                    return Err(("type", "must be a number".into()));
                };
                if self.kind == PropertyType::Integer && (number.fract() != 0.0 || !number.is_finite()) {
                    return Err(("type", "must be an integer".into()));
                }
                if let Some(min) = self.minimum
                    && number < min
                {
                    return Err(("minimum", format!("must be >= {}", format_number(min))));
                }
                if let Some(max) = self.maximum
                    && number > max
                {
                    return Err(("maximum", format!("must be <= {}", format_number(max))));
                }
            }
            PropertyType::Boolean => {
                if !value.is_boolean() {
                    return Err(("type", "must be a boolean".into()));
                }
            }
        }
        if let Some(options) = &self.enum_values
            && !options.iter().any(|option| json_equal(option, value))
        {
            return Err(("enum", "must be one of the allowed values".into()));
        }
        Ok(())
    }
}

/// Serialized with an extra `propertyOrder` array (the UI renders fields in manifest order).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsSchema {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub properties: IndexMap<String, SchemaProperty>,
    #[serde(default)]
    pub required: Vec<String>,
}

impl Serialize for SettingsSchema {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire<'a> {
            #[serde(rename = "type")]
            kind: &'a str,
            title: &'a Option<String>,
            description: &'a Option<String>,
            properties: &'a IndexMap<String, SchemaProperty>,
            property_order: Vec<&'a str>,
            required: &'a Vec<String>,
        }
        Wire {
            kind: &self.kind,
            title: &self.title,
            description: &self.description,
            properties: &self.properties,
            property_order: self.properties.keys().map(String::as_str).collect(),
            required: &self.required,
        }
        .serialize(serializer)
    }
}

/// One invalid field in submitted values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldError {
    pub field: String,
    /// `required`, `type`, `enum`, `minimum`, `maximum`, `min_length`, `max_length`, `pattern`, `format`, `unknown_field`.
    pub code: String,
    pub message: String,
}

impl FieldError {
    fn new(field: &str, code: &str, message: impl Into<String>) -> Self {
        Self { field: field.to_owned(), code: code.to_owned(), message: message.into() }
    }
}

/// Secret fields to write into / remove from the vault.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretChanges {
    pub set: Vec<(String, String)>,
    pub remove: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ValidatedSettings {
    /// Complete non-secret values (defaults applied for missing optional fields).
    pub values: Map<String, Value>,
    pub secrets: SecretChanges,
}

impl SettingsSchema {
    /// Parses a schema file. Returns a `settings_schema_invalid` issue on error.
    pub fn parse(json: &str) -> std::result::Result<Self, Issue> {
        serde_json::from_str(json).map_err(|err| {
            Issue::error(
                "settings_schema_invalid",
                format!("settings schema is invalid: {err} (line {}, column {})", err.line(), err.column()),
                None,
            )
        })
    }

    /// Consistency checks: `type` is "object", required keys exist, defaults/enums match their
    /// type, `minimum <= maximum`, `pattern` compiles, `enumLabels` length matches `enum`,
    /// secret properties are strings without defaults.
    ///
    /// Also rejected: property keys outside `^[A-Za-z0-9_.-]{1,64}$`, empty enums, and keywords
    /// that don't apply to the property type (e.g. `minLength` on a number). Issue fields are
    /// schema paths such as `properties.retries.maximum`.
    pub fn check(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        let mut push = |field: String, message: String| {
            issues.push(Issue::error("settings_schema_invalid", message, Some(&field)));
        };

        if self.kind != "object" {
            push("type".into(), format!("schema type must be \"object\", found {:?}", self.kind));
        }
        for (index, key) in self.required.iter().enumerate() {
            if !self.properties.contains_key(key) {
                push(format!("required[{index}]"), format!("required property {key:?} is not declared"));
            }
        }

        for (key, property) in &self.properties {
            let base = format!("properties.{key}");
            if !is_valid_property_key(key) {
                push(base.clone(), format!("property key {key:?} must match ^[A-Za-z0-9_.-]{{1,64}}$"));
            }
            let is_string = property.kind == PropertyType::String;
            let is_numeric = matches!(property.kind, PropertyType::Number | PropertyType::Integer);

            if property.is_secret() {
                if !is_string {
                    push(format!("{base}.type"), "secret properties must be strings".into());
                }
                if property.default.is_some() {
                    push(format!("{base}.default"), "secret properties cannot have a default".into());
                }
                if property.enum_values.is_some() {
                    push(format!("{base}.enum"), "secret properties cannot have an enum".into());
                }
            }
            if !is_string {
                for (keyword, present) in [
                    ("minLength", property.min_length.is_some()),
                    ("maxLength", property.max_length.is_some()),
                    ("pattern", property.pattern.is_some()),
                    ("format", property.format.is_some()),
                ] {
                    if present {
                        push(format!("{base}.{keyword}"), format!("{keyword} only applies to string properties"));
                    }
                }
            }
            if !is_numeric {
                for (keyword, present) in
                    [("minimum", property.minimum.is_some()), ("maximum", property.maximum.is_some())]
                {
                    if present {
                        push(format!("{base}.{keyword}"), format!("{keyword} only applies to numeric properties"));
                    }
                }
            }
            if let (Some(min), Some(max)) = (property.minimum, property.maximum)
                && min > max
            {
                push(format!("{base}.minimum"), "minimum must be <= maximum".into());
            }
            if let (Some(min), Some(max)) = (property.min_length, property.max_length)
                && min > max
            {
                push(format!("{base}.minLength"), "minLength must be <= maxLength".into());
            }
            if let Some(pattern) = &property.pattern
                && regex::Regex::new(pattern).is_err()
            {
                push(format!("{base}.pattern"), format!("pattern {pattern:?} is not a valid regular expression"));
            }

            match (&property.enum_values, &property.enum_labels) {
                (Some(options), labels) => {
                    if options.is_empty() {
                        push(format!("{base}.enum"), "enum must not be empty".into());
                    }
                    let without_enum = SchemaProperty { enum_values: None, ..property.clone() };
                    for (index, option) in options.iter().enumerate() {
                        if option.is_null() || without_enum.check_value(option).is_err() {
                            push(format!("{base}.enum[{index}]"), "enum value does not match the property type".into());
                        }
                    }
                    if let Some(labels) = labels
                        && labels.len() != options.len()
                    {
                        push(format!("{base}.enumLabels"), "enumLabels must have one label per enum value".into());
                    }
                }
                (None, Some(_)) => push(format!("{base}.enumLabels"), "enumLabels requires enum".into()),
                (None, None) => {}
            }

            if let Some(default) = &property.default
                && !property.is_secret()
                && (default.is_null() || property.check_value(default).is_err())
            {
                push(format!("{base}.default"), "default does not satisfy the property".into());
            }
        }
        issues
    }

    pub fn secret_keys(&self) -> Vec<&str> {
        self.properties.iter().filter(|(_, p)| p.is_secret()).map(|(k, _)| k.as_str()).collect()
    }

    /// Non-secret defaults.
    pub fn defaults(&self) -> Map<String, Value> {
        self.properties
            .iter()
            .filter(|(_, property)| !property.is_secret())
            .filter_map(|(key, property)| property.default.clone().map(|value| (key.clone(), value)))
            .collect()
    }

    /// Validates submitted values.
    ///
    /// - Non-secret fields: absent → keep `current` value (or default); `null` → unset (error if
    ///   required); otherwise validated against the property.
    /// - Secret fields: string → set (validated); `null` → remove (error if required); absent → keep.
    ///   A required secret is satisfied when it is in `existing_secrets` and not removed.
    /// - Unknown keys → `unknown_field` error.
    ///
    /// Details: an unset optional field falls back to its default (or is omitted). For required
    /// fields an empty string counts as missing. For secrets an empty string means "remove".
    /// Current values that no longer satisfy the schema are reported like submitted ones.
    pub fn validate_values(
        &self,
        submitted: &Map<String, Value>,
        current: &Map<String, Value>,
        existing_secrets: &HashSet<String>,
    ) -> std::result::Result<ValidatedSettings, Vec<FieldError>> {
        let mut errors = Vec::new();
        let mut result = ValidatedSettings::default();
        let required: HashSet<&str> = self.required.iter().map(String::as_str).collect();

        for key in submitted.keys() {
            if !self.properties.contains_key(key) {
                errors.push(FieldError::new(key, "unknown_field", "is not declared in the settings schema"));
            }
        }

        for (key, property) in &self.properties {
            let is_required = required.contains(key.as_str());
            let submitted_value = submitted.get(key);

            if property.is_secret() {
                match submitted_value {
                    None => {
                        if is_required && !existing_secrets.contains(key) {
                            errors.push(FieldError::new(key, "required", "is required"));
                        }
                    }
                    Some(Value::Null) => {
                        if is_required {
                            errors.push(FieldError::new(key, "required", "is required"));
                        } else {
                            result.secrets.remove.push(key.clone());
                        }
                    }
                    Some(Value::String(text)) if text.is_empty() => {
                        if is_required {
                            errors.push(FieldError::new(key, "required", "is required"));
                        } else {
                            result.secrets.remove.push(key.clone());
                        }
                    }
                    Some(value) => match property.check_value(value) {
                        Ok(()) => {
                            let text = value.as_str().unwrap_or_default().to_owned();
                            result.secrets.set.push((key.clone(), text));
                        }
                        Err((code, message)) => errors.push(FieldError::new(key, code, message)),
                    },
                }
                continue;
            }

            let value = match submitted_value {
                Some(Value::Null) => property.default.clone(),
                Some(value) => Some(value.clone()),
                None => current.get(key).filter(|v| !v.is_null()).cloned().or_else(|| property.default.clone()),
            };

            match value {
                None => {
                    if is_required {
                        errors.push(FieldError::new(key, "required", "is required"));
                    }
                }
                Some(Value::String(text)) if text.is_empty() && is_required => {
                    errors.push(FieldError::new(key, "required", "is required"));
                }
                Some(value) => match property.check_value(&value) {
                    Ok(()) => {
                        result.values.insert(key.clone(), value);
                    }
                    Err((code, message)) => errors.push(FieldError::new(key, code, message)),
                },
            }
        }

        if errors.is_empty() { Ok(result) } else { Err(errors) }
    }
}

fn is_valid_property_key(key: &str) -> bool {
    (1..=MAX_PROPERTY_KEY_LEN).contains(&key.len())
        && key.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

fn is_http_url(text: &str) -> bool {
    url::Url::parse(text)
        .map(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some_and(|h| !h.is_empty()))
        .unwrap_or(false)
}

fn is_email(text: &str) -> bool {
    if text.chars().any(char::is_whitespace) {
        return false;
    }
    let mut parts = text.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains("..")
}

/// JSON equality where numbers compare by value (`1` == `1.0`).
fn json_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 { format!("{}", value as i64) } else { value.to_string() }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Raw text on purpose: `json!` sorts keys, while real schema files keep their order.
    fn example_schema() -> SettingsSchema {
        SettingsSchema::parse(
            r#"{
                "type": "object",
                "title": "Ajustes de Hola",
                "properties": {
                    "endpoint": { "type": "string", "title": "URL", "format": "url", "default": "https://api.example.com" },
                    "token": { "type": "string", "title": "Token", "secret": true, "minLength": 10 },
                    "retries": { "type": "integer", "title": "Reintentos", "minimum": 0, "maximum": 10, "default": 3 },
                    "mode": { "type": "string", "title": "Modo", "enum": ["rapido", "seguro"], "enumLabels": ["Rápido", "Seguro"] },
                    "notify": { "type": "boolean", "title": "Notificar", "default": true },
                    "notes": { "type": "string", "title": "Notas", "format": "multiline", "maxLength": 20 }
                },
                "required": ["endpoint"]
            }"#,
        )
        .unwrap()
    }

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    fn error_codes(errors: &[FieldError]) -> Vec<(&str, &str)> {
        errors.iter().map(|e| (e.field.as_str(), e.code.as_str())).collect()
    }

    #[test]
    fn parses_example_and_preserves_order() {
        let schema = example_schema();
        assert!(schema.check().is_empty(), "{:?}", schema.check());
        assert_eq!(
            schema.properties.keys().collect::<Vec<_>>(),
            ["endpoint", "token", "retries", "mode", "notify", "notes"]
        );
        assert_eq!(schema.secret_keys(), ["token"]);

        let wire = serde_json::to_value(&schema).unwrap();
        assert_eq!(wire["propertyOrder"], json!(["endpoint", "token", "retries", "mode", "notify", "notes"]));
        assert_eq!(wire["properties"]["mode"]["enumLabels"], json!(["Rápido", "Seguro"]));
        assert_eq!(wire["properties"]["retries"]["type"], "integer");
        assert!(wire["properties"]["retries"].get("pattern").is_none());
    }

    #[test]
    fn parse_rejects_unknown_keywords_and_types() {
        let err = SettingsSchema::parse(r#"{"type":"object","properties":{"a":{"type":"string","minlength":1}}}"#)
            .unwrap_err();
        assert_eq!(err.code, "settings_schema_invalid");
        assert!(SettingsSchema::parse(r#"{"type":"object","properties":{"a":{"type":"array"}}}"#).is_err());
        assert!(SettingsSchema::parse("[]").is_err());
    }

    #[test]
    fn defaults_skip_secrets() {
        assert_eq!(
            example_schema().defaults(),
            map(json!({"endpoint": "https://api.example.com", "retries": 3, "notify": true}))
        );
    }

    #[test]
    fn check_reports_inconsistencies() {
        let schema = SettingsSchema::parse(
            &json!({
                "type": "array",
                "properties": {
                    "bad key": { "type": "string" },
                    "secretNum": { "type": "integer", "secret": true, "default": 1 },
                    "pw": { "type": "string", "format": "password", "default": "x" },
                    "range": { "type": "number", "minimum": 5, "maximum": 1 },
                    "len": { "type": "string", "minLength": 5, "maxLength": 1, "pattern": "(" },
                    "boolMin": { "type": "boolean", "minimum": 1, "maxLength": 3 },
                    "choice": { "type": "integer", "enum": [1, "two"], "enumLabels": ["uno"] },
                    "empty": { "type": "string", "enum": [] },
                    "labelsOnly": { "type": "string", "enumLabels": ["x"] },
                    "badDefault": { "type": "integer", "minimum": 0, "default": -1 },
                    "enumDefault": { "type": "string", "enum": ["a"], "default": "b" }
                },
                "required": ["missing"]
            })
            .to_string(),
        )
        .unwrap();
        let fields: Vec<String> = schema.check().into_iter().map(|i| i.field.unwrap()).collect();
        for expected in [
            "type",
            "required[0]",
            "properties.bad key",
            "properties.secretNum.type",
            "properties.secretNum.default",
            "properties.pw.default",
            "properties.range.minimum",
            "properties.len.minLength",
            "properties.len.pattern",
            "properties.boolMin.maxLength",
            "properties.boolMin.minimum",
            "properties.choice.enum[1]",
            "properties.choice.enumLabels",
            "properties.empty.enum",
            "properties.labelsOnly.enumLabels",
            "properties.badDefault.default",
            "properties.enumDefault.default",
        ] {
            assert!(fields.iter().any(|f| f == expected), "missing {expected} in {fields:?}");
        }
    }

    #[test]
    fn validates_full_submission() {
        let schema = example_schema();
        let result = schema
            .validate_values(
                &map(json!({
                    "endpoint": "https://backups.example.com/api",
                    "token": "0123456789abc",
                    "retries": 5,
                    "mode": "seguro",
                    "notify": false,
                    "notes": "hola"
                })),
                &Map::new(),
                &HashSet::new(),
            )
            .unwrap();
        assert_eq!(
            result.values,
            map(json!({
                "endpoint": "https://backups.example.com/api",
                "retries": 5,
                "mode": "seguro",
                "notify": false,
                "notes": "hola"
            }))
        );
        assert_eq!(result.secrets.set, vec![("token".to_owned(), "0123456789abc".to_owned())]);
        assert!(result.secrets.remove.is_empty());
    }

    #[test]
    fn collects_every_field_error() {
        let schema = example_schema();
        let errors = schema
            .validate_values(
                &map(json!({
                    "endpoint": "ftp://example.com",
                    "token": "short",
                    "retries": 2.5,
                    "mode": "lento",
                    "notify": "yes",
                    "notes": "x".repeat(21),
                    "extra": 1
                })),
                &Map::new(),
                &HashSet::new(),
            )
            .unwrap_err();
        assert_eq!(
            error_codes(&errors),
            vec![
                ("extra", "unknown_field"),
                ("endpoint", "format"),
                ("token", "min_length"),
                ("retries", "type"),
                ("mode", "enum"),
                ("notify", "type"),
                ("notes", "max_length"),
            ]
        );
        // Secret values never leak into messages.
        assert!(errors.iter().all(|e| !e.message.contains("short")));

        let errors = schema.validate_values(&map(json!({"retries": 11})), &Map::new(), &HashSet::new()).unwrap_err();
        assert_eq!(error_codes(&errors), vec![("retries", "maximum")]);
        assert_eq!(errors[0].message, "must be <= 10");
        let errors = schema
            .validate_values(&map(json!({"retries": -1, "retries_x": null})), &Map::new(), &HashSet::new())
            .unwrap_err();
        assert_eq!(error_codes(&errors), vec![("retries_x", "unknown_field"), ("retries", "minimum")]);
    }

    #[test]
    fn absent_keeps_current_and_null_resets_to_default() {
        let schema = example_schema();
        let current = map(json!({"endpoint": "https://old.example.com", "retries": 7, "mode": "rapido"}));

        let kept = schema.validate_values(&Map::new(), &current, &HashSet::new()).unwrap();
        assert_eq!(
            kept.values,
            map(json!({"endpoint": "https://old.example.com", "retries": 7, "mode": "rapido", "notify": true}))
        );

        let reset =
            schema.validate_values(&map(json!({"retries": null, "mode": null})), &current, &HashSet::new()).unwrap();
        assert_eq!(reset.values["retries"], json!(3));
        assert!(!reset.values.contains_key("mode"));

        // Required field without default cannot be unset; empty string counts as missing.
        let schema_no_default = SettingsSchema::parse(
            r#"{"type":"object","properties":{"name":{"type":"string"},"opt":{"type":"string"}},"required":["name"]}"#,
        )
        .unwrap();
        let errors = schema_no_default.validate_values(&Map::new(), &Map::new(), &HashSet::new()).unwrap_err();
        assert_eq!(error_codes(&errors), vec![("name", "required")]);
        let errors =
            schema_no_default.validate_values(&map(json!({"name": ""})), &Map::new(), &HashSet::new()).unwrap_err();
        assert_eq!(error_codes(&errors), vec![("name", "required")]);
        let errors = schema_no_default
            .validate_values(&map(json!({"name": null})), &map(json!({"name": "x"})), &HashSet::new())
            .unwrap_err();
        assert_eq!(error_codes(&errors), vec![("name", "required")]);
        let ok = schema_no_default.validate_values(&map(json!({"name": "a", "opt": ""})), &Map::new(), &HashSet::new());
        assert_eq!(ok.unwrap().values, map(json!({"name": "a", "opt": ""})));

        // Invalid current values are reported.
        let errors = schema.validate_values(&Map::new(), &map(json!({"mode": "old"})), &HashSet::new()).unwrap_err();
        assert_eq!(error_codes(&errors), vec![("mode", "enum")]);
    }

    #[test]
    fn secret_keep_set_remove_and_required() {
        let schema = SettingsSchema::parse(
            r#"{"type":"object","properties":{
                "token":{"type":"string","secret":true},
                "pw":{"type":"string","format":"password","minLength":4}
            },"required":["token"]}"#,
        )
        .unwrap();
        let stored: HashSet<String> = ["token".to_owned(), "pw".to_owned()].into();
        let none: HashSet<String> = HashSet::new();

        // Keep.
        let kept = schema.validate_values(&Map::new(), &Map::new(), &stored).unwrap();
        assert_eq!(kept.secrets, SecretChanges::default());
        assert!(kept.values.is_empty());

        // Required secret missing.
        assert_eq!(
            error_codes(&schema.validate_values(&Map::new(), &Map::new(), &none).unwrap_err()),
            vec![("token", "required")]
        );

        // Set and remove.
        let changed =
            schema.validate_values(&map(json!({"token": "new-token", "pw": null})), &Map::new(), &stored).unwrap();
        assert_eq!(changed.secrets.set, vec![("token".to_owned(), "new-token".to_owned())]);
        assert_eq!(changed.secrets.remove, vec!["pw".to_owned()]);
        assert!(changed.values.is_empty());

        // Empty string removes an optional secret.
        let emptied = schema.validate_values(&map(json!({"pw": ""})), &Map::new(), &stored).unwrap();
        assert_eq!(emptied.secrets.remove, vec!["pw".to_owned()]);

        // Removing a required secret fails, even if stored.
        assert_eq!(
            error_codes(&schema.validate_values(&map(json!({"token": null})), &Map::new(), &stored).unwrap_err()),
            vec![("token", "required")]
        );
        assert_eq!(
            error_codes(&schema.validate_values(&map(json!({"token": ""})), &Map::new(), &stored).unwrap_err()),
            vec![("token", "required")]
        );

        // Constraints and types apply to secrets.
        assert_eq!(
            error_codes(
                &schema.validate_values(&map(json!({"pw": "abc", "token": 5})), &Map::new(), &stored).unwrap_err()
            ),
            vec![("token", "type"), ("pw", "min_length")]
        );
    }

    #[test]
    fn value_checks() {
        let integer: SchemaProperty = serde_json::from_value(json!({"type": "integer"})).unwrap();
        assert!(integer.check_value(&json!(3)).is_ok());
        assert!(integer.check_value(&json!(3.0)).is_ok());
        assert_eq!(integer.check_value(&json!(3.5)).unwrap_err().0, "type");
        assert_eq!(integer.check_value(&json!("3")).unwrap_err().0, "type");

        let number: SchemaProperty = serde_json::from_value(json!({"type": "number", "enum": [1, 2.5]})).unwrap();
        assert!(number.check_value(&json!(1.0)).is_ok());
        assert!(number.check_value(&json!(2.5)).is_ok());
        assert_eq!(number.check_value(&json!(2)).unwrap_err().0, "enum");

        let pattern: SchemaProperty = serde_json::from_value(json!({"type": "string", "pattern": "^[a-z]+$"})).unwrap();
        assert!(pattern.check_value(&json!("abc")).is_ok());
        assert_eq!(pattern.check_value(&json!("ab1")).unwrap_err().0, "pattern");
        // Unanchored pattern = search semantics.
        let search: SchemaProperty = serde_json::from_value(json!({"type": "string", "pattern": "b"})).unwrap();
        assert!(search.check_value(&json!("abc")).is_ok());

        let chars: SchemaProperty = serde_json::from_value(json!({"type": "string", "maxLength": 3})).unwrap();
        assert!(chars.check_value(&json!("ñáé")).is_ok());

        let email: SchemaProperty = serde_json::from_value(json!({"type": "string", "format": "email"})).unwrap();
        assert!(email.check_value(&json!("ops@example.com")).is_ok());
        for bad in ["ops", "ops@example", "@example.com", "a@b@c.com", "a b@c.com", "a@.com", "a@c.com."] {
            assert_eq!(email.check_value(&json!(bad)).unwrap_err().0, "format", "{bad}");
        }

        let url: SchemaProperty = serde_json::from_value(json!({"type": "string", "format": "url"})).unwrap();
        assert!(url.check_value(&json!("http://localhost:8069")).is_ok());
        for bad in ["example.com", "ftp://example.com", "https://", "mailto:a@b.c"] {
            assert_eq!(url.check_value(&json!(bad)).unwrap_err().0, "format", "{bad}");
        }
    }
}

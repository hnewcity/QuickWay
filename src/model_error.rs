//! Safe, protocol-neutral errors from Quick's model responses.
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct ModelError {
    pub kind: &'static str,
    pub code: Option<String>,
    pub message: String,
}

impl ModelError {
    pub fn from_upstream(fallback_code: &str, payload: &Value) -> Self {
        let code = [
            payload.get("errorType"),
            payload.get("code"),
            payload.get("__type"),
            payload.pointer("/error/code"),
            payload.pointer("/error/type"),
        ]
        .into_iter()
        .flatten()
        .find_map(Value::as_str)
        .unwrap_or(fallback_code)
        .rsplit('#')
        .next()
        .unwrap_or("UpstreamError");
        // Error codes are identifiers, never arbitrary response bodies.
        let code = if !code.is_empty()
            && code.len() <= 128
            && code
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
        {
            code
        } else {
            "UpstreamError"
        };
        let kind = match code.to_ascii_lowercase().as_str() {
            "accessdeniedexception" | "accessdenied" | "permission_error" => "permission_error",
            "unauthorizedexception"
            | "unrecognizedclientexception"
            | "expiredtokenexception"
            | "authentication_error" => "authentication_error",
            "throttlingexception"
            | "toomanyrequestsexception"
            | "usagelimitexceeded"
            | "servicequotaexceededexception"
            | "rate_limit_error" => "rate_limit_error",
            "validationexception" | "invalidrequestexception" | "invalid_request_error" => {
                "invalid_request_error"
            }
            "resourcenotfoundexception" | "not_found_error" => "not_found_error",
            "serviceunavailableexception" | "overloaded_error" => "overloaded_error",
            _ => "api_error",
        };
        let detail = [
            payload.get("message"),
            payload.get("errorMessage"),
            payload.get("error"),
            payload.pointer("/error/message"),
        ]
        .into_iter()
        .flatten()
        .find_map(Value::as_str)
        .map(scrub_message)
        .filter(|s| !s.is_empty());
        let prefix = format!("Quick {code}: ");
        let message = match detail {
            Some(detail) if detail.starts_with(&prefix) => detail,
            Some(detail) => format!("Quick {code}: {detail}"),
            None => format!("Quick {code}: the upstream model request failed."),
        };
        Self {
            kind,
            code: Some(code.into()),
            message,
        }
    }

    pub fn status(&self) -> u16 {
        match self.kind {
            "invalid_request_error" => 400,
            "authentication_error" => 401,
            "permission_error" => 403,
            "not_found_error" => 404,
            "rate_limit_error" => 429,
            "overloaded_error" => 503,
            _ => 502,
        }
    }

    pub fn event(&self) -> Value {
        let mut error = json!({"type":self.kind,"message":self.message});
        if let Some(code) = &self.code {
            error["code"] = code.clone().into();
        }
        json!({"type":"error","error":error})
    }
}

impl From<String> for ModelError {
    fn from(message: String) -> Self {
        Self {
            kind: "api_error",
            code: None,
            message,
        }
    }
}
impl From<&str> for ModelError {
    fn from(message: &str) -> Self {
        message.to_owned().into()
    }
}
impl std::fmt::Display for ModelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ModelError {}

/// Only diagnostic message fields are exposed. Remove credential-shaped
/// values and URLs before bounding the message; do not serialize the payload.
fn scrub_message(text: &str) -> String {
    let mut output = String::new();
    let delimiter = |c: char| c.is_whitespace() || "\"',;:=()[]{}<>".contains(c);
    for word in text.split_whitespace() {
        if output.chars().count() >= 800 {
            output.push_str("...");
            break;
        }
        if !output.is_empty() {
            output.push(' ');
        }
        if word.contains("://") {
            output.push_str("[redacted URL]");
            continue;
        }
        for piece in word.split_inclusive(delimiter) {
            if output.chars().count() >= 800 {
                output.push_str("...");
                break;
            }
            let value = piece.trim_end_matches(delimiter);
            let tail = &piece[value.len()..];
            if value.is_empty() {
                output.push_str(tail);
                continue;
            }
            let lower = value.to_ascii_lowercase();
            if matches!(
                lower.as_str(),
                "bearer"
                    | "basic"
                    | "authorization"
                    | "x-api-key"
                    | "x-amz-security-token"
                    | "api_key"
                    | "apikey"
                    | "api-key"
                    | "accesskeyid"
                    | "aws_access_key_id"
                    | "secretaccesskey"
                    | "aws_secret_access_key"
                    | "access_token"
                    | "id_token"
                    | "refresh_token"
                    | "client_secret"
                    | "token"
                    | "password"
                    | "secret"
                    | "cookie"
                    | "set-cookie"
            ) {
                output.push_str(value);
                // A credential may contain whitespace or punctuation (for
                // example a Cookie header). Discard the remainder rather
                // than guessing where the credential value ends.
                output.push_str(" [redacted]");
                return output;
            } else if value.contains('@')
                || (value.starts_with("eyJ") && value.len() > 30)
                || value.starts_with("AKIA")
                || value.starts_with("ASIA")
                || (value.len() >= 32
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-./+=".contains(&b)))
            {
                output.push_str("[redacted]");
            } else {
                // No terminal control sequences or multiline log injection.
                output.extend(value.chars().filter(|c| !c.is_control()).take(800));
            }
            output.push_str(tail);
        }
    }
    if let Some((cut, _)) = output.char_indices().nth(800) {
        output.truncate(cut);
        output.push_str("...");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_client_is_a_permission_error_without_login_advice() {
        let error = ModelError::from_upstream(
            "error",
            &json!({
                "errorType":"AccessDeniedException",
                "error":"This client is not supported for this request."
            }),
        );
        assert_eq!(error.status(), 403);
        assert_eq!(error.event()["error"]["type"], "permission_error");
        assert_eq!(error.event()["error"]["code"], "AccessDeniedException");
        assert_eq!(
            error.message,
            "Quick AccessDeniedException: This client is not supported for this request."
        );
        let roundtrip = ModelError::from_upstream("error", &error.event());
        assert_eq!(roundtrip.kind, "permission_error");
        assert_eq!(roundtrip.code.as_deref(), Some("AccessDeniedException"));
        assert_eq!(roundtrip.message, error.message);
    }

    #[test]
    fn qbs_validation_error_keeps_detail_across_normalization() {
        let error = ModelError::from_upstream(
            "invalid_request_error",
            &json!({
                "errorMessage":"Member must have length less than or equal to 8192",
                "request":{"content":"private prompt"}
            }),
        );
        assert_eq!(error.status(), 400);
        assert!(error.message.contains("8192"));
        assert!(!error.message.contains("private prompt"));
        let normalized = ModelError::from_upstream("UpstreamError", &error.event());
        assert_eq!(normalized.message, error.message);
    }

    #[test]
    fn errors_expose_only_bounded_scrubbed_diagnostic_fields() {
        let error = ModelError::from_upstream(
            "error",
            &json!({
                "errorType":"ValidationException", "request":{"content":"private prompt"},
                "message":"Denied alice@example.org Bearer fake-credential access_token=short-token https://example.org/?token=private eyJhbGciOiJub25lIn0.eyJzdWIiOiJzZWNyZXQifQ.sig \u{1b}"
            }),
        );
        assert_eq!(error.kind, "invalid_request_error");
        for secret in [
            "alice@example.org",
            "fake-credential",
            "short-token",
            "example.org",
            "eyJ",
            "private prompt",
            "\u{1b}",
        ] {
            assert!(!error.message.contains(secret), "leaked {secret}");
        }
        let long = ModelError::from_upstream("error", &json!({"message":"界 ".repeat(1000)}));
        assert!(long.message.chars().count() < 850);
        for message in [
            "Denied Cookie: session=short-secret; other=another-secret",
            "Denied password=two word secret",
            "Denied AWS_SECRET_ACCESS_KEY=short-secret",
        ] {
            let error = ModelError::from_upstream("error", &json!({"message":message}));
            assert!(!error.message.contains("short-secret"));
            assert!(!error.message.contains("another-secret"));
            assert!(!error.message.contains("two word"));
        }
        for secret in [
            "alice@example.org",
            "https://example.org/?token=private",
            "eyJhbGciOiJub25lIn0.eyJzdWIiOiJzZWNyZXQifQ.sig",
            "ASIAEXAMPLE0123456789",
        ] {
            let error =
                ModelError::from_upstream("error", &json!({"message":format!("Denied {secret}")}));
            assert!(!error.message.contains(secret));
        }
    }
}

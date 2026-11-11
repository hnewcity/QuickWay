//! User-selected thinking profiles shared by desktop chat and Claude Code.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn budget(self) -> u64 {
        match self {
            Self::Off => 0,
            Self::Low => 1024,
            Self::Medium => 4096,
            Self::High => 8192,
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "off" => Ok(Self::Off),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            _ => Err("Thinking effort must be off, low, medium or high".into()),
        }
    }

    /// Overrides only reasoning controls. Keep messages, tools and output format.
    pub fn apply(self, body: &mut Value, model: &str) -> Result<(), String> {
        let object = body.as_object_mut().ok_or("Request must be an object")?;
        if let Some(config) = object.get_mut("output_config") {
            let config = config
                .as_object_mut()
                .ok_or("output_config must be an object")?;
            config.remove("effort");
        }
        let budget_thinking = model.contains("claude-haiku-4-5");
        if self == Self::Off && model.contains("claude-opus-5-5") {
            // Opus 5.5 rejects disabled/between_tools and effort=none (Quick,
            // 2026-11-11). Its closest mode is adaptive thinking at low effort.
            object.insert("thinking".into(), json!({"type":"adaptive"}));
            let config = object.entry("output_config").or_insert_with(|| json!({}));
            config["effort"] = json!("low");
        } else if self == Self::Off {
            let kind = if model.contains("claude-sonnet-5-5") {
                "between_tools"
            } else {
                "disabled"
            };
            object.insert("thinking".into(), json!({"type":kind}));
        } else if budget_thinking {
            // Haiku 4.5 uses token budgets; Haiku 5.5 requires adaptive effort.
            object.insert(
                "thinking".into(),
                json!({"type":"enabled","budget_tokens":self.budget()}),
            );
            let maximum = object
                .get("max_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(4096);
            object.insert(
                "max_tokens".into(),
                json!(maximum.max(self.budget() + 4096)),
            );
        } else {
            object.insert("thinking".into(), json!({"type":"adaptive"}));
            let config = object.entry("output_config").or_insert_with(|| json!({}));
            config["effort"] = json!(self.as_str());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_preserve_tools_and_format_for_supported_protocol_variants() {
        for (model, upstream) in [
            ("claude-opus-4-8", "global.anthropic.claude-opus-4-8"),
            ("claude-sonnet-5-5", "global.anthropic.claude-sonnet-5-5"),
            (
                "claude-haiku-4-5-20251001",
                "global.anthropic.claude-haiku-4-5-20251001-v1:0",
            ),
            ("claude-fable-5-1", "us.anthropic.claude-fable-5-1"),
            ("gpt-6-astra", "us.openai.gpt-6-astra"),
        ] {
            for effort in [Effort::Off, Effort::Low, Effort::Medium, Effort::High] {
                let mut body = json!({"model":model,"max_tokens":4096,
                    "messages":[{"role":"user","content":"Hello"}],
                    "tools":[{"name":"echo","input_schema":{"type":"object"}}],
                    "output_config":{"effort":"max","format":{"type":"json_schema","schema":{"type":"object"}}}});
                effort.apply(&mut body, model).unwrap();
                let converted = crate::converse::request(body, upstream).unwrap();
                assert!(converted.pointer("/toolConfig/tools/0/toolSpec").is_some());
                let format = if crate::models::is_gpt(model) {
                    "/additionalModelRequestFields/text/format"
                } else {
                    "/additionalModelRequestFields/output_config/format"
                };
                assert!(converted.pointer(format).is_some());
                if crate::models::is_gpt(model) {
                    assert!(
                        converted
                            .pointer("/additionalModelRequestFields/thinking")
                            .is_none()
                    );
                    assert_eq!(
                        converted["additionalModelRequestFields"]["reasoning"]["effort"],
                        if effort == Effort::Off {
                            "none"
                        } else {
                            effort.as_str()
                        }
                    );
                } else if model.contains("haiku") && effort != Effort::Off {
                    assert_eq!(
                        converted["additionalModelRequestFields"]["thinking"]["budget_tokens"],
                        effort.budget()
                    );
                    assert!(
                        converted["inferenceConfig"]["maxTokens"].as_u64().unwrap()
                            > effort.budget()
                    );
                    assert!(
                        converted
                            .pointer("/additionalModelRequestFields/output_config/effort")
                            .is_none()
                    );
                } else {
                    let kind = if effort != Effort::Off {
                        "adaptive"
                    } else if model.contains("sonnet-5-5") {
                        "between_tools"
                    } else {
                        "disabled"
                    };
                    assert_eq!(
                        converted["additionalModelRequestFields"]["thinking"]["type"],
                        kind
                    );
                }
            }
        }
    }

    #[test]
    fn newly_discovered_models_use_their_supported_reasoning_protocols() {
        for model in ["claude-opus-5-5", "claude-haiku-5-5", "claude-sonnet-5"] {
            for effort in [Effort::Off, Effort::Low, Effort::Medium, Effort::High] {
                let mut body =
                    json!({"max_tokens":4096,"messages":[{"role":"user","content":"Hello"}]});
                effort.apply(&mut body, model).unwrap();
                let converted =
                    crate::converse::request(body, &format!("us.anthropic.{model}")).unwrap();
                let kind = if effort == Effort::Off && model != "claude-opus-5-5" {
                    "disabled"
                } else {
                    "adaptive"
                };
                assert_eq!(
                    converted["additionalModelRequestFields"]["thinking"]["type"],
                    kind
                );
                assert!(
                    converted
                        .pointer("/additionalModelRequestFields/thinking/budget_tokens")
                        .is_none()
                );
                if kind == "adaptive" {
                    // Opus 5.5 Off falls back to the lowest effort it accepts.
                    assert_eq!(
                        converted["additionalModelRequestFields"]["output_config"]["effort"],
                        if effort == Effort::Off {
                            "low"
                        } else {
                            effort.as_str()
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn malformed_effort_or_output_config_is_rejected() {
        assert!(Effort::parse("ultra").is_err());
        assert!(
            Effort::High
                .apply(&mut json!({"output_config":false}), "opus")
                .is_err()
        );
    }
}

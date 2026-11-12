//! Quick social/AIDP WebSocket framing, separate from Gateway's CBOR codec.
//!
//! Outbound messages are binary UTF-8 JSON. Inbound messages contain one or
//! more AWS EventStream frames whose JSON `payload` field wraps base64 JSON.
//! WebSocket fragmentation must be reassembled by the WebSocket library first.

use crate::model_error::ModelError;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::collections::HashMap;

const MIN_FRAME_LEN: usize = 16;
const PRELUDE_LEN: usize = 12;
/// Also configure the WebSocket transport to enforce this limit before allocation.
pub const MAX_SOCIAL_MESSAGE: usize = 16 * 1024 * 1024;
const MAX_HEADERS_LEN: usize = 128 * 1024;

/// Serialize the actual social transport envelope, not Gateway's CBOR frame.
pub fn encode_social(kind: &str, payload: Value) -> Vec<u8> {
    let inner = json!({"type":kind,"payload":payload}).to_string();
    json!({"eventType":"rawInputEvent","payload":{"payload":STANDARD.encode(inner)}})
        .to_string()
        .into_bytes()
}

/// Decode one complete binary WebSocket message atomically. No events are
/// returned if a later frame in this same message is invalid.
pub fn decode_social(mut bytes: &[u8]) -> Result<Vec<Value>, ModelError> {
    if bytes.is_empty() {
        return Err("Quick cloud received an empty WebSocket message".into());
    }
    if bytes.len() > MAX_SOCIAL_MESSAGE {
        return Err("Quick cloud WebSocket message exceeds 16 MiB".into());
    }
    let mut events = Vec::new();
    while !bytes.is_empty() {
        if bytes.len() < PRELUDE_LEN {
            return Err("Truncated Quick cloud EventStream prelude".into());
        }
        if crc32fast::hash(&bytes[..8]) != u32_at(bytes, 8) {
            return Err("Quick cloud EventStream prelude CRC32 mismatch".into());
        }
        let total = u32_at(bytes, 0) as usize;
        let header_len = u32_at(bytes, 4) as usize;
        if !(MIN_FRAME_LEN..=MAX_SOCIAL_MESSAGE).contains(&total) {
            return Err("Invalid Quick cloud EventStream frame length".into());
        }
        if header_len > total - MIN_FRAME_LEN || header_len > MAX_HEADERS_LEN {
            return Err("Invalid Quick cloud EventStream header length".into());
        }
        if bytes.len() < total {
            return Err("Truncated Quick cloud EventStream frame".into());
        }
        let (frame, remaining) = bytes.split_at(total);
        bytes = remaining;
        if crc32fast::hash(&frame[..total - 4]) != u32_at(frame, total - 4) {
            return Err("Quick cloud EventStream message CRC32 mismatch".into());
        }
        let headers_end = PRELUDE_LEN + header_len;
        let headers = parse_headers(&frame[PRELUDE_LEN..headers_end])?;
        let body = &frame[headers_end..total - 4];
        match header(&headers, ":message-type")? {
            Some("event") => {}
            Some("error" | "exception") => return Err(upstream_error(&headers, body)?),
            _ => return Err("Invalid Quick cloud EventStream message type".into()),
        }
        if header(&headers, ":event-type")? != Some("rawOutputEvent") {
            return Err("Unexpected Quick cloud EventStream event type".into());
        }
        if let Some(content_type) = header(&headers, ":content-type")?
            && content_type != "application/json"
        {
            return Err("Unexpected Quick cloud EventStream content type".into());
        }
        let outer: Value = serde_json::from_slice(body)
            .map_err(|_| ModelError::from("Invalid Quick cloud EventStream JSON"))?;
        let encoded = outer
            .get("payload")
            .and_then(Value::as_str)
            .ok_or("Quick cloud envelope has no string payload")?;
        let decoded = STANDARD
            .decode(encoded)
            .map_err(|_| ModelError::from("Invalid Quick cloud payload base64"))?;
        let event: Value = serde_json::from_slice(&decoded)
            .map_err(|_| ModelError::from("Invalid Quick cloud payload JSON"))?;
        if event
            .get("type")
            .and_then(Value::as_str)
            .is_none_or(|kind| {
                kind.is_empty() || kind.len() > 128 || kind.chars().any(char::is_control)
            })
        {
            return Err("Quick cloud payload has no valid event type".into());
        }
        // session_established and some control events may omit payload.
        if event
            .get("payload")
            .is_some_and(|p| !p.is_object() && !p.is_null())
        {
            return Err("Quick cloud event payload is not an object".into());
        }
        events.push(event);
    }
    Ok(events)
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

// Preserve non-string headers so a boolean/integer control field is rejected.
type Headers<'a> = HashMap<&'a str, Option<&'a str>>;

fn take<'a>(input: &mut &'a [u8], len: usize) -> Result<&'a [u8], ModelError> {
    if input.len() < len {
        return Err("Truncated Quick cloud EventStream header".into());
    }
    let (head, tail) = input.split_at(len);
    *input = tail;
    Ok(head)
}

fn parse_headers(mut input: &[u8]) -> Result<Headers<'_>, ModelError> {
    let mut headers = HashMap::new();
    while !input.is_empty() {
        let name_len = take(&mut input, 1)?[0] as usize;
        if name_len == 0 {
            return Err("Empty Quick cloud EventStream header name".into());
        }
        let name = std::str::from_utf8(take(&mut input, name_len)?)
            .map_err(|_| ModelError::from("Invalid Quick cloud EventStream header name"))?;
        let kind = take(&mut input, 1)?[0];
        let value = match kind {
            0 | 1 => None,
            2 | 3 | 4 | 5 | 8 | 9 => {
                let len = match kind {
                    2 => 1,
                    3 => 2,
                    4 => 4,
                    5 | 8 => 8,
                    _ => 16,
                };
                take(&mut input, len)?;
                None
            }
            6 | 7 => {
                let len = u16::from_be_bytes(take(&mut input, 2)?.try_into().unwrap()) as usize;
                let data = take(&mut input, len)?;
                if kind == 7 {
                    Some(std::str::from_utf8(data).map_err(|_| {
                        ModelError::from("Invalid Quick cloud EventStream string header")
                    })?)
                } else {
                    None
                }
            }
            _ => return Err("Unknown Quick cloud EventStream header type".into()),
        };
        if headers.insert(name, value).is_some() {
            return Err("Duplicate Quick cloud EventStream header".into());
        }
    }
    Ok(headers)
}

fn header<'a>(headers: &Headers<'a>, name: &str) -> Result<Option<&'a str>, ModelError> {
    match headers.get(name) {
        None => Ok(None),
        Some(Some(value)) => Ok(Some(value)),
        Some(None) => Err("Quick cloud EventStream control header is not a string".into()),
    }
}

fn upstream_error(headers: &Headers<'_>, body: &[u8]) -> Result<ModelError, ModelError> {
    let code = header(headers, ":exception-type")?
        .or(header(headers, ":error-code")?)
        .unwrap_or("UpstreamError");
    // Never reflect an arbitrary header value as an error code. These are
    // ordinary AWS transport identities; unrecognized identities stay generic.
    let code = match code {
        "AccessDeniedException"
        | "UnauthorizedException"
        | "NotAuthorizedException"
        | "ValidationException"
        | "ThrottlingException"
        | "ServiceQuotaExceededException"
        | "ServiceUnavailableException"
        | "InternalServerException"
        | "InternalFailure"
        | "ResourceNotFoundException"
        | "ExpiredTokenException"
        | "UnrecognizedClientException"
        | "ModelStreamErrorException"
        | "ModelTimeoutException" => code,
        _ => "UpstreamError",
    };
    let value = serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
    let message = value
        .get("message")
        .or_else(|| value.get("Message"))
        .and_then(Value::as_str)
        .or(header(headers, ":error-message")?);
    Ok(ModelError::from_upstream(code, &json!({"message":message})))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string_header(name: &str, value: &str) -> Vec<u8> {
        let mut out = vec![name.len() as u8];
        out.extend(name.as_bytes());
        out.push(7);
        out.extend((value.len() as u16).to_be_bytes());
        out.extend(value.as_bytes());
        out
    }

    fn frame(headers: &[u8], body: &[u8]) -> Vec<u8> {
        let mut out = ((MIN_FRAME_LEN + headers.len() + body.len()) as u32)
            .to_be_bytes()
            .to_vec();
        out.extend((headers.len() as u32).to_be_bytes());
        out.extend(crc32fast::hash(&out).to_be_bytes());
        out.extend(headers);
        out.extend(body);
        out.extend(crc32fast::hash(&out).to_be_bytes());
        out
    }

    fn headers() -> Vec<u8> {
        let mut out = string_header(":message-type", "event");
        out.extend(string_header(":event-type", "rawOutputEvent"));
        out.extend(string_header(":content-type", "application/json"));
        out
    }

    fn inner_frame(inner: &[u8]) -> Vec<u8> {
        frame(
            &headers(),
            json!({"payload":STANDARD.encode(inner)})
                .to_string()
                .as_bytes(),
        )
    }

    fn event_frame(event: &Value) -> Vec<u8> {
        inner_frame(event.to_string().as_bytes())
    }

    #[test]
    fn outbound_matches_social_json_base64_and_preserves_unicode_tool_input() {
        let payload = json!({"tool_call_id":"id-1","result":{"text":"中文 🦀"},"error":null});
        let wire: Value =
            serde_json::from_slice(&encode_social("tool_result", payload.clone())).unwrap();
        assert_eq!(wire["eventType"], "rawInputEvent");
        let inner = STANDARD
            .decode(wire["payload"]["payload"].as_str().unwrap())
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&inner).unwrap(),
            json!({"type":"tool_result","payload":payload})
        );
    }

    #[test]
    fn decodes_multiple_complete_frames_preserving_routing_and_text() {
        let events = vec![
            json!({"type":"session_established"}),
            json!({"type":"text_delta","session_id":"conversation-1","payload":{"delta":"你好","kernel_thread_id":"thread-1"}}),
            json!({"type":"message_complete","payload":{"text":"你好","tool_calls":[]}}),
        ];
        let bytes: Vec<_> = events.iter().flat_map(event_frame).collect();
        assert_eq!(decode_social(&bytes).unwrap(), events);
    }

    #[test]
    fn rejects_every_truncated_frame_prefix_and_damaged_crcs() {
        let original = event_frame(&json!({"type":"connected","payload":{}}));
        for end in 0..original.len() {
            assert!(decode_social(&original[..end]).is_err(), "prefix {end}");
        }
        for position in [8, original.len() - 1] {
            let mut bytes = original.clone();
            bytes[position] ^= 1;
            assert!(decode_social(&bytes).unwrap_err().message.contains("CRC32"));
        }
        let mut concatenated = original.clone();
        concatenated.extend(&original[..original.len() - 1]);
        assert!(decode_social(&concatenated).is_err());
    }

    #[test]
    fn rejects_oversize_and_impossible_lengths_before_reading_payload() {
        for (total, header_len) in [
            (15, 0),
            (MAX_SOCIAL_MESSAGE as u32 + 1, 0),
            (16, 1),
            (200_000, MAX_HEADERS_LEN as u32 + 1),
        ] {
            let mut prelude = total.to_be_bytes().to_vec();
            prelude.extend(header_len.to_be_bytes());
            prelude.extend(crc32fast::hash(&prelude).to_be_bytes());
            assert!(decode_social(&prelude).is_err());
        }
        assert!(
            decode_social(&vec![0; MAX_SOCIAL_MESSAGE + 1])
                .unwrap_err()
                .message
                .contains("exceeds")
        );
    }

    #[test]
    fn malformed_headers_are_rejected_without_echoing_their_contents() {
        let secret = "private-credential-in-header";
        let invalid = [
            vec![0],
            vec![1, 0xff, 0],
            vec![1, b'x', 10],
            vec![1, b'x', 7, 0, 2, b'a'],
            string_header(":message-type", secret),
        ];
        for h in invalid {
            let error = decode_social(&frame(&h, b"{}")).unwrap_err();
            assert!(!error.to_string().contains(secret));
        }
        let mut duplicate = headers();
        duplicate.extend(string_header(":event-type", "rawOutputEvent"));
        assert!(
            decode_social(&frame(&duplicate, b"{}"))
                .unwrap_err()
                .message
                .contains("Duplicate")
        );
        let mut not_string = vec![13];
        not_string.extend(b":message-type");
        not_string.push(0);
        assert!(
            decode_social(&frame(&not_string, b"{}"))
                .unwrap_err()
                .message
                .contains("not a string")
        );
    }

    #[test]
    fn refuses_gateway_and_invalid_json_base64_or_event_shapes() {
        let mut gateway = string_header(":message-type", "event");
        gateway.extend(string_header(":event-type", "rawOutputEvent"));
        gateway.extend(string_header(":content-type", "application/cbor"));
        assert!(decode_social(&frame(&gateway, b"anything")).is_err());
        for outer in [
            b"invalid secret JSON".as_slice(),
            br#"{}"#,
            br#"{"payload":42}"#,
            br#"{"payload":"***"}"#,
        ] {
            let error = decode_social(&frame(&headers(), outer)).unwrap_err();
            assert!(!error.to_string().contains("secret"));
        }
        for inner in [
            b"invalid JSON".as_slice(),
            b"null",
            br#"{"type":42}"#,
            br#"{"type":""}"#,
            br#"{"type":"text_delta","payload":[]}"#,
        ] {
            assert!(decode_social(&inner_frame(inner)).is_err());
        }
    }

    #[test]
    fn transport_exception_preserves_permission_identity_and_scrubs_secrets() {
        let mut h = string_header(":message-type", "exception");
        h.extend(string_header(":exception-type", "AccessDeniedException"));
        let error = decode_social(&frame(
            &h,
            br#"{"message":"Denied Bearer private-secret-value","raw":"private-request"}"#,
        ))
        .unwrap_err();
        assert_eq!(error.status(), 403);
        assert_eq!(error.code.as_deref(), Some("AccessDeniedException"));
        assert!(
            !error.message.contains("private-secret") && !error.message.contains("private-request")
        );
        let mut h = string_header(":message-type", "error");
        h.extend(string_header(
            ":error-code",
            "an-arbitrary-private-credential",
        ));
        let error = decode_social(&frame(&h, b"{}")).unwrap_err();
        assert_eq!(error.code.as_deref(), Some("UpstreamError"));
        assert!(!error.message.contains("private-credential"));
    }
}

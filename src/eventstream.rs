//! Incremental decoder for Bedrock InvokeModelWithResponseStream responses.
//!
//! AWS EventStream framing is separate from the model protocol: a `chunk`
//! event contains JSON whose `bytes` field is base64-encoded Anthropic event
//! JSON. This decoder validates both CRCs before exposing an event. It does
//! not establish that a particular application uses this Bedrock operation.

use std::collections::HashMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use bytes::BytesMut;
use serde_json::Value;

const PRELUDE_LEN: usize = 12;
const MIN_FRAME_LEN: usize = 16;
const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

/// A framing or payload error is fatal; create a new decoder for a new stream.
#[derive(Debug, Default)]
pub struct Decoder {
    buffer: BytesMut,
    failure: Option<String>,
    finished: bool,
    proxy: bool,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Quick wraps Converse JSON in bedrockStreamEvent, without base64.
    pub fn new_proxy() -> Self {
        Self {
            proxy: true,
            ..Self::default()
        }
    }

    /// Accept arbitrary HTTP chunk boundaries and return complete model events.
    ///
    /// Memory used for an incomplete frame is bounded by `MAX_FRAME_LEN`, even
    /// when the caller supplies many complete frames in one HTTP chunk.
    pub fn push(&mut self, data: &[u8]) -> Result<Vec<Value>, String> {
        if let Some(error) = &self.failure {
            return Err(format!("EventStream decoder previously failed: {error}"));
        }
        if self.finished {
            return Err("EventStream data received after finish".into());
        }
        let result = self.push_inner(data);
        if let Err(error) = &result {
            self.failure = Some(error.clone());
            self.buffer.clear();
        }
        result
    }

    /// Verify that EOF did not cut a frame short. This checks framing only;
    /// the caller remains responsible for requiring a model `message_stop`.
    pub fn finish(&mut self) -> Result<(), String> {
        if let Some(error) = &self.failure {
            return Err(format!("EventStream decoder previously failed: {error}"));
        }
        if !self.buffer.is_empty() {
            let error = format!(
                "Truncated EventStream at EOF ({} buffered bytes)",
                self.buffer.len()
            );
            self.failure = Some(error.clone());
            self.buffer.clear();
            return Err(error);
        }
        self.finished = true;
        Ok(())
    }

    fn push_inner(&mut self, mut data: &[u8]) -> Result<Vec<Value>, String> {
        let mut events = Vec::new();
        while !data.is_empty() || self.buffer.len() >= PRELUDE_LEN {
            if self.buffer.len() < PRELUDE_LEN {
                let take = (PRELUDE_LEN - self.buffer.len()).min(data.len());
                self.buffer.extend_from_slice(&data[..take]);
                data = &data[take..];
                if self.buffer.len() < PRELUDE_LEN {
                    break;
                }
            }

            let (total_len, headers_len) = validate_prelude(&self.buffer)?;
            let take = (total_len - self.buffer.len()).min(data.len());
            self.buffer.extend_from_slice(&data[..take]);
            data = &data[take..];
            if self.buffer.len() < total_len {
                break;
            }

            let expected_crc = read_u32(&self.buffer[total_len - 4..total_len]);
            let actual_crc = crc32fast::hash(&self.buffer[..total_len - 4]);
            if expected_crc != actual_crc {
                return Err("EventStream message CRC32 mismatch".into());
            }
            let headers_end = PRELUDE_LEN + headers_len;
            let headers = parse_headers(&self.buffer[PRELUDE_LEN..headers_end])?;
            let event = decode_payload(
                &headers,
                &self.buffer[headers_end..total_len - 4],
                self.proxy,
            )?;
            events.push(event);
            self.buffer.clear();
        }
        Ok(events)
    }
}

fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn validate_prelude(bytes: &[u8]) -> Result<(usize, usize), String> {
    let total_len = read_u32(&bytes[..4]) as usize;
    let headers_len = read_u32(&bytes[4..8]) as usize;
    let expected_crc = read_u32(&bytes[8..12]);
    if crc32fast::hash(&bytes[..8]) != expected_crc {
        return Err("EventStream prelude CRC32 mismatch".into());
    }
    if !(MIN_FRAME_LEN..=MAX_FRAME_LEN).contains(&total_len) {
        return Err(format!(
            "Invalid EventStream frame length {total_len}; expected {MIN_FRAME_LEN}..={MAX_FRAME_LEN}"
        ));
    }
    if headers_len > total_len - MIN_FRAME_LEN {
        return Err("EventStream headers exceed frame boundary".into());
    }
    Ok((total_len, headers_len))
}

// None represents a non-string header. Its bytes still must be well formed;
// only the control headers read below are required to have string values.
type Headers = HashMap<String, Option<String>>;

fn parse_headers(mut input: &[u8]) -> Result<Headers, String> {
    let mut headers = HashMap::new();
    while !input.is_empty() {
        let name_len = input[0] as usize;
        input = &input[1..];
        if name_len == 0 {
            return Err("EventStream header name is empty".into());
        }
        let name = std::str::from_utf8(take_bytes(&mut input, name_len)?)
            .map_err(|_| "EventStream header name is not UTF-8")?
            .to_owned();
        let kind = take_bytes(&mut input, 1)?[0];
        let value = match kind {
            0 | 1 => None, // boolean true / false (no value bytes)
            2 => {
                take_bytes(&mut input, 1)?;
                None
            }
            3 => {
                take_bytes(&mut input, 2)?;
                None
            }
            4 => {
                take_bytes(&mut input, 4)?;
                None
            }
            5 | 8 => {
                take_bytes(&mut input, 8)?; // long / timestamp
                None
            }
            6 | 7 => {
                let len_bytes = take_bytes(&mut input, 2)?;
                let len = u16::from_be_bytes([len_bytes[0], len_bytes[1]]) as usize;
                let bytes = take_bytes(&mut input, len)?;
                if kind == 7 {
                    Some(
                        std::str::from_utf8(bytes)
                            .map_err(|_| "EventStream string header is not UTF-8")?
                            .to_owned(),
                    )
                } else {
                    None
                }
            }
            9 => {
                take_bytes(&mut input, 16)?; // UUID
                None
            }
            _ => return Err(format!("Unknown EventStream header type {kind}")),
        };
        if headers.insert(name, value).is_some() {
            return Err("Duplicate EventStream header".into());
        }
    }
    Ok(headers)
}

fn take_bytes<'a>(input: &mut &'a [u8], len: usize) -> Result<&'a [u8], String> {
    if input.len() < len {
        return Err("Truncated EventStream header".into());
    }
    let (head, tail) = input.split_at(len);
    *input = tail;
    Ok(head)
}

fn string_header<'a>(headers: &'a Headers, name: &str) -> Result<Option<&'a str>, String> {
    match headers.get(name) {
        Some(Some(value)) => Ok(Some(value)),
        Some(None) => Err(format!("EventStream control header {name} is not a string")),
        None => Ok(None),
    }
}

fn decode_payload(headers: &Headers, payload: &[u8], proxy: bool) -> Result<Value, String> {
    match string_header(headers, ":message-type")? {
        Some("event") => {}
        Some("exception") => {
            let kind = string_header(headers, ":exception-type")?.unwrap_or("unknown");
            return Err(format!("Bedrock EventStream exception: {kind}"));
        }
        Some("error") => {
            let code = string_header(headers, ":error-code")?.unwrap_or("unknown");
            return Err(format!("Bedrock EventStream error: {code}"));
        }
        Some(other) => return Err(format!("Unexpected EventStream message type: {other}")),
        None => return Err("EventStream frame is missing :message-type".into()),
    }
    match string_header(headers, ":event-type")? {
        Some("bedrockStreamEvent") if proxy => {
            return serde_json::from_slice(payload).map_err(|_| "Invalid Quick stream JSON".into());
        }
        Some("chunk") => {}
        Some(other) => return Err(format!("Unexpected Bedrock event type: {other}")),
        None => return Err("EventStream frame is missing :event-type".into()),
    }

    let envelope: Value =
        serde_json::from_slice(payload).map_err(|_| "Bedrock chunk payload is not valid JSON")?;
    let encoded = envelope
        .get("bytes")
        .and_then(Value::as_str)
        .ok_or("Bedrock chunk payload has no string bytes field")?;
    let decoded = STANDARD
        .decode(encoded)
        .map_err(|_| "Bedrock chunk bytes contain invalid base64")?;
    let event: Value = serde_json::from_slice(&decoded)
        .map_err(|_| "Bedrock chunk bytes do not contain valid JSON")?;
    if event
        .get("type")
        .and_then(Value::as_str)
        .is_none_or(|kind| kind.is_empty())
    {
        return Err("Bedrock chunk does not contain a typed Anthropic event object".into());
    }
    Ok(event)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quick_proxy_uses_plain_json_and_checks_frame_integrity() {
        let mut headers = string_header_bytes(":message-type", "event");
        headers.extend(string_header_bytes(":event-type", "bedrockStreamEvent"));
        let event = json!({"eventType":"contentBlockDelta","payload":{"contentBlockIndex":0,"delta":{"text":"中文"}}});
        let encoded = frame(&headers, &serde_json::to_vec(&event).unwrap());
        for split in 0..=encoded.len() {
            let mut decoder = Decoder::new_proxy();
            let mut events = decoder.push(&encoded[..split]).unwrap();
            events.extend(decoder.push(&encoded[split..]).unwrap());
            decoder.finish().unwrap();
            assert_eq!(events, vec![event.clone()]);
        }
    }

    fn string_header_bytes(name: &str, value: &str) -> Vec<u8> {
        let mut bytes = vec![name.len() as u8];
        bytes.extend_from_slice(name.as_bytes());
        bytes.push(7);
        bytes.extend_from_slice(&(value.len() as u16).to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
        bytes
    }

    fn frame(headers: &[u8], payload: &[u8]) -> Vec<u8> {
        let total = (MIN_FRAME_LEN + headers.len() + payload.len()) as u32;
        let mut bytes = total.to_be_bytes().to_vec();
        bytes.extend_from_slice(&(headers.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&crc32fast::hash(&bytes).to_be_bytes());
        bytes.extend_from_slice(headers);
        bytes.extend_from_slice(payload);
        bytes.extend_from_slice(&crc32fast::hash(&bytes).to_be_bytes());
        bytes
    }

    fn chunk_headers() -> Vec<u8> {
        let mut headers = string_header_bytes(":message-type", "event");
        headers.extend(string_header_bytes(":event-type", "chunk"));
        headers
    }

    fn chunk_with_payload(payload: &[u8]) -> Vec<u8> {
        frame(&chunk_headers(), payload)
    }

    fn chunk_with_inner(inner: &[u8]) -> Vec<u8> {
        chunk_with_payload(
            &serde_json::to_vec(&json!({ "bytes": STANDARD.encode(inner) })).unwrap(),
        )
    }

    fn chunk(event: &Value) -> Vec<u8> {
        chunk_with_inner(&serde_json::to_vec(event).unwrap())
    }

    fn sample_event() -> Value {
        json!({
            "type": "content_block_delta", "index": 0,
            "delta": { "type": "text_delta", "text": "你好，Rust 🦀\nこんにちは" }
        })
    }

    #[test]
    fn decodes_unicode_at_every_possible_two_chunk_boundary() {
        let event = sample_event();
        let bytes = chunk(&event);
        for boundary in 0..=bytes.len() {
            let mut decoder = Decoder::new();
            let mut events = decoder.push(&bytes[..boundary]).unwrap();
            events.extend(decoder.push(&bytes[boundary..]).unwrap());
            decoder.finish().unwrap();
            assert_eq!(events, vec![event.clone()], "split at {boundary}");
        }
    }

    #[test]
    fn decodes_multiple_frames_one_byte_at_a_time_and_in_one_push() {
        let expected = vec![
            json!({"type":"message_start", "message":{"id":"msg_test"}}),
            sample_event(),
            json!({"type":"message_stop"}),
        ];
        let bytes: Vec<u8> = expected.iter().flat_map(chunk).collect();
        let mut decoder = Decoder::new();
        let mut actual = Vec::new();
        for byte in &bytes {
            actual.extend(decoder.push(std::slice::from_ref(byte)).unwrap());
        }
        decoder.finish().unwrap();
        assert_eq!(actual, expected);
        let mut decoder = Decoder::new();
        assert_eq!(decoder.push(&bytes).unwrap(), expected);
        decoder.finish().unwrap();
    }

    #[test]
    fn preserves_partial_tool_json_without_parsing_it_as_a_complete_object() {
        let fragments = ["{\"text\":\"", "你好", "🦀\"}"];
        let expected: Vec<_> = fragments
            .iter()
            .map(|part| {
                json!({
                    "type":"content_block_delta", "index":1,
                    "delta":{"type":"input_json_delta", "partial_json":part}
                })
            })
            .collect();
        let bytes: Vec<u8> = expected.iter().flat_map(chunk).collect();
        for boundary in 0..=bytes.len() {
            let mut decoder = Decoder::new();
            let mut events = decoder.push(&bytes[..boundary]).unwrap();
            events.extend(decoder.push(&bytes[boundary..]).unwrap());
            decoder.finish().unwrap();
            assert_eq!(events, expected, "split at {boundary}");
        }
    }

    #[test]
    fn rejects_bad_prelude_and_message_crc_and_remains_failed() {
        for position in [8, chunk(&sample_event()).len() - 1] {
            let mut bytes = chunk(&sample_event());
            bytes[position] ^= 1;
            let mut decoder = Decoder::new();
            assert!(decoder.push(&bytes).unwrap_err().contains("CRC32"));
            assert!(decoder.push(&chunk(&sample_event())).is_err());
            assert!(decoder.finish().is_err());
        }
    }

    #[test]
    fn rejects_exceptions_and_error_frames() {
        for (message_type, header, code) in [
            ("exception", ":exception-type", "modelStreamErrorException"),
            ("error", ":error-code", "InternalFailure"),
        ] {
            let mut headers = string_header_bytes(":message-type", message_type);
            headers.extend(string_header_bytes(header, code));
            let bytes = frame(&headers, br#"{"message":"failure"}"#);
            let error = Decoder::new().push(&bytes).unwrap_err();
            assert!(error.contains(message_type));
            assert!(error.contains(code));
        }
    }

    #[test]
    fn rejects_truncated_eof_at_every_nonempty_prefix() {
        let bytes = chunk(&sample_event());
        for end in 1..bytes.len() {
            let mut decoder = Decoder::new();
            assert!(decoder.push(&bytes[..end]).unwrap().is_empty());
            assert!(
                decoder.finish().unwrap_err().contains("Truncated"),
                "EOF at {end}"
            );
        }
    }

    fn prelude(total: u32, headers: u32) -> Vec<u8> {
        let mut bytes = total.to_be_bytes().to_vec();
        bytes.extend_from_slice(&headers.to_be_bytes());
        bytes.extend_from_slice(&crc32fast::hash(&bytes).to_be_bytes());
        bytes
    }

    #[test]
    fn rejects_invalid_lengths_without_waiting_for_payload() {
        for total in [0, 15, MAX_FRAME_LEN as u32 + 1, u32::MAX] {
            let error = Decoder::new().push(&prelude(total, 0)).unwrap_err();
            assert!(error.contains("frame length"));
        }
        let error = Decoder::new().push(&prelude(16, 1)).unwrap_err();
        assert!(error.contains("headers exceed"));
    }

    #[test]
    fn rejects_invalid_base64_envelope_and_inner_json() {
        let cases = [
            (chunk_with_payload(b"not JSON"), "payload is not valid JSON"),
            (chunk_with_payload(br#"{"bytes":"***"}"#), "invalid base64"),
            (chunk_with_payload(br#"{"bytes":123}"#), "no string bytes"),
            (chunk_with_payload(br#"{}"#), "no string bytes"),
            (chunk_with_inner(b"not JSON"), "do not contain valid JSON"),
            (chunk_with_inner(b"null"), "typed Anthropic event"),
            (chunk_with_inner(br#"{"type":42}"#), "typed Anthropic event"),
        ];
        for (bytes, expected_error) in cases {
            let error = Decoder::new().push(&bytes).unwrap_err();
            assert!(error.contains(expected_error), "{error}");
        }
    }

    #[test]
    fn accepts_all_aws_header_value_types() {
        let mut headers = chunk_headers();
        for kind in 0..=9u8 {
            headers.extend_from_slice(&[1, b'a' + kind, kind]);
            match kind {
                0 | 1 => {}
                2 => headers.extend_from_slice(&[1]),
                3 => headers.extend_from_slice(&[0; 2]),
                4 => headers.extend_from_slice(&[0; 4]),
                5 | 8 => headers.extend_from_slice(&[0; 8]),
                6 | 7 => headers.extend_from_slice(&[0, 1, b'x']),
                9 => headers.extend_from_slice(&[0; 16]),
                _ => unreachable!(),
            }
        }
        let event = sample_event();
        let payload = serde_json::to_vec(&json!({
            "bytes": STANDARD.encode(serde_json::to_vec(&event).unwrap())
        }))
        .unwrap();
        let mut decoder = Decoder::new();
        assert_eq!(
            decoder.push(&frame(&headers, &payload)).unwrap(),
            vec![event]
        );
        decoder.finish().unwrap();
    }

    #[test]
    fn rejects_invalid_headers_with_valid_frame_crcs() {
        let cases = [
            (vec![1, b'x', 7, 0, 2, b'a'], "Truncated"),
            (vec![1, b'x', 10], "Unknown"),
            (vec![0], "empty"),
            (vec![1, 0xff, 0], "UTF-8"),
        ];
        for (headers, expected) in cases {
            let error = Decoder::new().push(&frame(&headers, b"{}")).unwrap_err();
            assert!(error.contains(expected));
        }
        let mut headers = chunk_headers();
        headers.extend(string_header_bytes(":event-type", "chunk"));
        assert!(
            Decoder::new()
                .push(&frame(&headers, b"{}"))
                .unwrap_err()
                .contains("Duplicate")
        );

        let mut headers = vec![13];
        headers.extend_from_slice(b":message-type");
        headers.push(0);
        assert!(
            Decoder::new()
                .push(&frame(&headers, b"{}"))
                .unwrap_err()
                .contains("not a string")
        );
    }

    #[test]
    fn rejects_unknown_event_kinds_and_missing_control_headers() {
        let cases = [
            (Vec::new(), "missing :message-type"),
            (
                string_header_bytes(":message-type", "event"),
                "missing :event-type",
            ),
            (
                string_header_bytes(":message-type", "other"),
                "Unexpected EventStream message",
            ),
        ];
        for (headers, expected) in cases {
            assert!(
                Decoder::new()
                    .push(&frame(&headers, b"{}"))
                    .unwrap_err()
                    .contains(expected)
            );
        }
        let mut headers = string_header_bytes(":message-type", "event");
        headers.extend(string_header_bytes(":event-type", "unknown"));
        assert!(
            Decoder::new()
                .push(&frame(&headers, b"{}"))
                .unwrap_err()
                .contains("Unexpected Bedrock event")
        );
    }

    #[test]
    fn finish_is_idempotent_but_disallows_more_data() {
        let mut decoder = Decoder::new();
        decoder.push(&chunk(&sample_event())).unwrap();
        decoder.finish().unwrap();
        decoder.finish().unwrap();
        assert!(decoder.push(&[]).unwrap_err().contains("after finish"));
    }
}

//! Present Quick's decision widgets and assistant references to API clients.
//! Only native markup at the start of a prose line is recognized. Literal XML in
//! fenced code or inline prose is untouched. Raw snapshots stay in Translator.
#[derive(Default)]
pub(crate) struct ClientText {
    pending: String,
    line_start: bool,
    fence: Option<(char, usize)>,
    decision: bool,
}

impl ClientText {
    pub fn new() -> Self {
        Self {
            line_start: true,
            ..Self::default()
        }
    }

    pub fn push(&mut self, text: &str, finish: bool) -> String {
        self.pending.push_str(text);
        let mut out = String::new();
        while !self.pending.is_empty() {
            if self.decision {
                let end = self
                    .pending
                    .find("</decision>")
                    .map(|i| i + "</decision>".len());
                if let Some(end) = end.or_else(|| finish.then_some(self.pending.len())) {
                    out.push_str(&render_decision(&self.pending[..end]));
                    self.pending.drain(..end);
                    self.decision = false;
                    self.line_start = false;
                    continue;
                }
                break;
            }
            if self.line_start {
                let head = self.pending.trim_start_matches(' ');
                let indentation = self.pending.len() - head.len();
                let widget = "<decision question=";
                let reference = "⟨assistant msg-";
                if indentation <= 3 {
                    if self.fence.is_none() && head.starts_with(reference) {
                        if let Some(end) = head.find('⟩') {
                            let end = end + '⟩'.len_utf8();
                            if is_native_reference(&head[..end]) {
                                self.pending.drain(..indentation + end);
                                self.line_start = false;
                                continue;
                            }
                        } else if !finish && !head.contains('\n') {
                            break;
                        }
                    }
                    if !finish && self.fence.is_none() && reference.starts_with(head) {
                        break;
                    }
                    if self.fence.is_none() && head.starts_with(widget) {
                        self.decision = true;
                        continue;
                    }
                    if !finish && self.fence.is_none() && widget.starts_with(head) {
                        break;
                    }
                    if head.starts_with("```") || head.starts_with("~~~") {
                        if !finish && !head.contains('\n') {
                            break;
                        }
                        let ch = head.chars().next().unwrap();
                        let count = head.chars().take_while(|c| *c == ch).count();
                        match self.fence {
                            None => self.fence = Some((ch, count)),
                            Some((open, length))
                                if ch == open
                                    && count >= length
                                    && head[count..]
                                        .lines()
                                        .next()
                                        .unwrap_or("")
                                        .trim()
                                        .is_empty() =>
                            {
                                self.fence = None
                            }
                            _ => {}
                        }
                    } else if !finish && ("```".starts_with(head) || "~~~".starts_with(head)) {
                        break;
                    }
                }
            }
            let end = self
                .pending
                .find('\n')
                .map_or(self.pending.len(), |i| i + 1);
            self.line_start = self.pending[..end].ends_with('\n');
            out.push_str(&self.pending[..end]);
            self.pending.drain(..end);
        }
        out
    }
}

fn is_native_reference(text: &str) -> bool {
    let Some(inner) = text
        .strip_prefix("⟨assistant msg-")
        .and_then(|s| s.strip_suffix('⟩'))
    else {
        return false;
    };
    let Some((id, timestamp)) = inner.split_once(" | ") else {
        return false;
    };
    !id.is_empty()
        && id.len() <= 80
        && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
        && !timestamp.is_empty()
        && timestamp.len() <= 128
        && !timestamp.chars().any(char::is_control)
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let value = tag.split_once(&format!("{name}="))?.1;
    let quote = value.chars().next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    let value = &value[quote.len_utf8()..];
    // A truncated opening tag can still carry a useful plain-text question.
    let end = value.find([quote, '\n', '<']).unwrap_or(value.len());
    Some(unescape(value[..end].trim()))
}

fn unescape(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

fn render_decision(text: &str) -> String {
    let mut result = attribute(text, "question").unwrap_or_default();
    let mut rest = text;
    while let Some(start) = rest.find("<option") {
        rest = &rest[start + "<option".len()..];
        let end = rest.find("<option").unwrap_or(rest.len());
        let option = &rest[..end];
        let description = attribute(option, "description").unwrap_or_default();
        let label = option
            .split_once('>')
            .map(|(_, body)| unescape(body.split('<').next().unwrap_or("").trim()))
            .unwrap_or_default();
        if !label.is_empty() || !description.is_empty() {
            result.push_str(if result.is_empty() { "- " } else { "\n- " });
            result.push_str(if label.is_empty() {
                &description
            } else {
                &label
            });
            if !label.is_empty() && !description.is_empty() {
                result.push_str(": ");
                result.push_str(&description);
            }
        }
        rest = &rest[end..];
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_assistant_references_are_removed_but_literal_examples_survive() {
        let reference = "⟨assistant msg-451195ca-c1 | Thu. 12 Nov 2026, 3:11 PM CST⟩";
        let source = format!(
            "Implementing now.\n{reference}\nDone.\n```text\n{reference}\n```\nInline {reference}"
        );
        let expected = source.replacen(reference, "", 1);
        for split in (0..=source.len()).filter(|i| source.is_char_boundary(*i)) {
            let mut filter = ClientText::new();
            let mut actual = filter.push(&source[..split], false);
            actual.push_str(&filter.push(&source[split..], false));
            actual.push_str(&filter.push("", true));
            assert_eq!(actual, expected, "split={split}");
        }
        assert!(!is_native_reference("⟨assistant msg-not-an-id | example⟩"));
    }
    #[test]
    fn native_widgets_are_converted_at_every_utf8_chunk_boundary() {
        let source = "Hello\n<decision question=\"接下来？\">\n<option description=\"运行测试\">检查</option>\n</decision>\nDone";
        let expected = "Hello\n接下来？\n- 检查: 运行测试\nDone";
        for split in (0..=source.len()).filter(|i| source.is_char_boundary(*i)) {
            let mut filter = ClientText::new();
            let mut actual = filter.push(&source[..split], false);
            actual.push_str(&filter.push(&source[split..], false));
            actual.push_str(&filter.push("", true));
            assert_eq!(actual, expected, "split={split}");
        }
        let mut filter = ClientText::new();
        let mut actual = String::new();
        for ch in source.chars() {
            actual.push_str(&filter.push(&ch.to_string(), false));
        }
        actual.push_str(&filter.push("", true));
        assert_eq!(actual, expected);
    }
    #[test]
    fn literal_xml_and_fenced_examples_are_preserved() {
        let source = "Use `<decision question=\"x\">`\n```xml\n<decision question=\"x\">\n<option>yes</option>\n</decision>\n```\n<decision question=\"Actual\"></decision>";
        let mut filter = ClientText::new();
        let actual = filter.push(source, true);
        assert_eq!(
            actual,
            source.replace("<decision question=\"Actual\"></decision>", "Actual")
        );
    }
    #[test]
    fn malformed_and_truncated_widgets_keep_readable_content() {
        let mut filter = ClientText::new();
        assert_eq!(filter.push("<decision question=\"接下来\n<option description=\"检查\">继续</option>\n<option description=\"测试", true), "接下来\n- 继续: 检查\n- 测试");
    }
}

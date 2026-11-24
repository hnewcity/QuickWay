//! Present Quick's decision widgets and assistant references to API clients.
//! Only native markup at the start of a prose line is recognized. Literal XML in
//! fenced code or inline prose is untouched. Raw snapshots stay in Translator.
//!
//! Quick annotates every history message with a `⟨role id | time …⟩` marker,
//! and the model sometimes imitates it, often several in a row. Every marker
//! leading a line is removed, and a line left empty by that removal disappears.
#[derive(Default)]
pub(crate) struct ClientText {
    pending: String,
    line_start: bool,
    fence: Option<(char, usize)>,
    decision: bool,
    stripped: bool,
}

/// Clean complete text, such as history a client stored before this filter.
pub(crate) fn strip_references(text: &str) -> String {
    ClientText::new().push(text, true)
}

const REFERENCE_LEADS: [&str; 4] = [
    "⟨assistant ",
    "⟨user ",
    "⟨main | assistant ",
    "⟨main | user ",
];
const MAX_REFERENCE_BYTES: usize = 512;

enum Reference {
    Complete(usize),
    Partial,
    None,
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
            if self.line_start && self.fence.is_none() {
                let head = self.pending.trim_start_matches(' ');
                let indentation = self.pending.len() - head.len();
                if indentation <= 3 || self.stripped {
                    match reference_at(head) {
                        Reference::Complete(end) => {
                            self.pending.drain(..indentation + end);
                            self.stripped = true;
                            continue;
                        }
                        Reference::Partial if !finish => break,
                        _ => {}
                    }
                }
                if self.stripped {
                    // Spaces after the markers, and the line break of a line
                    // that held only markers, belong to the markers.
                    if head.is_empty() || (!finish && head == "\r") {
                        if finish {
                            self.pending.clear();
                        }
                        break;
                    }
                    let newline = if head.starts_with("\r\n") {
                        2
                    } else {
                        usize::from(head.starts_with('\n'))
                    };
                    self.pending.drain(..indentation + newline);
                    self.stripped = false;
                    continue;
                }
            }
            if self.line_start {
                let head = self.pending.trim_start_matches(' ');
                let indentation = self.pending.len() - head.len();
                let widget = "<decision question=";
                if indentation <= 3 {
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

fn reference_at(head: &str) -> Reference {
    if !REFERENCE_LEADS.iter().any(|lead| head.starts_with(lead)) {
        return if REFERENCE_LEADS.iter().any(|lead| lead.starts_with(head)) {
            Reference::Partial
        } else {
            Reference::None
        };
    }
    match head.find(['⟩', '\n']) {
        Some(end) if head[end..].starts_with('⟩') => {
            let end = end + '⟩'.len_utf8();
            if is_native_reference(&head[..end]) {
                Reference::Complete(end)
            } else {
                Reference::None
            }
        }
        Some(_) => Reference::None,
        None if head.len() > MAX_REFERENCE_BYTES => Reference::None,
        None => Reference::Partial,
    }
}

/// `⟨[main | ]assistant|user <id> | <detail>[ | <detail>…]⟩`, where the ID is
/// `msg-` plus hex digits and dashes, or a bare UUID.
fn is_native_reference(text: &str) -> bool {
    let Some(inner) = text
        .strip_prefix('⟨')
        .and_then(|s| s.strip_suffix('⟩'))
    else {
        return false;
    };
    let inner = inner.strip_prefix("main | ").unwrap_or(inner);
    let Some(rest) = inner
        .strip_prefix("assistant ")
        .or_else(|| inner.strip_prefix("user "))
    else {
        return false;
    };
    let mut fields = rest.split(" | ");
    let id = fields.next().unwrap_or("");
    let (id, minimum) = match id.strip_prefix("msg-") {
        Some(id) => (id, 1),
        None => (id, 32),
    };
    let mut details = 0;
    let details_valid = fields.all(|field| {
        details += 1;
        !field.is_empty() && !field.contains(['⟨', '⟩']) && !field.chars().any(char::is_control)
    });
    text.len() <= MAX_REFERENCE_BYTES
        && (minimum..=80).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
        && details_valid
        && details > 0
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

    fn assert_every_split(source: &str, expected: &str) {
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
        assert_eq!(actual, expected, "one character at a time");
    }

    #[test]
    fn native_assistant_references_are_removed_but_literal_examples_survive() {
        let reference = "⟨assistant msg-451195ca-c1 | Thu. 12 Nov 2026, 3:11 PM CST⟩";
        let source = format!(
            "Implementing now.\n{reference}\nDone.\n```text\n{reference}\n```\nInline {reference}"
        );
        let expected =
            format!("Implementing now.\nDone.\n```text\n{reference}\n```\nInline {reference}");
        assert_every_split(&source, &expected);
        assert!(!is_native_reference("⟨assistant msg-not-an-id | example⟩"));
        assert!(!is_native_reference("⟨assistant add | example⟩"));
        assert!(!is_native_reference("⟨assistant msg-abc⟩"));
    }

    #[test]
    fn repeated_and_variant_references_leading_a_line_are_removed() {
        let first = "⟨assistant msg-c64c6ead-c12 | Thu. 12 Nov 2026, 11:44 PM CST⟩";
        let second = "⟨assistant msg-c64c6ead-c13 | Thu. 12 Nov 2026, 11:44 PM CST⟩";
        let user = "⟨user 15b50d80-5cf3-4cf9-8c08-c7f4f5d11af4 | Fri. 13 Nov 2026, 12:01 AM CST | via desktop (windows) · QuickWay_API_session⟩";
        let main = "⟨main | assistant msg-abc123 | 2:15 PM⟩";
        let source = format!("{first}{second}同意。\n{main} {user}\r\nDone.\n{first}");
        assert_every_split(&source, "同意。\nDone.\n");
        assert_eq!(strip_references(&format!("{first}{second}\n")), "");
        assert_eq!(strip_references(&format!("  {first}\n\n{second}Hi")), "\nHi");
        assert_eq!(strip_references("⟨user thinks⟩ so"), "⟨user thinks⟩ so");
    }

    #[test]
    fn native_widgets_are_converted_at_every_utf8_chunk_boundary() {
        let source = "Hello\n<decision question=\"接下来？\">\n<option description=\"运行测试\">检查</option>\n</decision>\nDone";
        assert_every_split(source, "Hello\n接下来？\n- 检查: 运行测试\nDone");
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

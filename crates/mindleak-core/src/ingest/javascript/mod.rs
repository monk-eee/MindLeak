//! Minimal JavaScript/TypeScript lexer and lexical-scope model.

mod binding;
mod callable;
mod nav;
mod scope;
mod shadowing;

pub(crate) use binding::variable_declarator_at;
pub(crate) use callable::callable_for_definition;
pub(crate) use nav::{next_non_newline, previous_non_newline};
pub(crate) use shadowing::{is_identifier_shadowed, is_identifier_shadowed_except};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Identifier(String),
    StringLiteral(String),
    Punctuation(char),
    Newline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
}

impl Token {
    pub fn identifier(&self) -> Option<&str> {
        match &self.kind {
            TokenKind::Identifier(value) => Some(value),
            _ => None,
        }
    }

    pub fn string_literal(&self) -> Option<&str> {
        match &self.kind {
            TokenKind::StringLiteral(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_punctuation(&self, punctuation: char) -> bool {
        self.kind == TokenKind::Punctuation(punctuation)
    }

    pub fn is_newline(&self) -> bool {
        self.kind == TokenKind::Newline
    }
}

pub(crate) fn tokenize(content: &str) -> Vec<Token> {
    let bytes = content.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\t' | b'\r' => index += 1,
            b'\n' => {
                tokens.push(Token {
                    kind: TokenKind::Newline,
                    start: index,
                    end: index + 1,
                });
                index += 1;
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            quote @ (b'\'' | b'"') => {
                let start = index;
                index += 1;
                let mut value = String::new();
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        if let Some(next) = bytes.get(index + 1) {
                            value.push(*next as char);
                            index += 2;
                        } else {
                            index += 1;
                        }
                    } else if bytes[index] == quote {
                        index += 1;
                        break;
                    } else {
                        value.push(bytes[index] as char);
                        index += 1;
                    }
                }
                tokens.push(Token {
                    kind: TokenKind::StringLiteral(value),
                    start,
                    end: index,
                });
            }
            b'`' => {
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == b'`' {
                        index += 1;
                        break;
                    } else {
                        index += 1;
                    }
                }
            }
            byte if is_identifier_start(byte) => {
                let start = index;
                index += 1;
                while index < bytes.len() && is_identifier_continue(bytes[index]) {
                    index += 1;
                }
                tokens.push(Token {
                    kind: TokenKind::Identifier(content[start..index].to_string()),
                    start,
                    end: index,
                });
            }
            byte => {
                tokens.push(Token {
                    kind: TokenKind::Punctuation(byte as char),
                    start: index,
                    end: index + 1,
                });
                index += 1;
            }
        }
    }
    tokens
}

/// Preserve byte offsets and newlines while hiding comments and literal bodies
/// from regex-based definition extraction.
pub(crate) fn mask_non_code(content: &str) -> String {
    let bytes = content.as_bytes();
    let mut masked = bytes.to_vec();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        match bytes[index] {
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
                mask_range(&mut masked, start, index);
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
                mask_range(&mut masked, start, index);
            }
            quote @ (b'\'' | b'"' | b'`') => {
                index += 1;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        index = (index + 2).min(bytes.len());
                    } else if bytes[index] == quote {
                        index += 1;
                        break;
                    } else {
                        index += 1;
                    }
                }
                mask_range(&mut masked, start, index);
            }
            _ => index += 1,
        }
    }
    String::from_utf8(masked).expect("masking preserves UTF-8")
}

fn mask_range(bytes: &mut [u8], start: usize, end: usize) {
    for byte in &mut bytes[start..end] {
        if *byte != b'\n' {
            *byte = b' ';
        }
    }
}

fn is_identifier_start(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphabetic()
}

fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_comments_strings_and_templates() {
        let tokens = tokenize("// ghost()\n/* hidden('x') */\n`template()`; real(); import './x';");
        let identifiers: Vec<&str> = tokens.iter().filter_map(Token::identifier).collect();
        assert_eq!(identifiers, vec!["real", "import"]);
        assert!(tokens
            .iter()
            .any(|token| token.string_literal() == Some("./x")));
    }
}

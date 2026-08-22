//! Pure data types returned by extraction.

/// A source symbol definition discovered in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    /// Stable identity within the file, qualified by a lexical owner when one
    /// exists (for example `GraphStore::new`).
    pub qualified_name: String,
    pub kind: String,
    pub line: usize,
}

/// An in-file call between qualified symbol identities defined in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub caller: String,
    pub callee: String,
}

/// A call to a name that is not defined in the current file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallReference {
    pub caller: String,
    pub callee: String,
}

/// The result of analysing one file.
#[derive(Debug, Clone, Default)]
pub struct Extraction {
    pub symbols: Vec<Symbol>,
    pub calls: Vec<Call>,
    pub call_references: Vec<CallReference>,
}

/// Internal definition record with the byte offset of its name.
pub(super) struct Def {
    pub(super) name: String,
    pub(super) qualified_name: String,
    pub(super) owner: Option<String>,
    pub(super) kind: String,
    pub(super) line: usize,
    pub(super) name_offset: usize,
}

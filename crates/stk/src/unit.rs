//! A single execution unit in the stk virtual machine.
//!
//! A unit consists of an array of instructions, and lookaside tables for
//! metadata like function locations.

use crate::collections::HashMap;
use crate::context::Context;
use crate::context::Item;
use crate::hash::Hash;
use crate::vm::{Inst, VmError};
use std::fmt;
use thiserror::Error;

/// Errors raised when building a new unit.
#[derive(Debug, Error)]
pub enum CompilationUnitError {
    /// Trying to register a conflicting function.
    #[error("conflicting function signature already exists `{existing}`")]
    FunctionConflict {
        /// The signature of an already existing function.
        existing: UnitFnSignature,
    },
    /// Tried to add an use that conflicts with an existing one.
    #[error("conflicting use already exists `{existing}`")]
    ImportConflict {
        /// The signature of the old use.
        existing: Item,
    },
    /// A static string was missing for the given hash and slot.
    #[error("missing static string for hash `{hash}` and slot `{slot}`")]
    StaticStringMissing {
        /// The hash of the string.
        hash: Hash,
        /// The slot of the string.
        slot: usize,
    },
    /// A static string was missing for the given hash and slot.
    #[error(
        "conflicting static string for hash `{hash}` between `{existing:?}` and `{current:?}`"
    )]
    StaticStringHashConflict {
        /// The hash of the string.
        hash: Hash,
        /// The static string that was inserted.
        current: String,
        /// The existing static string that conflicted.
        existing: String,
    },
    /// A static object keys was missing for the given hash and slot.
    #[error("missing static object keys for hash `{hash}` and slot `{slot}`")]
    StaticObjectKeysMissing {
        /// The hash of the object keys.
        hash: Hash,
        /// The slot of the object keys.
        slot: usize,
    },
    /// A static object keys was missing for the given hash and slot.
    #[error(
        "conflicting static object keys for hash `{hash}` between `{existing:?}` and `{current:?}`"
    )]
    StaticObjectKeysHashConflict {
        /// The hash of the object keys.
        hash: Hash,
        /// The static object keys that was inserted.
        current: Box<[String]>,
        /// The existing static object keys that conflicted.
        existing: Box<[String]>,
    },
    /// Tried to add a duplicate label.
    #[error("duplicate label `{label}`")]
    DuplicateLabel {
        /// The duplicate label.
        label: Label,
    },
    /// The specified label is missing.
    #[error("missing label `{label}`")]
    MissingLabel {
        /// The missing label.
        label: Label,
    },
}

/// A span corresponding to a range in the source file being parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Span {
    /// The start of the span in bytes.
    pub start: usize,
    /// The end of the span in bytes.
    pub end: usize,
}

impl Span {
    /// Construct a new span.
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// Return a span with a modified start position.
    pub fn with_start(self, start: usize) -> Self {
        Self {
            start,
            end: self.end,
        }
    }

    /// Return a span with a modified end position.
    pub fn with_end(self, end: usize) -> Self {
        Self {
            start: self.start,
            end,
        }
    }

    /// Check if current span completely overlaps with another.
    pub fn overlaps(self, other: Span) -> bool {
        self.start <= other.start && self.end >= other.end
    }

    /// An empty span.
    pub const fn empty() -> Self {
        Self { start: 0, end: 0 }
    }

    /// Get the length of the span.
    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Join this span with another span.
    pub fn join(self, other: Self) -> Self {
        Self {
            start: usize::min(self.start, other.start),
            end: usize::max(self.end, other.end),
        }
    }

    /// Get the point span.
    pub fn point(pos: usize) -> Self {
        Self {
            start: pos,
            end: pos,
        }
    }

    /// Narrow the span with the given amount.
    pub fn narrow(self, amount: usize) -> Self {
        Self {
            start: self.start.saturating_add(amount),
            end: self.end.saturating_sub(amount),
        }
    }

    /// Trim the start of the label by the given amount.
    pub fn trim_start(self, amount: usize) -> Self {
        Self {
            start: usize::min(self.start.saturating_add(amount), self.end),
            end: self.end,
        }
    }
}

impl fmt::Display for Span {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{}:{}", self.start, self.end)
    }
}

/// Information about a registered function.
#[derive(Debug)]
pub struct UnitFnInfo {
    /// Offset into the instruction set.
    pub offset: usize,
    /// Signature of the function.
    pub signature: UnitFnSignature,
}

/// A description of a function signature.
#[derive(Debug)]
pub struct UnitFnSignature {
    /// The path of the function.
    pub path: Item,
    /// The number of arguments expected in the function.
    pub args: usize,
}

impl UnitFnSignature {
    /// Construct a new function signature.
    pub fn new(path: Item, args: usize) -> Self {
        Self {
            path: path.to_owned(),
            args,
        }
    }
}

impl fmt::Display for UnitFnSignature {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{}(", self.path)?;

        let mut it = 0..self.args;
        let last = it.next_back();

        for _ in it {
            write!(fmt, "arg, ")?;
        }

        if last.is_some() {
            write!(fmt, "arg")?;
        }

        write!(fmt, ")")?;
        Ok(())
    }
}

/// Debug information for every instruction.
#[derive(Debug)]
pub struct DebugInfo {
    /// The span of the instruction.
    pub span: Span,
    /// The comment for the line.
    pub comment: Option<Box<str>>,
    /// Label associated with the location.
    pub label: Option<Label>,
}

/// Instructions from a single source file.
#[derive(Debug)]
pub struct CompilationUnit {
    /// The instructions contained in the source file.
    instructions: Vec<Inst>,
    /// All imports in the current unit.
    ///
    /// Only used to link against the current environment to make sure all
    /// required units are present.
    imports: HashMap<String, Item>,
    /// Where functions are located in the collection of instructions.
    functions: HashMap<Hash, UnitFnInfo>,
    /// Function by address.
    functions_rev: HashMap<usize, Hash>,
    /// A static string.
    static_strings: Vec<String>,
    /// Reverse lookup for static strings.
    static_string_rev: HashMap<Hash, usize>,
    /// Slots used for object keys.
    ///
    /// This is used when an object is used in a pattern match, to avoid having
    /// to send the collection of keys to the virtual machine.
    ///
    /// All keys are sorted with the default string sort.
    static_object_keys: Vec<Box<[String]>>,
    /// Used to detect duplicates in the collection of static object keys.
    static_object_keys_rev: HashMap<Hash, usize>,
    /// Debug info for each line.
    debug: Vec<DebugInfo>,
    /// The current label count.
    label_count: usize,
    /// A collection of required function hashes.
    required_functions: HashMap<Hash, Vec<Span>>,
}

impl CompilationUnit {
    /// Construct a new unit.
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            imports: HashMap::new(),
            functions: HashMap::new(),
            functions_rev: HashMap::new(),
            static_strings: Vec::new(),
            static_string_rev: HashMap::new(),
            static_object_keys: Vec::new(),
            static_object_keys_rev: HashMap::new(),
            debug: Vec::new(),
            label_count: 0,
            required_functions: HashMap::new(),
        }
    }

    /// Construct a new unit with the default prelude.
    pub fn with_default_prelude() -> Self {
        let mut this = Self::new();
        this.imports
            .insert(String::from("dbg"), Item::of(&["std", "dbg"]));
        this.imports
            .insert(String::from("unit"), Item::of(&["std", "unit"]));
        this.imports
            .insert(String::from("bool"), Item::of(&["std", "bool"]));
        this.imports
            .insert(String::from("char"), Item::of(&["std", "char"]));
        this.imports
            .insert(String::from("int"), Item::of(&["std", "int"]));
        this.imports
            .insert(String::from("float"), Item::of(&["std", "float"]));
        this.imports.insert(
            String::from("Object"),
            Item::of(&["std", "object", "Object"]),
        );
        this.imports
            .insert(String::from("Array"), Item::of(&["std", "array", "Array"]));
        this.imports.insert(
            String::from("String"),
            Item::of(&["std", "string", "String"]),
        );
        this
    }

    /// Access the function at the given instruction location.
    pub fn function_at(&self, n: usize) -> Option<(Hash, &UnitFnInfo)> {
        let hash = self.functions_rev.get(&n).copied()?;
        Some((hash, self.functions.get(&hash)?))
    }

    /// Access debug information for the given location if it is available.
    pub fn debug_info_at(&self, n: usize) -> Option<&DebugInfo> {
        self.debug.get(n)
    }

    /// Get the instruction at the given instruction pointer.
    pub fn instruction_at(&self, ip: usize) -> Option<&Inst> {
        self.instructions.get(ip)
    }

    /// Iterate over all static strings in the unit.
    pub fn iter_static_strings(&self) -> impl Iterator<Item = (Hash, &str)> + '_ {
        let mut it = self.static_strings.iter();

        std::iter::from_fn(move || {
            let s = it.next()?;
            Some((Hash::of(s), s.as_str()))
        })
    }

    /// Iterate over all static object keys in the unit.
    pub fn iter_static_object_keys(&self) -> impl Iterator<Item = (Hash, &[String])> + '_ {
        let mut it = self.static_object_keys.iter();

        std::iter::from_fn(move || {
            let s = it.next()?;
            Some((Hash::object_keys(&s[..]), &s[..]))
        })
    }

    /// Iterate over all instructions in order.
    pub fn iter_instructions(&self) -> impl Iterator<Item = Inst> + '_ {
        self.instructions.iter().copied()
    }

    /// Iterate over known functions.
    pub fn iter_functions(&self) -> impl Iterator<Item = (Hash, &UnitFnInfo)> + '_ {
        let mut it = self.functions.iter();

        std::iter::from_fn(move || {
            let (k, v) = it.next()?;
            Some((*k, v))
        })
    }

    /// Iterate over known imports.
    pub fn iter_imports<'a>(&'a self) -> impl Iterator<Item = (&'a str, &'a Item)> + '_ {
        let mut it = self.imports.iter();

        std::iter::from_fn(move || {
            let (k, v) = it.next()?;
            Some((k.as_str(), v))
        })
    }

    /// Lookup the static string by slot, if it exists.
    pub fn lookup_string(&self, slot: usize) -> Result<&str, VmError> {
        Ok(self
            .static_strings
            .get(slot)
            .ok_or_else(|| VmError::MissingStaticString { slot })?
            .as_str())
    }

    /// Lookup the static object keys by slot, if it exists.
    pub fn lookup_object_keys(&self, slot: usize) -> Option<&[String]> {
        self.static_object_keys.get(slot).map(|keys| &keys[..])
    }

    /// Insert a static string and return its associated slot that can later be
    /// looked up through [lookup_string][Self::lookup_string].
    ///
    /// Only uses up space if the static string is unique.
    pub fn new_static_string(&mut self, current: &str) -> Result<usize, CompilationUnitError> {
        let hash = Hash::of(&current);

        if let Some(existing_slot) = self.static_string_rev.get(&hash).copied() {
            let existing = self.static_strings.get(existing_slot).ok_or_else(|| {
                CompilationUnitError::StaticStringMissing {
                    hash,
                    slot: existing_slot,
                }
            })?;

            if existing != current {
                return Err(CompilationUnitError::StaticStringHashConflict {
                    hash,
                    current: current.to_owned(),
                    existing: existing.clone(),
                });
            }

            return Ok(existing_slot);
        }

        let new_slot = self.static_strings.len();
        self.static_strings.push(current.to_owned());
        self.static_string_rev.insert(hash, new_slot);
        Ok(new_slot)
    }

    /// Insert a new collection of static object keys, or return one already
    /// existing.
    pub fn new_static_object_keys(
        &mut self,
        current: &[String],
    ) -> Result<usize, CompilationUnitError> {
        let current = current.to_vec().into_boxed_slice();
        let hash = Hash::object_keys(&current[..]);

        if let Some(existing_slot) = self.static_object_keys_rev.get(&hash).copied() {
            let existing = self.static_object_keys.get(existing_slot).ok_or_else(|| {
                CompilationUnitError::StaticObjectKeysMissing {
                    hash,
                    slot: existing_slot,
                }
            })?;

            if *existing != current {
                return Err(CompilationUnitError::StaticObjectKeysHashConflict {
                    hash,
                    current,
                    existing: existing.clone(),
                });
            }

            return Ok(existing_slot);
        }

        let new_slot = self.static_object_keys.len();
        self.static_object_keys.push(current);
        self.static_object_keys_rev.insert(hash, new_slot);
        Ok(new_slot)
    }

    /// Lookup information of a function.
    pub fn lookup(&self, hash: Hash) -> Option<&UnitFnInfo> {
        self.functions.get(&hash)
    }

    /// Lookup the location of a dynamic function.
    pub fn lookup_offset(&self, hash: Hash) -> Option<usize> {
        Some(self.functions.get(&hash)?.offset)
    }

    /// Look up an use by name.
    pub fn lookup_import_by_name(&self, name: &str) -> Option<&Item> {
        self.imports.get(name)
    }

    /// Declare a new use.
    pub fn new_import<I>(&mut self, path: I) -> Result<(), CompilationUnitError>
    where
        I: Copy + IntoIterator,
        I::Item: AsRef<str>,
    {
        let path = Item::of(path);

        if let Some(last) = path.last() {
            if let Some(old) = self.imports.insert(last.to_owned(), path) {
                return Err(CompilationUnitError::ImportConflict { existing: old });
            }
        }

        Ok(())
    }

    /// Construct a new empty assembly associated with the current unit.
    pub fn new_assembly(&mut self) -> Assembly {
        Assembly::new(self.label_count)
    }

    /// Declare a new function at the current instruction pointer.
    pub fn new_function<I>(
        &mut self,
        path: I,
        args: usize,
        assembly: Assembly,
    ) -> Result<(), CompilationUnitError>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let offset = self.instructions.len();
        let path = Item::of(path);
        let hash = Hash::function(&path);

        self.functions_rev.insert(offset, hash);

        let info = UnitFnInfo {
            offset,
            signature: UnitFnSignature::new(path, args),
        };

        if let Some(old) = self.functions.insert(hash, info) {
            return Err(CompilationUnitError::FunctionConflict {
                existing: old.signature,
            });
        }

        self.add_assembly(assembly)?;
        Ok(())
    }

    /// Translate the given assembly into instructions.
    fn add_assembly(&mut self, assembly: Assembly) -> Result<(), CompilationUnitError> {
        self.label_count = assembly.label_count;

        self.required_functions.extend(assembly.required_functions);

        for (pos, (inst, span)) in assembly.instructions.into_iter().enumerate() {
            let mut comment = None;
            let label = assembly.labels_rev.get(&pos).copied();

            match inst {
                AssemblyInst::Jump { label } => {
                    comment = Some(format!("label:{}", label).into_boxed_str());
                    let offset = translate_offset(pos, label, &assembly.labels)?;
                    self.instructions.push(Inst::Jump { offset });
                }
                AssemblyInst::JumpIf { label } => {
                    comment = Some(format!("label:{}", label).into_boxed_str());
                    let offset = translate_offset(pos, label, &assembly.labels)?;
                    self.instructions.push(Inst::JumpIf { offset });
                }
                AssemblyInst::JumpIfNot { label } => {
                    comment = Some(format!("label:{}", label).into_boxed_str());
                    let offset = translate_offset(pos, label, &assembly.labels)?;
                    self.instructions.push(Inst::JumpIfNot { offset });
                }
                AssemblyInst::Raw { raw } => {
                    self.instructions.push(raw);
                }
            }

            self.debug.push(DebugInfo {
                span,
                comment,
                label,
            });
        }

        return Ok(());

        fn translate_offset(
            base: usize,
            label: Label,
            labels: &HashMap<Label, usize>,
        ) -> Result<isize, CompilationUnitError> {
            let base = base as isize;

            let offset =
                labels
                    .get(&label)
                    .copied()
                    .ok_or_else(|| CompilationUnitError::MissingLabel {
                        label: label.to_owned(),
                    })?;

            Ok((offset as isize) - base)
        }
    }

    /// Try to link the unit with the context, checking that all necessary
    /// functions are provided.
    ///
    /// This can prevent a number of runtime errors, like missing functions.
    pub fn link(&self, context: &Context, errors: &mut LinkerErrors) -> bool {
        for (hash, spans) in &self.required_functions {
            if self.functions.get(hash).is_none() && context.lookup(*hash).is_none() {
                errors.errors.push(LinkerError::MissingFunction {
                    hash: *hash,
                    spans: spans.clone(),
                });
            }
        }

        errors.errors.is_empty()
    }
}

/// A label that can be jumped to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Label {
    name: &'static str,
    ident: usize,
}

impl fmt::Display for Label {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "{}_{}", self.name, self.ident)
    }
}

#[derive(Debug, Clone)]
enum AssemblyInst {
    Jump { label: Label },
    JumpIf { label: Label },
    JumpIfNot { label: Label },
    Raw { raw: Inst },
}

/// Helper structure to build instructions and maintain certain invariants.
#[derive(Debug, Clone, Default)]
pub struct Assembly {
    /// Label to offset.
    labels: HashMap<Label, usize>,
    /// Registered label by offset.
    labels_rev: HashMap<usize, Label>,
    /// Instructions with spans.
    instructions: Vec<(AssemblyInst, Span)>,
    /// The number of labels.
    label_count: usize,
    /// The collection of functions required by this assembly.
    required_functions: HashMap<Hash, Vec<Span>>,
}

impl Assembly {
    /// Construct a new assembly.
    fn new(label_count: usize) -> Self {
        Self {
            labels: Default::default(),
            labels_rev: Default::default(),
            instructions: Default::default(),
            label_count,
            required_functions: Default::default(),
        }
    }

    /// Construct and return a new label.
    pub fn new_label(&mut self, name: &'static str) -> Label {
        let label = Label {
            name,
            ident: self.label_count,
        };

        self.label_count += 1;
        label
    }

    /// Apply the label at the current instruction offset.
    pub fn label(&mut self, label: Label) -> Result<Label, CompilationUnitError> {
        let offset = self.instructions.len();

        if let Some(_) = self.labels.insert(label, offset) {
            return Err(CompilationUnitError::DuplicateLabel { label });
        }

        self.labels_rev.insert(offset, label);
        Ok(label)
    }

    /// Add a jump to the given label.
    pub fn jump(&mut self, label: Label, span: Span) {
        self.instructions.push((AssemblyInst::Jump { label }, span));
    }

    /// Add a conditional jump to the given label.
    pub fn jump_if(&mut self, label: Label, span: Span) {
        self.instructions
            .push((AssemblyInst::JumpIf { label }, span));
    }

    /// Add a conditional jump to the given label.
    pub fn jump_if_not(&mut self, label: Label, span: Span) {
        self.instructions
            .push((AssemblyInst::JumpIfNot { label }, span));
    }

    /// Push a raw instruction.
    pub fn push(&mut self, raw: Inst, span: Span) {
        match raw {
            Inst::Call { hash, .. } => {
                self.required_functions.entry(hash).or_default().push(span);
            }
            _ => (),
        }

        self.instructions.push((AssemblyInst::Raw { raw }, span));
    }
}

/// An error raised during linking.
#[derive(Debug)]
pub enum LinkerError {
    /// Missing a function with the given hash.
    MissingFunction {
        /// Hash of the function.
        hash: Hash,
        /// Spans where the function is used.
        spans: Vec<Span>,
    },
}

/// Linker errors.
#[derive(Debug)]
pub struct LinkerErrors {
    errors: Vec<LinkerError>,
}

impl LinkerErrors {
    /// Construct a new collection of linker errors.
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    /// Test if error collection is empty.
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Return an iterator over all linker errors.
    pub fn errors(self) -> impl Iterator<Item = LinkerError> {
        self.errors.into_iter()
    }
}

impl<'a> IntoIterator for &'a LinkerErrors {
    type IntoIter = std::slice::Iter<'a, LinkerError>;
    type Item = <Self::IntoIter as Iterator>::Item;

    fn into_iter(self) -> Self::IntoIter {
        self.errors.iter()
    }
}
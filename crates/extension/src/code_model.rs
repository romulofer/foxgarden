//! `PLAN.md` Track 24 Phase 6: the type model behind dot-completion and code
//! generation, as far as the core is concerned.
//!
//! The editor offers a type's members after a `.`, and generates accessors,
//! constructors and overrides into a type's body. Both used to be answered
//! by Java- and Kotlin-shaped tree walks inside the core. What a type is,
//! which of its members a caller can see, what the receiver of a `.`
//! resolves to and what a generated accessor looks like are all answers only
//! a language's own extension has. What stays generic is the shape of those
//! answers — a type's name, its members, where new code goes — which is what
//! this module describes.
//!
//! Every answer is syntactic, read off the tree the editor already parsed:
//! completion has to work the instant a file opens, before any language
//! server is up, and in a file that belongs to no project at all.

/// What kind of member a [`TypeMember`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    /// State: a field, a property.
    Field,
    /// Behavior: a method, a function.
    Method,
}

/// One member of a type, as completion and code generation see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeMember {
    pub name: String,
    pub kind: MemberKind,
    /// A field's declared type or a method's return type, in the language's
    /// own notation — shown to the user as written, never interpreted by
    /// the core.
    pub type_text: String,
    /// A method's parameters as `(type, name)`, in declaration order. Empty
    /// for a field.
    pub params: Vec<(String, String)>,
    /// A field that cannot be reassigned once set. Always `false` for a
    /// method.
    pub read_only: bool,
}

/// Which of a type's members a question is about. The extension decides
/// what each one means in its language's own visibility rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberView {
    /// Seen from inside the type itself or a subtype of it — every member,
    /// whatever its visibility.
    Inside,
    /// Seen from outside, through a value of the type.
    Outside,
    /// The methods a subtype may override — the candidates "Override
    /// Method" offers.
    Overridable,
}

/// A type declared in a file: its name, and where generated members go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDeclaration {
    pub name: String,
    /// Byte offset generated members are inserted at — just before the
    /// body's closing delimiter. `None` when the declaration has no body to
    /// insert into.
    pub insertion_byte: Option<usize>,
}

/// A type declaration together with the fields code generation can use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeFields {
    pub declaration: TypeDeclaration,
    /// Every field accessors or a constructor may be generated for, in
    /// source order.
    pub fields: Vec<TypeMember>,
}

/// What the receiver written before a `.` resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverType {
    /// The type's simple name — what its source file is named after.
    pub type_name: String,
    pub view: MemberView,
    /// Declared in the file being edited (the type the cursor sits in),
    /// rather than in a project file named after it.
    pub in_this_file: bool,
}

/// One piece of code the user asked the editor to generate.
///
/// Borrows rather than owns: a request lives exactly as long as the one
/// `Extension::generate_code` call that answers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeGeneration<'a> {
    /// A getter and/or a setter per field. A read-only field never gets a
    /// setter.
    Accessors {
        fields: &'a [TypeMember],
        getters: bool,
        setters: bool,
    },
    /// A constructor assigning every field from a parameter.
    Constructor { type_name: &'a str, fields: &'a [TypeMember] },
    /// A string rendering of every field.
    ToString { type_name: &'a str, fields: &'a [TypeMember] },
    /// Value equality and a matching hash over every field.
    EqualsAndHashCode { type_name: &'a str, fields: &'a [TypeMember] },
    /// A stub overriding each method.
    Overrides { methods: &'a [TypeMember] },
}

use crate::ast::{
    Arg, BinaryOp, Block, BlockElement, Condition, Expr, Func, IfForm, NumLiteral, Param,
    ParamMode, PlacePath, PlaceRootName, Program as AstProgram, Span, Spanned, TypeBody, TypeDecl,
    TypeName, TypeRef, TypeTest, UnaryOp, Value,
};
use crate::types::{
    self, BoxId, CollectionDef, CollectionId, FuncSig, MethodId, MethodSig, SumId, TypeDef, TypeId,
    Types,
};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

mod convert;
use convert::*;
mod places;
use places::*;
mod program;
use program::*;
mod stmt;
use stmt::*;
mod calls;
use calls::*;
mod expr;
use expr::*;

/// Every checked type. User-defined types have exactly one variant here --
/// their category (represented, struct, union, union member) lives in the type
/// table, never in this enum (Specification 010 section 19 phase 3).
/// `Sum` is an inline sum's normalized member set (Specification 018 section
/// 4); like `User`, its members live in the type table (`Types::sum_members`),
/// never here. `Ord` gives every sum's member set one canonical sorted order,
/// so `Byte | Nil` and `Nil | Byte` intern to the same id regardless of
/// source order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ty {
    Float64,
    Int64,
    Bool,
    Nil,
    String,
    Unicode,
    ViewByte,
    ViewUnicode,
    Array(CollectionId),
    List(CollectionId),
    View(CollectionId),
    Map(CollectionId),
    Set(CollectionId),
    Byte,
    UInt16,
    UInt32,
    UInt64,
    Float32,
    User(TypeId),
    Sum(SumId),
    /// `Box<T>` (Specification 016 section 4.1). The pointee lives in
    /// `Types`' box table, indexed by `BoxId`, mirroring how a `Sum`'s
    /// members live in the type table rather than here.
    Box(BoxId),
}

impl From<TypeName> for Ty {
    fn from(value: TypeName) -> Self {
        match value {
            TypeName::Float64 => Self::Float64,
            TypeName::Int64 => Self::Int64,
            TypeName::Bool => Self::Bool,
            TypeName::Nil => Self::Nil,
            TypeName::String => Self::String,
            TypeName::Unicode => Self::Unicode,
            TypeName::Byte => Self::Byte,
            TypeName::UInt16 => Self::UInt16,
            TypeName::UInt32 => Self::UInt32,
            TypeName::UInt64 => Self::UInt64,
            TypeName::Float32 => Self::Float32,
        }
    }
}

impl std::fmt::Display for Ty {
    /// A user-defined type has no name without its table, so every diagnostic
    /// renders types through [`Types::display`] instead. The placeholder below
    /// exists only so `Ty` stays printable for debugging.
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Float64 => write!(f, "Float64"),
            Self::Int64 => write!(f, "Int64"),
            Self::Bool => write!(f, "Bool"),
            Self::Nil => write!(f, "Nil"),
            Self::String => write!(f, "String"),
            Self::Unicode => write!(f, "Unicode"),
            Self::ViewByte => write!(f, "View<Byte>"),
            Self::ViewUnicode => write!(f, "View<Unicode>"),
            Self::Array(_) => write!(f, "Array<...>"),
            Self::List(_) => write!(f, "List<...>"),
            Self::View(_) => write!(f, "View<...>"),
            Self::Map(_) => write!(f, "Map<...>"),
            Self::Set(_) => write!(f, "Set<...>"),
            Self::Byte => write!(f, "Byte"),
            Self::UInt16 => write!(f, "UInt16"),
            Self::UInt32 => write!(f, "UInt32"),
            Self::UInt64 => write!(f, "UInt64"),
            Self::Float32 => write!(f, "Float32"),
            Self::User(id) => write!(f, "<user type #{}>", id.0),
            Self::Sum(id) => write!(f, "<sum #{}>", id.0),
            Self::Box(id) => write!(f, "<box #{}>", id.0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Error {
    pub span: Span,
    pub msg: String,
}

#[derive(Debug)]
pub enum Failure {
    Source(Vec<Error>),
    Unknown(&'static str),
}

#[derive(Clone, Copy)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Copy)]
pub enum CmpOp {
    Eq,
    NotEq,
    Less,
    LessEq,
    Greater,
    GreaterEq,
}

#[derive(Clone, Copy)]
pub enum LogicalOp {
    And,
    Or,
}

/// The root of a checked place. Local names are unique for a whole function or
/// method (Specification 012 section 5.2), so a name is the binding identity;
/// no separate ID table is needed to tell two roots apart.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PlaceRoot {
    Local(String),
    /// The implicit method receiver.
    SelfRef,
}

impl std::fmt::Display for PlaceRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Local(name) => f.write_str(name),
            Self::SelfRef => f.write_str("self"),
        }
    }
}

/// A resolved place: a root plus field selectors. Assignment, type tests, and
/// receiver places all share this shape, and Specification 011's reference
/// arguments reuse it unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub root: PlaceRoot,
    pub root_ty: Ty,
    /// Field indices in selection order; empty selects the root itself.
    pub path: Vec<usize>,
    /// The type reached after applying `path`.
    pub ty: Ty,
}

/// Specification 016 section 12 (phase 3 step 1): the explicit mode a checked
/// place use occupies. Borrowing and mutation already have their own distinct
/// checked shapes -- a reference argument is [`TArg::Reference`], an
/// assignment target is `TStmt::Assign`'s `place`, and a receiver place is
/// [`TReceiver::Place`] -- so only the copy/consume distinction is ambiguous
/// enough to need a tag on [`TExpr::Place`] itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UseMode {
    /// An ordinary read: an operand, a receiver, a print argument, or any
    /// position other than the five consuming contexts below.
    Copy,
    /// Specification 016 section 6.1: initialization, assignment's right
    /// operand, a by-value argument, a function/method result, or an
    /// aggregate constructor argument. A move-only place used this way
    /// transfers its complete value and cannot be used again (section 6.2);
    /// a copyable place used this way remains an ordinary copy.
    Consume,
}

/// One resolved parameter. The passing mode travels with the value type, so no
/// later phase re-derives it from source syntax (Specification 011 section 11).
/// A `Ref<T>` parameter's `ty` is the referent type `T`.
#[derive(Clone, Debug)]
pub struct TParam {
    pub name: String,
    pub ty: Ty,
    pub mode: ParamMode,
}

/// One checked call argument. Specification 011 section 19 phase 2 step 5 keeps
/// the two kinds apart so lowering can never copy a reference argument.
pub enum TArg {
    Value(TExpr),
    /// The referent place, resolved once. Lowering passes its address.
    Reference(Place),
}

/// How a method call reaches its receiver. A call that may write through `self`
/// requires the `Place` form; a read-only call may use a temporary, which the
/// backend gives compiler-owned storage (Specification 010 section 15.3).
pub enum TReceiver {
    Place(Place),
    /// A receiver with no addressable place of its own (a temporary, such as
    /// a fresh `box(...)` or a call result). `Ty` is the value's own checked
    /// static type, un-dereferenced -- exactly what `Place::ty` already
    /// carries for the `Place` variant, so lowering (Specification 016
    /// section 4.3) can peel the same number of `Box<T>` layers regardless
    /// of which variant a call reached, even though a box's own LLVM value
    /// never reveals its pointee type on its own.
    Value(TExpr, Ty),
}

pub struct TMethodCall {
    pub receiver: TReceiver,
    pub method: MethodId,
    pub args: Vec<TArg>,
}

pub struct TStringPart {
    pub value: TExpr,
    pub ty: Ty,
}

/// Value-producing checked nodes. Nothing here may stand for a construct
/// without a result: no sentinel type, dummy value, or fallback expression.
pub enum TExpr {
    Num(NumLiteral),
    Bool(bool),
    Nil,
    StringLiteral(String),
    Unicode(u32),
    StringClone(Box<TExpr>),
    /// One maximal concatenation/interpolation plan. Parts remain in source
    /// evaluation order and lower through one final allocation.
    StringConcat(Vec<TStringPart>),
    StringFromUnicode(Box<TExpr>),
    StringFromUtf8(Box<TExpr>, SumId),
    ViewFromString(Box<TExpr>, Ty),
    ViewLength(Box<TExpr>, Ty),
    /// A checked, bounds-safe element lookup. The runtime returns a signed
    /// sentinel so lowering can construct the specified `T | Nil` sum without
    /// exposing a private result ABI to the runtime.
    ViewAt(Box<TExpr>, Box<TExpr>, Ty, SumId),
    ViewSlice {
        value: Box<TExpr>,
        start: Box<TExpr>,
        end: Box<TExpr>,
        view_ty: Ty,
        sum: SumId,
    },
    CollectionLiteral {
        ty: Ty,
        items: Vec<TExpr>,
    },
    CollectionNew(Ty),
    CollectionLength(Box<TExpr>),
    CollectionIsEmpty(Box<TExpr>),
    CollectionCapacity(Box<TExpr>),
    CollectionView(Box<TExpr>, Ty),
    CollectionSlice {
        value: Box<TExpr>,
        start: Box<TExpr>,
        end: Box<TExpr>,
        view_ty: Ty,
        sum: SumId,
        elem: Ty,
    },
    CollectionIndex {
        collection: Box<TExpr>,
        index: Box<TExpr>,
        collection_ty: Ty,
        elem: Ty,
    },
    /// Removes and returns the last scalar element of a mutable list.
    ListPop {
        receiver: Place,
        elem: Ty,
    },
    /// Removes and returns one scalar element from a mutable list.
    ListRemove {
        receiver: Place,
        index: Box<TExpr>,
        elem: Ty,
    },
    MapContains {
        receiver: Box<TExpr>,
        key: Box<TExpr>,
        key_ty: Ty,
        value_ty: Ty,
    },
    MapInsert {
        receiver: Place,
        key: Box<TExpr>,
        value: Box<TExpr>,
        key_ty: Ty,
        value_ty: Ty,
        require_existing: bool,
    },
    MapDelete {
        receiver: Place,
        key: Box<TExpr>,
        key_ty: Ty,
        value_ty: Ty,
    },
    MapIndex {
        receiver: Box<TExpr>,
        key: Box<TExpr>,
        key_ty: Ty,
        value_ty: Ty,
    },
    MapTake {
        receiver: Place,
        key: Box<TExpr>,
        key_ty: Ty,
        value_ty: Ty,
    },
    SetContains {
        receiver: Box<TExpr>,
        value: Box<TExpr>,
        elem: Ty,
    },
    SetInsert {
        receiver: Place,
        value: Box<TExpr>,
        elem: Ty,
    },
    SetDelete {
        receiver: Place,
        value: Box<TExpr>,
        elem: Ty,
    },
    /// Reads a place. Specification 016 section 12 (phase 3 step 1): the
    /// [`UseMode`] records whether this occurrence sits in a consuming
    /// context, set by `mark_consumed` after `check_expr` produces this node
    /// and before any coercion wraps it.
    Place(Place, UseMode),
    /// Reads one field of a value that has no addressable storage. `base_ty`
    /// is `base`'s own checked type, not yet automatically dereferenced --
    /// unlike `Print`'s trailing `Ty`, `base_ty` here exists so lowering
    /// (Specification 016 section 4.3) knows how many `Box<T>` layers, if
    /// any, to peel before reading `base`'s field, since a fresh `box(...)`
    /// value (not a place) can be the base of a field chain and the backend
    /// cannot recover that from `base`'s own lowered LLVM value (every box,
    /// regardless of pointee, lowers to the same opaque pointer type).
    FieldRead {
        base: Box<TExpr>,
        base_ty: Ty,
        index: usize,
        ty: Ty,
    },
    /// Builds a struct or union-member value. Entries appear in written
    /// evaluation order; each `usize` is the destination field's declaration
    /// index, so lowering evaluates left to right and stores in field order.
    Construct {
        type_id: TypeId,
        fields: Vec<(usize, TExpr)>,
    },
    /// Adds or removes exactly one represented-type layer. Identity at runtime;
    /// the node exists so the nominal change is explicit before lowering.
    Represent {
        value: Box<TExpr>,
        ty: Ty,
    },
    /// Injects a direct member value into its containing union.
    Inject {
        member: TypeId,
        into_union: TypeId,
        value: Box<TExpr>,
    },
    /// Injects a direct member value into an inline sum (Specification 018
    /// section 5). Unlike [`Self::Inject`], `member` names the exact member
    /// type directly (a scalar, `Ty::User`, or nothing else, never another
    /// sum) rather than a `TypeId`, since a sum member need not be a
    /// user-defined type at all. Deterministic tag assignment is lowering's
    /// job, not the checker's, so no tag is recorded here.
    InjectSum {
        sum: SumId,
        member: Ty,
        value: Box<TExpr>,
    },
    /// Lifts a reduced inline sum into a larger inline sum when every source
    /// member is already a direct target member (used by successful
    /// `return_on_error` results).
    LiftSum {
        value: Box<TExpr>,
        from: SumId,
        to: SumId,
    },
    Arith(Box<TExpr>, ArithOp, Box<TExpr>, Ty),
    Cmp(Box<TExpr>, CmpOp, Box<TExpr>, Ty),
    Not(Box<TExpr>),
    Logical(Box<TExpr>, LogicalOp, Box<TExpr>),
    /// Converts a condition value to the control-flow predicate defined by
    /// Specification 021. The original type is retained because truthiness
    /// is not an ordinary Boolean conversion.
    Truthiness(Box<TExpr>, Ty),
    /// Fallible-call propagation with the operand's one non-Error success
    /// member. The enclosing result type is retained for lowering.
    ReturnOnError {
        value: Box<TExpr>,
        sum: SumId,
        success: Ty,
        result: Ty,
        cleanup: Vec<TCleanup>,
    },
    /// A call to a declaration that has a result.
    Call(String, Vec<TArg>),
    /// A method call that has a result.
    MethodCall(Box<TMethodCall>),
    /// An `if` classified as value-form: every path produces a value, either
    /// through an `else` or through a proven-exhaustive type-test chain.
    If(Box<TValueIf>),
    Print(Box<TExpr>, Ty),
    Cast(Box<TExpr>, Ty),
    /// `box(expression)` (Specification 016 section 4.2). `ty` is the whole
    /// allocation's result type `Box<T>`, not the operand's type `T` --
    /// unlike `Print`'s trailing `Ty`, which names the operand. RFC 016 Task
    /// B/C gives this a lowering strategy; the checker only produces it.
    Box(Box<TExpr>, Ty),
}

/// A proven type test against a named union. `tag` is the member's
/// deterministic union tag, so lowering compares stored tags without
/// consulting the type table.
pub struct TTypeTest {
    pub place: Place,
    pub member: TypeId,
    pub tag: u32,
    /// The name and exact member type bound on the successful edge.
    pub binding: Option<(String, Ty)>,
}

/// A proven type test against one direct member of an inline sum
/// (Specification 018 section 6). Kept separate from [`TTypeTest`] rather
/// than folded into it because a sum member need not be a `TypeId` at all
/// (it may be a bare scalar), and because deterministic tag assignment is
/// lowering's job (Task B), not the checker's, so no tag is recorded here.
pub struct TSumTypeTest {
    pub place: Place,
    pub sum: SumId,
    pub member: Ty,
    /// The name and exact member type bound on the successful edge. `Nil`
    /// carries no value and is never bound.
    pub binding: Option<(String, Ty)>,
}

/// An `if`/`elseif` condition: an ordinary `Bool` value or a type test
/// against a named union or an inline sum.
pub enum TCondition {
    Expr(TExpr),
    Test(TTypeTest),
    SumTest(TSumTypeTest),
}

pub struct TValueIf {
    /// First arm is the `if`; remaining arms are `elseif`s, in source order.
    pub arms: Vec<(TCondition, TBlock)>,
    /// `None` only when `exhaustive` proved every path is covered.
    pub else_branch: Option<TBlock>,
    /// Every arm tests the same place and every direct member of its union
    /// appears exactly once, so the fall-through edge is unreachable.
    pub exhaustive: bool,
    pub ty: Ty,
}

/// Checked statements. These perform control flow or effects and produce no
/// value, so none of them can satisfy a value-required block.
pub enum TStmt {
    Let {
        mutable: bool,
        name: String,
        ty: Ty,
        value: TExpr,
    },
    Assign {
        place: Place,
        value: TExpr,
        /// Specification 016 section 6.3: an assignment to a move-only place
        /// that currently holds a live value must destroy that old value
        /// before the new one is installed. False for a copyable
        /// destination (nothing to destroy) and for a whole-root
        /// destination that is currently moved (reinitializing a moved
        /// mutable local, section 6.3's closing sentence, installs a value
        /// where none was live).
        drop_before: bool,
    },
    /// Replaces one checked array/list element after validating the runtime
    /// index. The receiver remains an owning place, so a view can never reach
    /// this statement.
    SequenceIndexAssign {
        receiver: Place,
        index: TExpr,
        value: TExpr,
        elem: Ty,
    },
    While {
        condition: TExpr,
        body: TBlock,
    },
    For {
        value_name: String,
        value_ty: Ty,
        key_name: Option<String>,
        key_ty: Option<Ty>,
        iterable: TExpr,
        collection_ty: Ty,
        body: TBlock,
    },
    Break {
        cleanup: Vec<TCleanup>,
    },
    /// `return` (Specification 026 section 10). `value` is `Some` only for a
    /// value return from a result-declaring callable, already checked and
    /// assignable to its declared result -- never a sentinel for a bare
    /// return. `cleanup` is the ordered, exit-sensitive plan computed at this
    /// exact point and run after the result is materialized (section 8).
    Return {
        value: Option<TExpr>,
        result: Option<Ty>,
        cleanup: Vec<TCleanup>,
    },
    /// Statement-form `return_on_error` for an operand whose exact type is
    /// `Nil | Error`. Nil continues the current block; Error exits through
    /// the enclosing fallible callable.
    ReturnOnError {
        value: TExpr,
        sum: SumId,
        result: Ty,
        cleanup: Vec<TCleanup>,
    },
    /// An `if` classified as statement-form: `else` is optional and every
    /// branch is a no-result block.
    If(TStmtIf),
    /// A call to a declaration without a result.
    Call(String, Vec<TArg>),
    /// A call to a method without a result.
    MethodCall(TMethodCall),
    /// A built-in collection mutation lowered directly through the runtime.
    ListPush {
        receiver: Place,
        value: TExpr,
        elem: Ty,
    },
    ListClear {
        receiver: Place,
        elem: Ty,
    },
    ListInsert {
        receiver: Place,
        index: TExpr,
        value: TExpr,
        elem: Ty,
    },
    ListReserve {
        receiver: Place,
        minimum: TExpr,
        elem: Ty,
    },
    MapClear {
        receiver: Place,
        key_ty: Ty,
        value_ty: Ty,
    },
    MapReserve {
        receiver: Place,
        minimum: TExpr,
        key_ty: Ty,
        value_ty: Ty,
    },
    SetClear {
        receiver: Place,
        elem: Ty,
    },
    SetReserve {
        receiver: Place,
        minimum: TExpr,
        elem: Ty,
    },
    /// A value-producing expression whose result is discarded.
    Expr(TExpr),
}

pub struct TStmtIf {
    pub arms: Vec<(TCondition, TBlock)>,
    pub else_branch: Option<TBlock>,
    /// Same meaning as [`TValueIf::exhaustive`].
    pub exhaustive: bool,
}

/// A block's ordered checked elements plus its optional resulting value.
/// `result` is `Some` only for a value-required block whose final element
/// supplied a value.
pub struct TBlock {
    pub statements: Vec<TStmt>,
    pub result: Option<TExpr>,
    /// Expected type of a value-producing block. Lowering uses it to classify
    /// an implicit fallthrough result before error-sensitive cleanup runs.
    pub result_ty: Option<Ty>,
    /// A single reverse-registration cleanup plan for local destruction and
    /// deferred calls (Specification 025).
    pub cleanup: Vec<TCleanup>,
}

pub enum TCleanup {
    Drop(Place),
    Deferred(Rc<TDeferred>),
}

pub struct TDeferred {
    pub on_error: bool,
    pub call: TStmt,
    pub span: Span,
    /// Roots read, borrowed, or consumed when the call is evaluated at exit.
    pub dependencies: Vec<PlaceRoot>,
    /// Whole roots consumed by the deferred call at exit. The backend uses
    /// these facts to avoid dropping a value after the call transfers it.
    pub consumes: Vec<PlaceRoot>,
}

pub struct TFunc {
    pub params: Vec<TParam>,
    /// `None` is a function without a result; it lowers to LLVM `void`.
    pub result: Option<Ty>,
    pub body: TBlock,
}

pub struct TMethod {
    pub receiver: TypeId,
    pub name: String,
    pub params: Vec<TParam>,
    pub result: Option<Ty>,
    /// The least-fixed-point receiver-write effect. Internal only: it creates
    /// no source-level method category and is not part of the signature.
    pub writes_receiver: bool,
    pub body: TBlock,
}

pub struct TExtern {
    pub symbol: String,
    pub params: Vec<TParam>,
    /// `None` is a bridge without a result; its C ABI result is `void`.
    pub result: Option<Ty>,
    pub span: std::ops::Range<usize>,
}

pub struct Program {
    pub funcs: HashMap<String, TFunc>,
    pub externs: HashMap<String, TExtern>,
    /// Every resolved user-defined type, indexed by `TypeId`.
    pub types: Vec<TypeDef>,
    /// Every checked method, indexed by `MethodId`.
    pub methods: Vec<TMethod>,
    /// Every interned inline sum's normalized member list, indexed by
    /// `SumId` (Specification 018 section 4). Lowering assigns each member's
    /// deterministic tag from its position here, the same way a named
    /// union's tag is its member's declaration position.
    pub sums: Vec<Vec<Ty>>,
    /// Every interned `Box<T>`'s pointee type, indexed by `BoxId`
    /// (Specification 016 section 4.1). RFC 016 Task B/C's lowering strategy
    /// is the first consumer; Task A only records it here, mirroring `sums`.
    pub boxes: Vec<Ty>,
    pub collections: Vec<CollectionDef>,
    pub body: TBlock,
}

#[derive(Clone, Copy)]
struct Binding<'src> {
    name: &'src str,
    ty: Ty,
    mutable: bool,
    /// Lexical cleanup-scope identity used by borrow provenance when this
    /// binding is reassigned from a nested block.
    scope: usize,
    /// Specification 016 section 7.3: set only for a union- or sum-test
    /// binding. Such a binding is never an independent owning root -- it is
    /// always a branch-scoped alias to its tested place's active payload --
    /// so `mark_consumed` treats a whole-binding consuming use as a subplace
    /// move (Specification 016 section 6.4) instead of a legitimate whole-
    /// root move, even though its own path is empty.
    type_test_alias: bool,
}

/// One method call awaiting the receiver-write fixed point. Validation cannot
/// run while bodies are checked because a callee's effect may not be known yet.
#[derive(Clone)]
struct ReceiverCall {
    method: MethodId,
    mutable_root: bool,
    receiver: String,
    span: Span,
}

/// A local immutable view borrow tracked to its source root. The scope index
/// limits last-use pruning to borrows whose view binding is alive in the block
/// being analyzed.
#[derive(Clone)]
struct ViewBorrow {
    view_name: String,
    sources: Vec<PlaceRoot>,
    scope: usize,
    /// Iteration keeps its source structurally borrowed for the complete loop
    /// body even when the source-visible binding is unused.
    persistent: bool,
}

struct Ctx<'src> {
    sigs: HashMap<String, FuncSig>,
    externs: HashSet<String>,
    types: Types,
    method_sigs: Vec<MethodSig>,
    method_index: HashMap<(TypeId, String), MethodId>,
    /// Every name bound anywhere in the function or method being checked.
    /// Specification 012 section 5.2 makes this uniqueness rule function-wide,
    /// so nested blocks and sibling branches share one set.
    declared: Vec<&'src str>,
    /// One entry per enclosing `while` body. `break` needs a non-empty stack.
    loops: Vec<usize>,
    /// The receiver type while a method body is checked.
    self_ty: Option<Ty>,
    /// Specification 026 section 5: the enclosing callable's declared result
    /// while its body is checked, distinct from "no result" -- `None` means
    /// no callable currently encloses the point being checked (top level),
    /// `Some(None)` a no-result function or method, `Some(Some(ty))` one that
    /// declares `: ty`. `return`'s permitted form is checked against this
    /// fact alone, never the syntactic kind of the immediately enclosing
    /// block.
    callable_result: Option<Option<Ty>>,
    current_method: Option<MethodId>,
    direct_writes: Vec<bool>,
    effect_edges: Vec<(MethodId, MethodId)>,
    receiver_calls: Vec<ReceiverCall>,
    /// Specification 016 section 6.2: every whole move-only root that is
    /// currently moved, for the function or method body being checked, keyed
    /// by root and recording the consuming operation's span. A root absent
    /// here is available -- true for every non-move-only root, so this stays
    /// empty for a program that never uses `Box<T>` -- so no entry is made
    /// until something actually moves. Reset per function/method by
    /// `begin_region`, snapshotted and restored across sibling `if`/`elseif`/
    /// `else` arms, and merged back by unioning every reachable arm's exit
    /// (available only when available on every one, Specification 016
    /// section 6.2).
    move_state: HashMap<PlaceRoot, Span>,
    view_borrows: Vec<ViewBorrow>,
    /// Ordered cleanup entries for every currently open lexical block.
    cleanup_scopes: Vec<Vec<TCleanup>>,
    errors: Vec<Error>,
    unknown: Option<&'static str>,
    generic_funcs: HashMap<&'src str, &'src Func<'src>>,
    generic_types: HashMap<&'src str, &'src TypeDecl<'src>>,
    generic_type_finished: HashSet<String>,
    generic_type_in_progress: HashSet<String>,
    generic_type_stack: Vec<String>,
    generic_subst: HashMap<&'src str, Ty>,
    generic_queue: Vec<GenericRequest>,
    generic_seen: HashSet<String>,
    specialization_count: usize,
    generic_depth: usize,
    generic_chain: Vec<String>,
}

#[derive(Clone)]
struct GenericRequest {
    name: String,
    args: Vec<Ty>,
    depth: usize,
    use_span: Span,
    chain: Vec<String>,
}

const MAX_SPECIALIZATION_DEPTH: usize = 128;
const MAX_SPECIALIZATIONS: usize = 4096;

impl<'src> Ctx<'src> {
    /// Builds an isolated semantic context for checking one generic template.
    /// Checked trees and cleanup plans stay in the scratch context; only its
    /// diagnostics are copied back to the real compilation.
    fn generic_scratch(&self) -> Self {
        Self {
            sigs: self.sigs.clone(),
            externs: self.externs.clone(),
            types: self.types.clone(),
            method_sigs: self.method_sigs.clone(),
            method_index: self.method_index.clone(),
            declared: Vec::new(),
            loops: Vec::new(),
            self_ty: None,
            callable_result: None,
            current_method: None,
            direct_writes: self.direct_writes.clone(),
            effect_edges: self.effect_edges.clone(),
            receiver_calls: self.receiver_calls.clone(),
            move_state: HashMap::new(),
            view_borrows: Vec::new(),
            cleanup_scopes: Vec::new(),
            errors: Vec::new(),
            unknown: None,
            generic_funcs: self.generic_funcs.clone(),
            generic_types: self.generic_types.clone(),
            generic_type_finished: self.generic_type_finished.clone(),
            generic_type_in_progress: self.generic_type_in_progress.clone(),
            generic_type_stack: self.generic_type_stack.clone(),
            generic_subst: HashMap::new(),
            generic_queue: Vec::new(),
            generic_seen: HashSet::new(),
            specialization_count: self.specialization_count,
            generic_depth: 0,
            generic_chain: Vec::new(),
        }
    }

    fn error(&mut self, span: Span, msg: String) {
        self.errors.push(Error { span, msg });
    }

    /// The qualified name of a type, for diagnostics.
    fn name(&self, ty: Ty) -> String {
        self.types.display(ty)
    }

    fn mismatch(&mut self, span: Span, expected: Ty, found: Ty) {
        let msg = format!(
            "expected '{}', found '{}'",
            self.name(expected),
            self.name(found)
        );
        self.error(span, msg);
    }

    /// Specification 009 section 6: a rejected operand pair names both types
    /// and the exact-match requirement.
    fn operands(&mut self, span: Span, what: &str, left: Ty, right: Ty) {
        let msg = format!(
            "{what} operands must be two numbers of the same type, found '{}' and '{}'",
            self.name(left),
            self.name(right)
        );
        self.error(span, msg);
    }

    fn method_name(&self, method: MethodId) -> String {
        self.method_sigs[method.index()].qualified(&self.types)
    }

    /// Renders a resolved place the way it was written, so an overlap or
    /// mutability diagnostic can name both argument places (Specification 011
    /// section 13).
    fn place_name(&self, place: &Place) -> String {
        let mut text = place.root.to_string();
        let mut current = place.root_ty;
        for index in &place.path {
            // Specification 016 section 4.3: a path may cross a box exactly
            // where the place itself does, so rendering it back needs the
            // same automatic dereference `walk_fields` used to build it.
            current = deref_box(&self.types, current);
            let Ty::User(id) = current else { break };
            let Some(fields) = self.types.def(id).fields() else {
                break;
            };
            let Some((name, ty)) = fields.get(*index) else {
                break;
            };
            text.push('.');
            text.push_str(name);
            current = *ty;
        }
        text
    }
}

type Env<'src> = Vec<Binding<'src>>;

pub fn check<'src>(program: &'src AstProgram<'src>) -> Result<Program, Failure> {
    let mut errors = Vec::new();
    let collected = types::collect(program, &mut errors);
    let method_count = collected.methods.len();
    let static_names = collected.static_names;
    let specialization_count = collected.specialization_count;
    let mut ctx = Ctx {
        sigs: collected.sigs,
        externs: program
            .externs
            .keys()
            .map(|name| (*name).to_string())
            .collect(),
        types: collected.types,
        method_sigs: collected.methods,
        method_index: collected.method_index,
        declared: Vec::new(),
        loops: Vec::new(),
        self_ty: None,
        callable_result: None,
        current_method: None,
        direct_writes: vec![false; method_count],
        effect_edges: Vec::new(),
        receiver_calls: Vec::new(),
        move_state: HashMap::new(),
        view_borrows: Vec::new(),
        cleanup_scopes: Vec::new(),
        errors,
        unknown: None,
        generic_funcs: program
            .funcs
            .iter()
            .filter(|(_, function)| !function.generic_params.is_empty())
            .map(|(name, function)| (*name, function))
            .collect(),
        generic_types: program
            .types
            .iter()
            .filter(|declaration| !declaration.generic_params.is_empty())
            .map(|declaration| (declaration.name, declaration))
            .collect(),
        generic_type_finished: HashSet::new(),
        generic_type_in_progress: HashSet::new(),
        generic_type_stack: Vec::new(),
        generic_subst: HashMap::new(),
        generic_queue: Vec::new(),
        generic_seen: HashSet::new(),
        specialization_count,
        generic_depth: 0,
        generic_chain: Vec::new(),
    };

    let generic_functions: Vec<&Func<'src>> = ctx.generic_funcs.values().copied().collect();
    let mut generic_errors = Vec::new();
    for function in generic_functions {
        validate_generic_function(function, &ctx, &mut generic_errors);
        validate_generic_function_body(function, &ctx, &mut generic_errors);
    }
    ctx.errors.extend(generic_errors);

    for function in program.externs.values() {
        check_duplicate_params(&mut ctx, &function.args);
        if !function.symbol.starts_with("snacc_user_") {
            ctx.error(
                function.span,
                "Rust bridge symbols must start with 'snacc_user_'".into(),
            );
        } else if !is_rust_identifier(function.symbol) {
            ctx.error(
                function.span,
                "Rust bridge symbols must be valid Rust identifiers".into(),
            );
        }
    }

    let mut names: Vec<&str> = program.funcs.keys().copied().collect();
    names.sort_unstable();
    let mut typed_funcs = HashMap::new();
    for name in names {
        let function = &program.funcs[name];
        if !function.generic_params.is_empty() {
            continue;
        }
        let signature = ctx.sigs[name].clone();
        if let Some(result) = signature.result
            && is_borrowed_type(&ctx, result)
        {
            ctx.error(
                function
                    .ret
                    .as_ref()
                    .map_or(function.span, |(_, span)| *span),
                format!(
                    "'{}' is borrowed and cannot be returned from a function",
                    ctx.name(result)
                ),
            );
        }
        let mut env = Env::new();
        let params = begin_region(&mut ctx, &mut env, &function.args, &signature.params, None);
        let result = signature.result;
        ctx.callable_result = Some(result);
        let (mut body, _) = check_block(&mut ctx, &mut env, &function.body, result);
        ctx.callable_result = None;
        append_parameter_drops(&ctx, &env, &mut body);
        typed_funcs.insert(
            name.to_string(),
            TFunc {
                params,
                result,
                body,
            },
        );
    }

    for (declaration, qualified) in program.statics.iter().zip(static_names) {
        let Some(qualified) = qualified else { continue };
        check_duplicate_params(&mut ctx, &declaration.args);
        let signature = ctx.sigs[&qualified].clone();
        if let Some(result) = signature.result
            && is_borrowed_type(&ctx, result)
        {
            ctx.error(
                declaration
                    .ret
                    .as_ref()
                    .map_or(declaration.span, |(_, span)| *span),
                format!(
                    "'{}' is borrowed and cannot be returned from an associated function",
                    ctx.name(result)
                ),
            );
        }
        let mut env = Env::new();
        let params = begin_region(
            &mut ctx,
            &mut env,
            &declaration.args,
            &signature.params,
            None,
        );
        ctx.callable_result = Some(signature.result);
        let (mut body, _) = check_block(&mut ctx, &mut env, &declaration.body, signature.result);
        ctx.callable_result = None;
        append_parameter_drops(&ctx, &env, &mut body);
        typed_funcs.insert(
            qualified,
            TFunc {
                params,
                result: signature.result,
                body,
            },
        );
    }

    // Methods are checked in declaration order so `MethodId` and the checked
    // vector agree, and so the effect analysis input is deterministic.
    let mut typed_methods = Vec::with_capacity(method_count);
    for index in 0..method_count {
        let receiver = ctx.method_sigs[index].receiver;
        let name = ctx.method_sigs[index].name.clone();
        let result = ctx.method_sigs[index].result;
        let declared_params = ctx.method_sigs[index].params.clone();
        let declaration = &program.methods[ctx.method_sigs[index].decl];
        if let Some(result) = result
            && is_borrowed_type(&ctx, result)
        {
            ctx.error(
                declaration
                    .ret
                    .as_ref()
                    .map_or(declaration.span, |(_, span)| *span),
                format!(
                    "'{}' is borrowed and cannot be returned from a method",
                    ctx.name(result)
                ),
            );
        }
        let mut env = Env::new();
        let self_ty = Ty::User(receiver);
        let params = begin_region(
            &mut ctx,
            &mut env,
            &declaration.args,
            &declared_params,
            Some(self_ty),
        );
        ctx.current_method = Some(MethodId(index as u32));
        ctx.callable_result = Some(result);
        let (mut body, _) = check_block(&mut ctx, &mut env, &declaration.body, result);
        ctx.callable_result = None;
        append_parameter_drops(&ctx, &env, &mut body);
        ctx.current_method = None;
        ctx.self_ty = None;
        typed_methods.push(TMethod {
            receiver,
            name,
            params,
            result,
            writes_receiver: false,
            body,
        });
    }

    let mut typed_externs = HashMap::new();
    let mut extern_names: Vec<&str> = program.externs.keys().copied().collect();
    extern_names.sort_unstable();
    for name in extern_names {
        let function = &program.externs[name];
        let signature = &ctx.sigs[name];
        let params = signature.params.clone();
        typed_externs.insert(
            name.to_string(),
            TExtern {
                symbol: function.symbol.to_string(),
                params,
                result: signature.result,
                span: function.span.into_range(),
            },
        );
    }

    // The top-level executable body is a no-result block with its own binding
    // namespace; Snacc creates no implicit global state.
    ctx.declared.clear();
    ctx.loops.clear();
    ctx.move_state.clear();
    ctx.view_borrows.clear();
    ctx.callable_result = None;
    let mut env = Env::new();
    let (body, _) = check_block(&mut ctx, &mut env, &program.body, None);

    // Generic bodies are checked only for concrete applications. Each request
    // can enqueue more requests, so this is a deterministic work queue rather
    // than recursive lowering of the source AST.
    let mut request_index = 0;
    while request_index < ctx.generic_queue.len() {
        let (request_name, request_args, request_depth, request_use_span, request_chain) = {
            let request = &ctx.generic_queue[request_index];
            (
                request.name.clone(),
                request.args.clone(),
                request.depth,
                request.use_span,
                request.chain.clone(),
            )
        };
        request_index += 1;
        let function = {
            let generic_funcs = &ctx.generic_funcs;
            let Some(function) = generic_funcs.get(request_name.as_str()).copied() else {
                continue;
            };
            function
        };
        let mangled = generic_name(&ctx, &request_name, &request_args);
        if typed_funcs.contains_key(&mangled) {
            continue;
        }
        let substitutions: HashMap<&str, Ty> = function
            .generic_params
            .iter()
            .zip(&request_args)
            .map(|((name, _), ty)| (*name, *ty))
            .collect();
        let error_start = ctx.errors.len();
        let previous = std::mem::replace(&mut ctx.generic_subst, substitutions.clone());
        let params = resolve_generic_params(&mut ctx, &function.args);
        let result = function.ret.as_ref().map(|ty| resolve_type(&mut ctx, ty));
        let signature = FuncSig {
            params: params.clone(),
            result,
        };
        ctx.generic_subst = previous;
        ctx.sigs.insert(mangled.clone(), signature);
        let previous_subst = std::mem::replace(&mut ctx.generic_subst, substitutions);
        let mut env = Env::new();
        let params = begin_region(&mut ctx, &mut env, &function.args, &params, None);
        ctx.callable_result = Some(result);
        ctx.generic_depth = request_depth;
        let previous_chain = std::mem::replace(&mut ctx.generic_chain, request_chain.clone());
        let (mut body, _) = check_block(&mut ctx, &mut env, &function.body, result);
        ctx.callable_result = None;
        ctx.generic_depth = 0;
        ctx.generic_chain = previous_chain;
        ctx.generic_subst = previous_subst;
        let concrete = request_args
            .iter()
            .map(|ty| ctx.name(*ty))
            .collect::<Vec<_>>()
            .join(", ");
        let chain = request_chain.join(" -> ");
        for error in &mut ctx.errors[error_start..] {
            error.msg = format!(
                "{} (while specializing {request_name}<{concrete}> declared at {}..{}; requested at {}..{}; instantiation chain: {chain})",
                error.msg,
                function.span.start,
                function.span.end,
                request_use_span.start,
                request_use_span.end,
            );
        }
        append_parameter_drops(&ctx, &env, &mut body);
        typed_funcs.insert(
            mangled,
            TFunc {
                params,
                result,
                body,
            },
        );
    }

    // Specification 010 section 19 phase 4: solve the effect to its least fixed
    // point, then validate every deferred receiver-writing call.
    let writes = solve_receiver_writes(&ctx.direct_writes, &ctx.effect_edges);
    for (index, method) in typed_methods.iter_mut().enumerate() {
        method.writes_receiver = writes[index];
    }
    let pending = std::mem::take(&mut ctx.receiver_calls);
    for call in pending {
        if writes[call.method.index()] && !call.mutable_root {
            let method = ctx.method_name(call.method);
            ctx.error(
                call.span,
                format!(
                    "'{method}' may assign through 'self', so its receiver requires a \
                     mutable root, but '{}' is not mutable",
                    call.receiver
                ),
            );
        }
    }

    if let Some(detail) = ctx.unknown {
        return Err(Failure::Unknown(detail));
    }
    if ctx.errors.is_empty() {
        // Read before `ctx.types.defs` moves out below: `all_sums`/
        // `all_boxes` borrow the whole `Types` value, which a partial move
        // would break.
        let sums = ctx.types.all_sums().to_vec();
        let boxes = ctx.types.all_boxes().to_vec();
        let collections = ctx.types.all_collections().to_vec();
        Ok(Program {
            funcs: typed_funcs,
            externs: typed_externs,
            types: ctx.types.defs,
            methods: typed_methods,
            sums,
            boxes,
            collections,
            body,
        })
    } else {
        Err(Failure::Source(ctx.errors))
    }
}

/// A resolved place plus its root's mutability. Mutability is a property of the
/// root alone (Specification 012 section 7); the struct definition is never
/// consulted.
struct Resolved {
    place: Place,
    mutable: bool,
}

/// Whether an expression resolves to a place.
enum PlaceOutcome {
    Resolved(Resolved),
    /// The root was a binding or `self` but the field path failed; a diagnostic
    /// has already been recorded.
    Reported,
    /// The root is not a binding, so the caller may treat this as a value or a
    /// qualified type path.
    NotAPlace,
}

enum CheckedReturnOnError {
    Expr { value: TExpr, ty: Ty },
    Statement(TStmt),
}

/// What a checked call turned out to be.
enum CheckedCall {
    Function {
        name: String,
        args: Vec<TArg>,
        result: Option<Ty>,
    },
    Method {
        call: TMethodCall,
        result: Option<Ty>,
    },
    Statement(TStmt),
    /// A constructor, wrap, or unwrap. These always produce a value.
    Value(TExpr, Ty),
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

// ---------------------------------------------------------------------
// Specification 010: nominal types, structs, unions, and methods.
// ---------------------------------------------------------------------

# Specification 030: Affine Ownership and Explicit Borrow Scopes

Status: Proposed

Document kind: Language semantics (ISO/IEC-style specification)

## 1. Proposal state

This specification is implementation-ready. It formalizes Snacc's existing
affine ownership model and replaces inferred last-use lifetimes for stored
views with one explicit lexical borrow construct:

~~~snacc
with text.bytes() as bytes do
    print(bytes.length())
end
~~~

The design deliberately retains automatic deterministic destruction, moves,
`Box<T>`, call-scoped `Ref<T>`, and zero-copy immutable views. It does not add
general references, lifetime annotations, manual memory management, shared
ownership, raw pointers, or mutable views.

There are no open design questions. Section 15 is the detailed implementation
plan and fixes the required implementation order.

## 2. Goals

The memory model shall be explainable by four user-facing rules:

1. Copyable values copy.
2. Resource-owning values move.
3. `.clone()` creates an independent owner only where a type explicitly
   supports cloning.
4. A borrow lasts for one call or one written `with` block and cannot escape.

The compiler shall continue to guarantee that every initialized owned value is
destroyed exactly once on every supported normal exit. Source programs shall
not contain manual `free`, address-of, dereference, or lifetime syntax.

The design favors predictable lexical restrictions over accepting more
programs through non-lexical, last-use inference.

## 3. Non-goals

This specification does not add:

- garbage collection or tracing;
- reference counting, `Shared<T>`, or `Weak<T>`;
- user-defined destructors or externally owned resource types;
- raw pointers, pointer arithmetic, address-of, or explicit dereference;
- manual allocation or deallocation;
- mutable views or mutable slice bridge parameters;
- partial moves from fields or indexed elements;
- general reference value types or lifetime parameters;
- pinning, self-referential values, or escaping borrows; or
- changes to runtime-trap cleanup behavior.

Those capabilities require separate specifications. In particular, any future
mutable view shall use the explicit lexical borrow construct defined here; it
shall not reintroduce inferred borrow lifetimes.

## 4. Terms

An **owner** is a live storage location responsible for one value's eventual
destruction.

A **copyable type** permits contraction: one value may be duplicated into
multiple independent values with no resource ownership transfer.

An **affine type** forbids contraction: an owned value may be transferred at
most once. It may be used zero times because the compiler destroys an
untransferred owner automatically.

A **move** transfers a complete value and its destruction obligation from one
owner to another. The source becomes unavailable.

A **borrow** grants temporary access to storage without transferring its
ownership. A borrow is either call-scoped through `Ref<T>` or `View<T>`, or
lexically scoped through `with`.

A **normal exit** is fallthrough, `return`, `return_on_error`, or `break`.
Runtime traps and process termination are not normal exits.

## 5. Type ownership classes

The scalar types `Int64`, `Float64`, `Bool`, `Byte`, `UInt16`, `UInt32`,
`UInt64`, `Float32`, and `Unicode` are copyable.

`String`, `Box<T>`, `List<T>`, `Map<K, V>`, and `Set<T>` are affine regardless
of their type arguments.

`Array<T, N>` is affine exactly when `T` is affine. A represented type has the
ownership class of its representation. A struct, named union, union member, or
inline sum is affine when any value it can actively contain is affine.
Otherwise it is copyable.

Generic ownership is determined after concrete substitution. Each concrete
specialization applies the rules above; specialization never grants an
implicit copy of an affine argument.

`Ref<T>` is not a value type. `View<T>` is a borrowed descriptor, not an owner
of the viewed elements.

## 6. Moves and copies

The following contexts consume an affine value:

- a variable initializer;
- the right side of assignment;
- a by-value argument;
- a returned result;
- a struct, union-member, array, list, map, set, or box construction operand;
  and
- any other operation documented as taking ownership.

Using an affine owning place in one of these contexts moves its complete value
and makes that place unavailable:

~~~snacc
let first: String = "hello"
let second: String = first
print(first) // error: first was moved
~~~

The same source use of a copyable place copies:

~~~snacc
let first: Int64 = 10
let second: Int64 = first
print(first) // valid
~~~

A moved mutable root may be reinitialized by complete assignment. A moved
immutable root remains unavailable. Moving an affine field, indexed element,
automatic box dereference, type-test binding, reference referent, or view
element out as an independent subplace is rejected. A complete owning
container must be moved, or an operation such as `take` or `remove` must
explicitly transfer an element.

Assignment from an owner to an overlapping destination is rejected. Before a
live affine destination is replaced, its old value is destroyed exactly once.

## 7. Cloning

Affine ownership does not imply that every affine type is cloneable. A type
supports `.clone()` only when its contract explicitly defines an independent
copy and every contained value can be cloned.

`String.clone()` produces independently owned UTF-8 storage. This
specification does not add a universal clone capability or implicit cloning.
The compiler never inserts a clone to make an otherwise invalid move compile.

## 8. Deterministic destruction

Every initialized affine owner still available at a normal scope exit is
destroyed exactly once. Locals are destroyed in reverse successful
initialization order, from the innermost exited scope outward. Function
parameters whose ownership entered the function participate in the same
cleanup order.

Destruction is structural:

- a box destroys its pointee and releases its allocation;
- a string releases its owned UTF-8 allocation;
- a list, map, or set destroys its live owned entries and releases its
  allocation;
- an affine array or struct destroys its affine elements or fields;
- a named union or inline sum destroys only its active member; and
- a represented type destroys its representation.

An affine result is fully materialized and its ownership transferred to the
caller before the callee destroys its remaining locals. `defer` and
`defer_on_error` remain entries in the same reverse-registration cleanup plan.

Runtime traps execute neither deferred calls nor ordinary destruction. They
terminate through the runtime fatal path.

## 9. Call-scoped references

`Ref<T>` remains permitted only as the direct type of a function, method,
static associated-function, or supported Rust bridge parameter. It grants
exclusive mutable access to one initialized mutable place for the duration of
that call.

The referent is used with ordinary value syntax and dereferenced
automatically. A reference cannot be constructed, stored, returned, boxed,
compared, placed in an aggregate, or nested in another type. Overlapping
exclusive reference arguments are rejected.

Changing a parameter between `T` and `Ref<T>` remains a breaking signature
change.

## 10. Temporary immutable views

A view-producing expression may be used without a binding when its complete
lifetime is contained in the surrounding expression or call:

~~~snacc
print(text.bytes().length())
consume_bytes(text.bytes())
~~~

An owning `String`, `Array<T, N>`, or `List<T>` may lend the corresponding view
to a `View<T>` value parameter for one call. The conversion does not move the
owner. A same-call move or exclusive borrow overlapping that loan is rejected.

A temporary view cannot be returned, placed in an owning aggregate, or stored
by an ordinary variable declaration.

## 11. Explicit lexical view binding

### 11.1 Syntax

This specification adds the reserved keywords `with` and `as` and the grammar
production:

~~~ebnf
with-statement = "with", expression, "as", identifier, "do", block, "end" ;
~~~

`with-statement` is one alternative of `block-element`. It is a statement and
never produces a value.

The sole spelling is:

~~~snacc
with view_expression as name do
    block
end
~~~

The initializer shall have a concrete `View<T>` type and shall retain a
compiler-known owning source, or derive from an existing view parameter or
enclosing `with` binding whose source is known. The initializer evaluates
exactly once before the body starts.

The binding's type is the initializer's exact view type. It is immutable, has
the lexical scope of the body, and follows the existing function-wide
unique-name rule. It is not an implicitly typed ordinary variable declaration.

### 11.2 Lifetime

The borrow begins after the initializer has evaluated successfully and lasts
until control exits the written body. It does not end at the binding's last
use. Every normal exit from the body ends the borrow after applicable deferred
actions for that body have executed.

~~~snacc
let mut text: String = "hello"

with text.bytes() as bytes do
    print(bytes.length())
end

text = "goodbye" // valid
~~~

This remains invalid even when `bytes` is not used after the assignment:

~~~snacc
let mut text: String = "hello"

with text.bytes() as bytes do
    text = "goodbye" // error: text is borrowed for the complete block
end
~~~

Within the body, the owning source cannot be moved, replaced, destroyed,
boxed, passed by exclusive reference, or structurally mutated. Immutable
reads and additional immutable borrows are permitted.

### 11.3 Nested views

A slice or view derived from another view retains the original owning source.
It may be used temporarily or bound by a nested `with`:

~~~snacc
with text.bytes() as bytes do
    with bytes.slice(1, 4) as middle do
        print(middle.length())
    end
end
~~~

Multiple immutable `with` borrows of the same source may overlap. Every such
borrow must have ended before the owner can be consumed or mutated.

## 12. Stored borrowed values are removed

An ordinary `let` declaration or assignment may not have a borrowed type.
Struct fields, named-union members, inline-sum members, boxes, arrays, lists,
maps, and sets may not contain a view directly or transitively.

Consequently these formerly accepted forms become errors:

~~~snacc
let bytes: View<Byte> = text.bytes()

type Selection is struct
    bytes: View<Byte>
end

let found: View<Byte> | Nil = text.bytes()
~~~

Functions, methods, and static associated functions may accept a view value
parameter because the caller controls that call-scoped borrow. User-authored
callables may not return a view. Rust bridges retain only the immutable scalar
view parameters already permitted by the bridge contract; views remain
forbidden as bridge results.

This restriction intentionally removes general borrowed aggregates and
last-use lifetime inference. Programs express optional or grouped borrowed
information through control flow inside the `with` body rather than storing a
borrowed sum or struct.

## 13. Interaction with control flow and cleanup

An `if`, `while`, or `for` nested in a `with` body remains within that borrow.
A `return`, `return_on_error`, or `break` that exits the body ends the borrow as
part of that lexical exit, but it may not first move or mutate the borrowed
owner. Returning an unrelated value is valid.

A `defer` registered in the `with` body executes while the view and its source
are still valid. The borrow ends after that deferred action and the body's
other cleanup entries complete. A deferred call may use the view binding but
cannot make it escape.

Sequence iteration retains its existing lexical rule: the iterable is
immutably borrowed for the complete loop body. No separate `with` is required
for `for` iteration because the loop syntax already writes the exact lexical
extent of the borrow.

## 14. Diagnostics

Diagnostics shall use ownership language rather than lifetime-calculus terms.
They shall identify the owner, the move or mutation, and the written call or
`with` block that keeps the borrow active.

Required diagnostic categories include:

- use of an affine place after it was moved;
- a second consumption of one owner;
- moving an affine subplace;
- replacing an owner while a call or `with` block borrows it;
- structurally mutating a collection during a view or iteration borrow;
- overlapping exclusive reference arguments;
- storing or returning a view;
- a `with` initializer that is not a view;
- a `with` view with no compiler-known source; and
- a `with` binding that violates function-wide name uniqueness.

Diagnostics shall not expose internal region variables or suggest authoring a
lifetime annotation, because Snacc has neither concept in source.

## 15. Detailed implementation plan

### Phase 1: grammar and syntax tree

1. Reserve `with` and `as` in the lexer and reject both as identifiers in all
   contexts.
2. Add `with-statement` to the formal grammar in `LANGUAGE.md` and
   `GRAMMAR.ebnf` identically.
3. Parse the initializer, binding name, and body into one source-spanned
   `BlockElement::With` node.
4. Add parser tests for nesting, every surrounding block kind, missing `as`,
   missing `do`, missing `end`, and keyword reservation.

### Phase 2: formal ownership classification

1. Centralize copyable-versus-affine classification in the semantic type
   table and use it for moves, assignment replacement, arguments, returns,
   construction, collection storage, and cleanup.
2. Preserve the existing structural fixed-point computation for represented,
   struct, union, sum, array, box, and generic-specialization types.
3. Rename internal `move_only` terminology to `affine` only where doing so
   improves the semantic invariant; do not perform an unrelated mechanical
   rename.
4. Add table-driven tests for every built-in and structural ownership class.

### Phase 3: remove stored borrowed types

1. Reject direct or transitive borrowed types in ordinary locals, fields,
   represented types, named and inline unions, boxes, arrays, lists, maps, and
   sets.
2. Preserve view parameters and the existing prohibition on view results.
3. Remove checked representations and lowering paths that exist solely to
   carry views inside owning aggregates or stored inline sums.
4. Convert existing valid view-local corpus cases to `with` blocks and add
   negative cases for every removed storage position.

### Phase 4: lexical borrow checking

1. Give each checked `with` node one lexical borrow-scope identity and retain
   the initializer's complete source-root set.
2. Keep that source set active for the complete checked body; do not run
   last-reachable-use pruning on a `with` binding.
3. Permit overlapping immutable borrow scopes and reject any overlapping move,
   replacement, exclusive reference, box transfer, or structural collection
   mutation.
4. Propagate original source roots through slicing and nested views.
5. End the borrow on every normal edge leaving the body, including fallthrough,
   `break`, `return`, and `return_on_error`.
6. Retain call-boundary view loans only for the duration of one call and reject
   same-call overlap with moves or exclusive references.
7. Delete non-lexical last-use pruning and its control-flow merge state after
   all source sites use call scopes, `with`, or iteration scopes.

### Phase 5: checked cleanup and lowering

1. Add `TStmt::With` carrying the checked initializer, immutable binding,
   source facts, and checked body.
2. Lower the initializer exactly once, bind its view descriptor in ordinary
   block-local storage, and lower the body without adding a runtime lifetime
   object or reference count.
3. Run body defers and owned cleanup before leaving the borrow scope. The view
   descriptor itself requires no destruction.
4. Verify that early exits reuse the existing forward-only cleanup plans and
   introduce no backend lifetime reconstruction.

### Phase 6: conformance and contract synchronization

1. Add compile-and-run tests for String byte and Unicode views, array and list
   views, nested slices, multiple immutable scopes, calls receiving views, and
   loop interaction.
2. Add rejection tests for owner moves, replacement, structural mutation,
   `Ref<T>` overlap, view escape, borrowed aggregates, invalid initializers,
   and duplicate binding names.
3. Add cleanup-order tests covering fallthrough, `break`, `return`,
   `return_on_error`, `defer`, and `defer_on_error` inside a `with` body.
4. Run `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`,
   and `cargo test --workspace`.
5. Update the normative ownership and view sections of `LANGUAGE.md` in the
   same implementation change, keeping its embedded grammar identical to
   `GRAMMAR.ebnf` and both synchronized with parser and checker behavior.
6. Move this specification to `docs/specs/archive/` with `Status: Closed` only
   after every acceptance criterion below passes.

## 16. Acceptance criteria

1. Copyable values may be reused; affine owners may be consumed at most once.
2. Every still-owned affine value is destroyed exactly once on every normal
   exit, including early returns and error propagation.
3. No source syntax for manual free, general references, raw pointers, or
   lifetime annotations exists.
4. `Ref<T>` remains call-scoped, exclusive, automatic-dereference access.
5. Temporary immutable views and call-boundary view loans remain zero-copy.
6. `with view_expression as name do ... end` evaluates its initializer once
   and keeps its source borrowed for exactly the written body.
7. Moving, replacing, exclusively borrowing, or structurally mutating the
   source within that body is rejected even after the view's last use.
8. Nested views retain the original owner and multiple immutable view scopes
   may overlap.
9. Ordinary locals and owning types cannot store views directly or
   transitively; user callables and bridges cannot return them.
10. `for` iteration continues to borrow its iterable for the complete loop
    body.
11. The checked representation carries every ownership and borrow fact needed
    by lowering; the backend performs no source-level lifetime inference.
12. `LANGUAGE.md`, its embedded EBNF, `GRAMMAR.ebnf`, parser behavior, checker
    behavior, and conformance tests agree exactly when this specification is
    closed.

## 17. Consequences

This design rejects some programs that are safe under last-use analysis. The
rejected program can make its lifetime explicit by shortening or nesting a
`with` block. In exchange, moving a later statement no longer silently changes
where an earlier borrow ends.

Affine ownership remains the mechanism that prevents leaks, double frees, and
use-after-move without a garbage collector. Explicit lexical borrowing keeps
the aliasing model substantially smaller than a general ownership-and-lifetime
system while preserving zero-copy access and deterministic cleanup.

The language still cannot directly express every safe shared or externally owned
resource pattern. Shared ownership, opaque host resources with destruction,
and mutable contiguous borrowing remain separate, explicit design decisions
rather than being smuggled into the core view type.

## 18. Normative reference

Until this specification is implemented and closed, [LANGUAGE.md](../../LANGUAGE.md)
remains authoritative. In particular, its current last-reachable-use view
rules remain in force; this Proposed specification does not change compiler
behavior by itself.

# Specification 031: Generational Arenas

Status: Proposed

Document kind: Language semantics (ISO/IEC-style specification)

## 1. Proposal state

This implementation-ready specification adds two closed built-in type forms:

~~~snacc
Arena<T>
ArenaKey<T>
~~~

An `Arena<T>` is one affine owner of a dynamically sized set of `T` values.
An `ArenaKey<T>` is a copyable, non-owning, generational identifier for one
logical slot. Keys are not pointers and never grant access without their
arena.

The design permits cyclic and multiply referenced data structures without
garbage collection, reference counting, raw pointers, general references, or
source-level lifetime parameters. Scoped element access extends the explicit
borrow construct proposed by
[Specification 030](030-affine-ownership-and-explicit-borrow-scopes.md).

There are no open design questions. Section 16 is the detailed implementation
plan and fixes the required implementation order.

Until this specification is implemented and closed, [LANGUAGE.md](../../LANGUAGE.md)
remains the sole normative language contract and neither type form nor the
arena access rules described here exist.

## 2. Goals

This specification shall provide:

- compact ownership of many same-typed values;
- bulk deterministic destruction;
- stable logical identity across arena growth and movement;
- safe cyclic graphs, parent links, backlinks, and cross-references;
- stale-key and wrong-arena detection without memory unsafety;
- immutable and mutable element access with one written lexical extent;
- exact structural ownership for affine elements; and
- useful keys for map and set membership.

The user-facing model is:

> The arena owns values. Keys identify slots. A `with` block temporarily
> accesses one slot.

## 3. Non-goals

This specification does not add:

- raw or typed memory addresses;
- manual allocation or deallocation;
- references that can be stored or returned;
- inferred or author-written lifetime parameters;
- shared ownership of the arena;
- concurrent arena mutation;
- compacting garbage collection;
- inheritance, object identity for ordinary structs, or reference equality;
- heterogeneous arenas;
- user-selected allocators;
- iteration that yields keys;
- stable element addresses outside a `with` block; or
- arena values or keys across a Rust bridge.

An arena may relocate its internal allocation whenever no element borrow is
active. Only keys, not addresses, remain stable.

## 4. Type formation

`Arena` and `ArenaKey` are reserved built-in type constructors. Each requires
exactly one storable, non-borrowed value type argument:

~~~snacc
Arena<Node>
ArenaKey<Node>
Arena<Box<Node>>
~~~

`Arena<Ref<T>>`, `Arena<View<T>>`, `ArenaKey<Ref<T>>`, and a no-result type
argument are invalid. A type transitively containing a view is also invalid.

`Arena<T>` has an implementation-private descriptor layout independent of
`T`. It is affine regardless of whether `T` is copyable.

`ArenaKey<T>` has a fixed, copyable representation independent of `T`. The
type argument participates in exact type identity:

~~~text
ArenaKey<Node> != ArenaKey<Token>
~~~

There is no implicit conversion between keys with different arguments.
`ArenaKey<T>` owns no `T`, does not keep an arena alive, and requires no
destruction.

Because both forms have finite representation independent of `T`, an
`ArenaKey<Node>` field is a valid indirection edge in `Node` and does not form
an infinite by-value layout.

## 5. Construction and ownership

The sole empty constructor is the complete type application:

~~~snacc
let mut nodes: Arena<Node> = Arena<Node>()
~~~

It accepts no value arguments. The arena initially has length zero.

Moving the arena transfers its complete allocation and identity. Existing
keys remain associated with that logical arena after the move:

~~~snacc
let first: Arena<Node> = Arena<Node>()
let second: Arena<Node> = first
// keys issued by first are now used with second
~~~

The moved source is unavailable. Copying or cloning an arena is unsupported.
Dropping an arena destroys every live element exactly once and then releases
all arena storage. Keys may syntactically outlive the arena because they own no
memory; without the matching live arena they cannot access an element.

## 6. Key representation and validity

Each key logically contains:

- the arena's runtime identity;
- a slot index; and
- that slot's generation.

These fields are private and cannot be selected, constructed, converted to
integers, or modified by source code.

A key is valid for an arena exactly when:

1. its type argument exactly matches the arena's element type;
2. its arena identity matches;
3. its slot exists and is occupied; and
4. its generation equals the slot's current generation.

Removal invalidates the removed key before control returns to source code.
Reusing the slot uses a different generation. Clearing the arena invalidates
every previously issued key. Reserving capacity, growing storage, and moving
the arena do not invalidate keys.

Arena identities and generations shall never wrap into a value that could
make an old key valid again. An implementation may retire a slot whose next
generation would overflow. Exhausting the arena-identity space terminates
through a defined runtime fatal path before reusing an identity.

A wrong-arena or stale key never produces a dangling address. Operations
either report absence or terminate through the defined `InvalidArenaKey`
runtime path, as specified per operation below.

## 7. Operation surface

`Arena<T>` exposes exactly:

~~~text
length() -> Int64
is_empty() -> Bool
capacity() -> Int64
reserve(minimum: Int64)
insert(value: T) -> ArenaKey<T>
contains(key: ArenaKey<T>) -> Bool
delete(key: ArenaKey<T>) -> Bool
take(key: ArenaKey<T>) -> T
clear()
~~~

There are no aliases for these operations.

### 7.1 Observation

`length`, `is_empty`, and `capacity` do not borrow an element and do not
modify the arena. Capacity is at least length and remains otherwise
implementation-defined.

`contains` returns `true` exactly for a currently valid key of that arena. It
returns `false` for a stale or wrong-arena key. A key with a different type
argument is rejected statically.

### 7.2 Insertion and capacity

`insert` evaluates and consumes its value exactly once, installs it into one
vacant logical slot, and returns that slot's current key. If allocation is
required and fails, execution terminates through the runtime allocation-fatal
path; no partially inserted source-visible value exists.

`reserve(minimum)` requires a non-negative minimum capacity. A negative value
terminates through a defined bounds error. It may relocate arena storage but
does not change length, elements, keys, or arena identity.

### 7.3 Removal

`delete` destroys a valid element, invalidates its key, and returns `true`.
For a stale or wrong-arena key it changes nothing and returns `false`.

`take` moves a valid element out, invalidates its key, and returns the complete
`T` without destroying it. A stale or wrong-arena key terminates through
`InvalidArenaKey`.

`clear` destroys every live element, sets length to zero, and invalidates all
keys issued before the clear. It may retain allocated capacity.

No structural operation above is permitted while any element borrow from that
arena is active.

## 8. Key equality and collection use

Two values of the same `ArenaKey<T>` type support `==` and `!=`. They compare
their complete private identity, slot, and generation. Keys with different
type arguments cannot be compared.

Keys support no ordered comparison, arithmetic, printing, numeric conversion,
truthiness exception, field access, or direct construction. Like every value
other than `false` and active `Nil`, a key is truthy.

`ArenaKey<T>` is a permitted `Map` key and `Set` element. Hashing uses the same
complete identity used by equality, so equal keys have equal hashes and keys
from different arenas remain distinct:

~~~snacc
let mut visited: Set<ArenaKey<Node>> = Set<ArenaKey<Node>>()
visited.insert(root)
~~~

This extends the currently closed map/set key set by exactly the
`ArenaKey<T>` family. `Arena<T>` itself supports neither equality nor hashing.

## 9. Immutable element access

Arena indexing is a borrow source and is valid only as the direct initializer
of `with`:

~~~snacc
with nodes[key] as node do
    print(node.value)
end
~~~

Outside that position, `nodes[key]` is rejected. It never produces a storable
reference or a copied `T`.

The arena and key expressions each evaluate exactly once, from left to right.
The key is validated before the body begins. An invalid key terminates through
`InvalidArenaKey`; the body does not execute.

The binding denotes the element as an immutable borrowed place of exact type
`T`. It may be read, have fields selected, call read-only methods, and copy
copyable subvalues. It cannot be assigned, moved, returned, boxed, stored,
passed as `Ref<T>`, or consumed by value.

The immutable borrow lasts for the complete written body. During it:

- the arena cannot be moved, replaced, destroyed, cleared, reserved, grown,
  inserted into, deleted from, or taken from;
- another immutable element borrow from the same arena is permitted; and
- a mutable element borrow from the same arena is rejected.

Observation operations and `contains` remain valid during immutable access.

## 10. Mutable element access

This specification extends Specification 030's `with` production to:

~~~ebnf
with-statement = "with", expression, "as", [ "mut" ], identifier,
                 "do", block, "end" ;
~~~

The optional `mut` is valid only when the initializer is an arena index. A
view binding remains immutable.

Mutable arena access is written:

~~~snacc
with nodes[key] as mut node do
    node.value = node.value + 1
end
~~~

The arena expression must resolve to a mutable owning root. The binding is a
mutable borrowed place of exact type `T`. It permits whole-element assignment,
field assignment, and receiver-writing method calls. Whole-element assignment
destroys the previous element before installing the new one but preserves the
slot, generation, and key validity.

The element or any affine subplace cannot be moved out. The complete arena
cannot be moved or structurally modified while the borrow is active.

For simplicity, any other element borrow from the same arena is rejected while
a mutable element borrow is active, even when two runtime keys would identify
different slots. Snacc does not attempt value-dependent key-disjointness
proofs.

Observation operations and `contains` remain valid during mutable access.

## 11. Borrow lifetime and exits

An arena element borrow begins only after the arena and key have evaluated and
the key has been validated. It ends when control exits the written `with`
body. Last-use inference does not shorten it.

Fallthrough, `return`, `return_on_error`, and `break` end the borrow as part of
leaving the body. Such an exit may return an unrelated value but cannot move
the borrowed element, its arena, or an affine value obtained as a forbidden
subplace move.

Deferred calls registered within the body execute while the element binding
and arena remain valid. The element borrow ends after applicable body cleanup
has completed. Neither the binding nor a reference to it can escape through a
defer declared outside the body.

## 12. Iteration

An arena supports value iteration using the existing single-binding `for`
form:

~~~snacc
for node in nodes do
    print(node.value)
end
~~~

The loop binding is an immutable borrowed element place valid only for that
iteration. Iteration order is unspecified and must not be used to infer a slot
index, generation, or insertion order.

The arena is immutably borrowed for the complete loop. No structural arena
operation or mutable element borrow is permitted in the body. Empty arenas
execute no iteration. This specification adds no implicit key or index
binding; programs retain keys returned by `insert` when identity is required.

## 13. Cyclic data structures

Keys may be stored inside their own target type because they are non-owning,
fixed-layout values:

~~~snacc
type Node is struct
    value: Int64
    edges: List<ArenaKey<Node>>
end

let mut nodes: Arena<Node> = Arena<Node>()
let first: ArenaKey<Node> = nodes.insert(Node(value: 1, edges: []))
let second: ArenaKey<Node> = nodes.insert(Node(value: 2, edges: [first]))

with nodes[first] as mut node do
    node.edges.push(second)
end
~~~

The two nodes may now refer to one another without either owning the other.
Destroying the arena destroys both nodes exactly once; key cycles require no
cycle collector.

A stale edge is possible after deletion. It is a logical absence, not a memory
error: `contains` detects it, `delete` returns `false`, and indexed access or
`take` terminates through `InvalidArenaKey`.

## 14. Rust bridge and runtime boundary

Neither `Arena<T>` nor `ArenaKey<T>` may appear in an `extern rust` parameter
or result. Their private identity, generation, storage, and destruction rules
have no stable host representation.

The compiler/runtime boundary shall use pointer-safe descriptor inputs,
caller-provided output storage for aggregate results, and compiler-generated
element ownership descriptors where `T` is not a scalar. No LLVM declaration
may assume an unverified by-value aggregate ABI.

Adding the arena descriptor, key representation, runtime symbols, fatal error,
or ownership rules changes the internal compiler/runtime ABI. Implementation
shall advance the ABI version once, update compiler and runtime together, and
invalidate older cached objects.

## 15. Diagnostics

Required compile-time diagnostics include:

- wrong arity for either type constructor;
- a borrowed, reference, or no-result type argument;
- an operation receiving the wrong `ArenaKey<U>` type;
- arena indexing outside a direct `with` initializer;
- `as mut` on an ordinary view;
- mutable access through an immutable arena root;
- moving or structurally modifying an arena while an element borrow is live;
- overlapping mutable and immutable borrows of one arena;
- moving, returning, boxing, storing, or exclusively reborrowing an element
  binding;
- using an element binding outside its body; and
- attempting unsupported equality, ordering, printing, or conversion.

Runtime invalid-key diagnostics shall distinguish an invalid arena key from a
numeric collection-bounds failure. They need not reveal private arena
identities or addresses.

## 16. Detailed implementation plan

### Phase 0: dependency

1. Implement and close Specification 030 before adding arena element access.
   Its affine terminology, `with` block, lexical-exit rules, and removal of
   stored borrowed values are prerequisites rather than parallel alternatives.

### Phase 1: grammar and parsed representation

1. Reserve `Arena` and `ArenaKey` as built-in type names.
2. Extend the type grammar with their exact one-argument forms.
3. Extend `with-statement` with optional `mut` after `as`, preserving the one
   immutable spelling from Specification 030.
4. Parse `arena[key]` through the existing index syntax and retain enough
   context to diagnose its use outside `with` semantically.
5. Add lexer and parser tests for type arity, nested type applications,
   immutable and mutable access, malformed delimiters, and keyword
   reservation.

### Phase 2: semantic types and ownership

1. Add interned semantic identities for `Arena<T>` and `ArenaKey<T>` using the
   complete resolved `T`.
2. Make every arena affine and every key copyable; treat a key edge as finite
   during recursive-layout analysis.
3. Reject borrowed, reference, and no-result arguments and propagate ordinary
   `T` storage validation.
4. Add keys to equality support and to map/set key validation; add neither
   ordering nor printing.
5. Add table-driven tests for identity, recursive layout, structural affine
   propagation, equality, hashing eligibility, and bridge rejection.

### Phase 3: checked operation surface

1. Add checked nodes for construction, observation, reserve, insert, contains,
   delete, take, clear, and iteration.
2. Reuse ordinary move checking so insert consumes `T`, take produces one new
   owner, delete destroys in place, and clear destroys all live entries.
3. Enforce exact receiver mutability and argument arity/type rules.
4. Reject every unlisted method name and every ordinary arena index
   expression.
5. Add positive and negative checker tests for every operation and ownership
   transition.

### Phase 4: scoped element borrowing

1. Extend the checked `with` node with an arena-element source carrying the
   arena place, key expression, exact element type, and immutable-or-mutable
   access mode.
2. Evaluate the arena and key once, validate the key, and bind one borrowed
   place for the complete body.
3. Track borrows by arena owning root. Permit multiple immutable scopes and
   reject every additional borrow while a mutable scope is active.
4. Block arena movement, replacement, structural operations, and exclusive
   reference arguments for the borrow's complete lexical extent.
5. Permit in-place whole-element and field replacement through `as mut` while
   preserving the slot generation.
6. Reject subplace moves and every escape route.
7. Integrate fallthrough, return, propagation, break, defer, and iteration
   edges with the unified checked cleanup plan.

### Phase 5: runtime storage

1. Implement an opaque arena descriptor containing allocation state, length,
   capacity, arena identity, slot layout information, and compiler-provided
   element operations.
2. Implement occupied slot metadata and generations without exposing a source
   address. Reuse vacant slots only with a new generation and retire a slot
   before generation wrap.
3. Implement insertion, validation, deletion, take, clear, reserve, iteration,
   and destruction through pointer-safe runtime entry points.
4. Use compiler-generated element move/drop descriptors for every non-scalar
   `T`; do not duplicate type-specific ownership logic in the runtime.
5. Add `InvalidArenaKey` and identity-exhaustion fatal paths with deterministic
   process failure.
6. Add runtime unit tests for allocation growth, alignment, slot reuse,
   generations, wrong-arena keys, clear, move-only elements, and exact
   destruction counts.

### Phase 6: LLVM lowering and ABI

1. Lower arenas and keys through target-correct private layouts while keeping
   all descriptor-bearing calls pointer-safe.
2. Lower `with` access to one validation followed by a body-scoped element
   pointer that never enters a storable checked value.
3. Lower mutable whole-element assignment as destroy-then-store without
   changing key metadata.
4. Lower iteration without exposing slot metadata or yielding vacant slots.
5. Add LLVM verification tests for scalar, aligned aggregate, String, Box,
   collection, and recursively keyed node element types.
6. Advance and verify the compiler/runtime ABI version and cache invalidation.

### Phase 7: conformance and contract synchronization

1. Add native run tests for insertion, observation, removal, capacity, clear,
   immutable and mutable access, iteration, and movement of the owning arena.
2. Add graph tests for a tree, linked list, directed cycle, stale backlink,
   and set/map membership by key.
3. Add rejection tests for every diagnostic category in section 15.
4. Add cleanup tests proving exact destruction for normal fallthrough, early
   return, error propagation, defer ordering, deletion, take, clear, and arena
   destruction.
5. Run `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`,
   and `cargo test --workspace`.
6. Update `LANGUAGE.md` and its embedded grammar in the same implementation
   change, keep the embedded grammar identical to `GRAMMAR.ebnf`, and keep
   both synchronized with parser, checker, lowering, and runtime behavior.
7. Move this specification to `docs/specs/archive/` with `Status: Closed` only
   after every acceptance criterion passes.

## 17. Acceptance criteria

1. `Arena<T>` is one affine owner and `ArenaKey<T>` is a copyable non-owner.
2. Keys remain logically valid across arena movement and growth and become
   invalid after removal or clear.
3. A stale or wrong-arena key can never read, write, move, or destroy unrelated
   storage.
4. Arena destruction destroys every remaining live element exactly once.
5. Insert consumes one `T`; take returns one `T`; delete and clear destroy their
   targets exactly once.
6. Keys support exact equality and map/set use, but no ordering, printing,
   numeric conversion, or field access.
7. Arena indexing is usable only through an explicit lexical `with` scope and
   never produces a storable or returnable reference.
8. Immutable access permits other immutable access but blocks structural
   mutation; mutable access is exclusive for the complete arena.
9. Mutable element replacement preserves the key while element removal
   invalidates it.
10. Arena iteration borrows the arena immutably and never yields vacant slots
    or exposes iteration order as a contract.
11. Cyclic graphs built from keys require no shared ownership or cycle
    collector and are destroyed safely with their arena.
12. Neither arenas nor keys cross a Rust bridge.
13. Runtime descriptor calls follow the pointer-safe ABI contract and the ABI
    version advances consistently across compiler, runtime, and cache metadata.
14. The checked representation contains all ownership, borrow, type, and
    cleanup facts required by lowering; the backend performs no source-level
    semantic reconstruction.
15. `LANGUAGE.md`, its embedded grammar, `GRAMMAR.ebnf`, implementation, and
    conformance tests agree exactly when this specification is closed.

## 18. Consequences

Arenas make non-owning cycles safe by replacing memory addresses with checked
logical identities. They do not make stale relationships impossible: deleting
a node can leave keys to it elsewhere. That state remains memory-safe and can
be tested with `contains`; unchecked access fails deterministically.

The runtime pays for arena identity and generation checks on element access.
In return, source code gains stable keys without reference counting, tracing,
or lifetime parameters. The deliberately conservative mutable-borrow rule may
reject simultaneous mutation of provably different elements; a later design
may add a bulk disjoint-access operation only if a concrete use case justifies
the additional surface.

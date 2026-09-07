# RFC 029: Source File Decomposition and Dependency Offloading

Status: Closed

Document kind: Repository refactor (Rust-style RFC)

## 1. Proposal state

This RFC is implementation-ready. It is a **behavior-preserving** refactor: no
language rule, diagnostic, ABI version, generated artifact, or observable
compiler output changes. Every phase is a mechanical move or a like-for-like
substitution verified by the existing test suite.

It contains no open design questions. Section 9 fixes the phase order.

## 2. Measured problem

Current state, measured rather than estimated:

| File | Total | Production | Inline tests |
| --- | ---: | ---: | ---: |
| `semantics/checker.rs` | 9890 | 6481 | 3409 |
| `backend/llvm.rs` | 6837 | 6837 | 0 |
| `snacc-runtime/src/lib.rs` | 3092 | 3092 | 0 |
| `semantics/types.rs` | 1984 | 1865 | 119 |
| `syntax/parser.rs` | 1896 | 1069 | 827 |
| `apps/cargo-snacc/src/main.rs` | 1738 | 1457 | 281 |
| `syntax/lexer.rs` | 1636 | 1134 | 502 |
| `apps/cargo-snacc/tests/cargo_hosted.rs` | 1589 | — | 1589 |
| `apps/snacc-workbench/src/lib.rs` | 1260 | 689 | 571 |

The workspace holds 32,615 lines across 30 Rust files. **Ten files exceed 500
lines**, and the nine above plus `syntax/ast.rs` at 510 are all of them, so the
problem is concentrated rather than pervasive. `ast.rs` is 2% over target and is
left alone. The remaining 20 files are already within target and this RFC does
not touch them.

Three distinct causes account for nearly all of the excess:

1. **Inline tests.** 5,709 lines of `#[cfg(test)]` live inside production files.
   `checker.rs` is 35% tests. Moving them changes no behavior and is the single
   largest reduction available.
2. **Two oversized functions and one oversized `impl`.** `llvm.rs` is one
   ~5,800-line `impl Codegen` holding 110 methods. Two of them — `expr` (1,741
   lines) and `stmt` (745 lines) — are 36% of the file on their own.
3. **Hand-written repetition.** 46 `*_import` methods in `llvm.rs` span ~1,185
   lines of near-identical extern declarations. `snacc-runtime` spells out every
   monomorphized symbol name twice: once in 38 macro invocations and again in a
   312-line `force_link`.

## 3. Goals

- Bring production files toward a **soft 500-line target**.
- Split along seams that already exist in the code, not along invented ones.
- Replace hand-written repetition with data tables or generated identifiers.
- Offload well-understood work to established crates where the crate genuinely
  removes code.
- Preserve every diagnostic, generated artifact, symbol name, and test result.

## 4. Non-goals

- Changing any language rule, type rule, diagnostic text, or ABI version.
- Changing crate boundaries or the `apps/` and `crates/` split settled by the
  archived workspace RFC.
- Introducing traits, generics, or indirection whose only purpose is to make a
  file smaller. A trait with one implementation is not a decomposition.
- Rewriting the LLVM backend, checker algorithms, or borrow analysis.
- Enforcing 500 lines as a hard cap. See section 5.
- Adding a dependency that saves fewer lines than its integration costs.

## 5. The 500-line target is a guideline

A hard cap would itself be over-engineering. Some units have a natural size
floor: a `match` over the complete expression AST has one arm per variant, and
splitting it across files to satisfy a number makes it harder to read, not
easier.

The rule this RFC adopts:

> A production file over 500 lines is acceptable only when it is one cohesive
> unit whose split would require inventing an abstraction. Such a file is listed
> explicitly below with its reason.

Permitted exceptions after this refactor:

| File | Expected size | Reason |
| --- | ---: | --- |
| `ast.rs` | 510 | One cohesive AST definition; splitting individual node families would obscure the syntax tree |
| `types.rs` | 1884 | Built-in, nominal, represented, generic, collection, and recursive-layout rules share one type-resolution table |
| `lexer/mod.rs` | 1351 | Token definitions, the main scanner, and interpolation dispatch share the lexer state and token-boundary rules |
| `checker/mod.rs` | 1295 | Checked-IR definitions, context state, and the top-level program walk form one shared checker unit |
| `checker/program.rs` | 834 | Declaration collection, signatures, generic declarations, and program-wide validation share symbol tables |
| `checker/expr.rs` | 670 | Expression checking is one exhaustive expression-shape unit |
| `checker/stmt.rs` | 1334 | Statement, block, control-flow, cleanup, and return-flow checking share block state |
| `checker/places.rs` | 742 | Place, move, borrow, overlap, and view-liveness analysis share the same dataflow state |
| `checker/calls.rs` | 1717 | Function, method, constructor, static, and bridge call checking share argument and receiver validation |
| `llvm/lower/mod.rs` | 789 | Codegen state, module construction, calls, and shared lowering helpers require one context |
| `llvm/lower/expr.rs` | 1130 | One exhaustive match over expression variants |
| `llvm/lower/stmt.rs` | 1099 | Statement and block lowering share terminator and cleanup state |
| `llvm/lower/equality.rs` | 739 | Structural equality and error-propagation lowering share aggregate layout traversal |
| `llvm/lower/collections.rs` | 1206 | List, map, and set operation lowering shares runtime descriptors and ownership moves |
| `llvm/lower/cleanup.rs` | 501 | Drop and cleanup lowering is one cohesive operation table |
| `snacc-runtime/src/lib.rs` | 510 | Runtime ABI vocabulary and the symbol-set golden test stay together at the crate boundary |
| `snacc-runtime/src/map.rs` | 1249 | Map runtime families share storage synchronization and generated ABI symbols |
| `snacc-runtime/src/set.rs` | 510 | Set runtime families share storage synchronization and generated ABI symbols |

Everything else is expected under 500 lines. A future file that exceeds it
either splits or is added to this table with a reason.

These are measured post-refactor sizes. The exceptions are deliberately
explicit: they are cohesive units whose further separation would require
inventing a trait, duplicating shared state, or scattering an exhaustive
variant table. A future file that exceeds its recorded size by more than 20%
must be reviewed and either split or receive a new, justified RFC change.

## 6. Target structure

### 6.1 `snacc-compiler`

~~~text
crates/snacc-compiler/src/
  lib.rs
  diagnostics.rs
  ast.rs
  lexer/
    mod.rs            token definitions, scanning, numeric and text rules
  parser/
    mod.rs            entry point and item parsing
    types.rs          type syntax, sums, parameterized forms
    expr.rs           expression and postfix parsing
    stmt.rs           blocks, declarations, control flow
  types.rs
  checker/
    mod.rs
    program.rs        declaration collection, signatures, generics
    expr.rs           expression checking
    stmt.rs           statement and block checking
    places.rs         places, moves, borrows, view liveness
    calls.rs          call, method, constructor, bridge argument checking
    convert.rs        coercion, common types, injection
  llvm/
    mod.rs
    types.rs          LLVM type mapping and layout
    imports.rs        runtime import declarations (table-driven)
    symbols.rs        runtime symbol name construction (table-driven)
    lower/
      mod.rs          Codegen struct and shared helpers
      expr.rs         expression lowering
      stmt.rs         statement and block lowering
      equality.rs     structural equality lowering
      cleanup.rs      drop and cleanup lowering
      collections.rs  list, map, set operation lowering
~~~

`Codegen` keeps one struct with its `impl` blocks split across the `lower/`
modules. Rust permits multiple `impl` blocks for one type in one crate, so this
requires no trait, no field changes, and no visibility widening beyond
`pub(crate)`. The phase names are direct modules: there is no `syntax/`,
`semantics/`, or `backend/` umbrella. `llvm/` is the concrete backend because
Snacc has one backend; a future second backend may introduce an umbrella only
when that additional implementation exists.

### 6.2 `snacc-runtime`

~~~text
crates/snacc-runtime/src/
  lib.rs              re-exports and force_link
  print.rs
  string.rs
  view.rs
  list.rs
  map.rs
  set.rs
  fail.rs             allocation and invalid-operation failure paths
~~~

### 6.3 `cargo-snacc`

The module split the archived workspace RFC described but deliberately did not
require. It is required now because the file has grown past 1,700 lines.

~~~text
apps/cargo-snacc/src/
  main.rs             process entry and error reporting
  cli.rs              option parsing and command dispatch
  metadata.rs         package selection and Snacc metadata
  commands/
    mod.rs
    init.rs
    build.rs          check, build, run, test, clean
    doctor.rs
  cache.rs            build identity, manifest, publication
  bridge.rs           assertion rendering and host validation
  templates/
    host.rs           generated host source
    host_pre_ferris.rs
    host_pre_rfc_007.rs
~~~

The three host templates are currently single-line escaped string literals up to
1,200 characters wide. They become real `.rs` template files loaded with
`include_str!`, which makes them readable, diffable, and formattable. Their
byte content must not change; a test asserts each template still matches the
value it had before the move.

### 6.4 `snacc-workbench`

~~~text
apps/snacc-workbench/src/
  main.rs
  lib.rs              server setup and shared state
  http.rs             request parsing and response writing
  routes.rs           route handling and validation
  run.rs              compile-and-execute lifecycle
~~~

## 7. Removing hand-written repetition

### 7.1 Runtime symbol names become one table

`llvm.rs` builds runtime symbol names with `scalar_map_symbol`,
`scalar_map_raw_symbol`, `scalar_set_symbol`, `list_scalar_symbol`,
`map_symbol`, and `set_symbol` — six parallel functions of long `match` arms —
and then declares each result through one of 46 `*_import` methods whose bodies
differ only in signature.

Both collapse into one table in `llvm/symbols.rs`:

~~~rust
struct RuntimeImport {
    family: Family,      // Map, Set, List, String, View, Print
    operation: &'static str,
    params: &'static [AbiTy],
    result: Option<AbiTy>,
}
~~~

with one lookup that constructs the name and one generic `declare` that builds
the signature. The generated LLVM declarations must be byte-identical to those
produced today; a test compares emitted IR for a program exercising every family
before and after.

### 7.2 Runtime macros generate their own identifiers

`snacc-runtime` already uses macros, but each invocation must spell out all ten
generated symbol names, and `force_link` spells them out a second time. Adding
the **`paste`** crate lets the macro build `snacc_map_ ## $key ## _ ## $value ##
_insert` itself:

~~~rust
scalar_map_runtime!(bool, i64, u8);   // was 12 lines
~~~

Each invocation drops from ~12 lines to one, and `force_link` is generated by
the same macro instead of hand-listed. Exported symbol names must not change; a
test asserts the exported symbol set is identical before and after.

## 8. Dependency offloading

Only substitutions where the crate removes more code than it adds:

| Crate | Replaces | Removed | Justification |
| --- | --- | ---: | --- |
| `paste` | Hand-spelled macro identifiers and `force_link` | ~600 | Tiny, stable, no runtime cost, no ABI effect |
| `cargo_metadata` | Hand-written `Metadata`/`Package`/`Target` structs and their `serde_json` parsing | ~120 | The canonical typed reader for `cargo metadata`; tracks Cargo's schema so we do not |

**Considered and rejected:**

- **An HTTP crate for the workbench.** ~200 lines of hand-rolled HTTP/1.1
  parsing in `apps/snacc-workbench/src/http.rs` is a genuine maintenance
  surface, and `tiny_http` would remove it. But the workbench was specified with
  no external HTTP dependency, it is loopback-only and developer-facing, and
  swapping the server changes request handling — which is behavior, not
  structure. This RFC is behavior-preserving, so the substitution belongs to a
  separate change that can re-run the workbench's request-rejection tests as its
  acceptance evidence. Section 6.4 isolates the code so that change is small.
- **Replacing `chumsky` or `inkwell`.** Both already are the offload.
- **A general "collections runtime" crate.** The runtime's map and set behavior
  is defined by Snacc's ownership and ABI rules; no crate implements that
  contract.

## 9. Detailed implementation plan

Each phase ends with `cargo fmt --all -- --check`, `cargo check --workspace
--all-targets`, and the complete workspace test suite passing. **No phase
changes logic.** A phase that cannot be completed without a logic change stops
and reports instead.

### Phase 1: move inline tests out

1. Move each `#[cfg(test)] mod tests` into a sibling `tests.rs` included with
   `#[cfg(test)] mod tests;`, or into the crate's `tests/` directory when it
   exercises only the public API.
2. Adjust `use super::*` to explicit imports; widen visibility to `pub(crate)`
   only where a moved test requires it.
3. Confirm the test count is unchanged before and after: the suite must report
   the same number of passing tests.

Expected effect: 5,709 lines leave production files. `checker.rs` drops to
6,481, `parser.rs` to 1,069, `lexer.rs` to 1,134 with no other work.

### Phase 2: split the checker

1. Create `checker/` and move the checked-IR type definitions,
   `Ctx`, and the program walk into `mod.rs`.
2. Move the existing function groups into `program.rs`, `expr.rs`, `stmt.rs`,
   `places.rs`, `calls.rs`, and `convert.rs` along the boundaries the current
   function order already follows.
3. Change no function body. Adjust only visibility and imports.

### Phase 3: split the backend

1. Create `llvm/` and move type mapping, layout, and entry points.
2. Split `impl Codegen` across `lower/` modules as additional `impl` blocks.
3. Split `expr` and `stmt` by delegating each match arm group to a private
   method in the owning module. The match arms themselves move unchanged.

### Phase 4: table-drive the runtime interface

1. Replace the six symbol-name functions with one table and lookup.
2. Replace the 46 `*_import` methods with one table-driven declaration path.
3. Assert emitted LLVM IR is unchanged for a program exercising every runtime
   family.

### Phase 5: compress the runtime

1. Add `paste` and rewrite the six macros to generate their own identifiers.
2. Generate `force_link` from the same macro invocations.
3. Split the result into the modules in section 6.2.
4. Assert the exported symbol set is identical before and after.

### Phase 6: split the applications

1. Split `cargo-snacc` into the modules in section 6.3.
2. Move the three host templates to `include_str!` files and assert their bytes
   are unchanged.
3. Adopt `cargo_metadata` in `metadata.rs`, keeping `Selected` and the
   canonical-path rules exactly as they are.
4. Split the workbench into the modules in section 6.4.

### Phase 7: guard the result

1. Add a repository test that fails when a production `.rs` file exceeds 500
   lines unless its path appears in the exception table of section 5.
2. Record the exception table in the test itself so adding an exception is a
   visible, reviewed change.
3. Update `AGENTS.md` or the contributor notes with the file-size rule.

## 10. Verification

This refactor is behavior-preserving, so its evidence is comparative:

- the workspace test suite reports the **same number** of passing tests before
  and after every phase;
- emitted LLVM IR for a program exercising every runtime family is byte-identical
  across Phase 4;
- the exported symbol set of `snacc-runtime` is identical across Phase 5;
- the three generated host templates are byte-identical across Phase 6;
- a direct single-file compile and a Cargo-hosted build produce the same program
  output before and after;
- the object-cache identity is stable for an unchanged compiler source tree and
  changes when the compiler source hash set changes, so a moved implementation
  cannot silently reuse an incompatible object.

## 11. Rejected alternatives

### Enforce a hard 500-line cap everywhere

A cap with no exceptions forces artificial splits of cohesive matches and
invents abstractions to satisfy a number. The soft target plus a reviewed
exception table keeps the pressure without the damage.

### Split `Codegen` into several structs behind a trait

The methods share one context: module, builder, type table, and environment.
Splitting the struct would require passing that context between pieces or
introducing a trait with one implementation. Multiple `impl` blocks give the
file-size benefit with none of the indirection.

### Extract each compiler phase into its own crate

Crate boundaries were settled deliberately, and phases share the checked IR.
Separate crates would force public APIs for what are internal details and slow
every build. Modules provide the separation; crates would add ceremony.

### Rewrite the runtime collections over a crate's hash map

The runtime's map and set semantics are fixed by Snacc's ownership, key
equivalence, and ABI contracts. A crate's map does not implement that contract,
so the adapter would be as large as the code it replaced.

### Do the split and the crate substitutions in one change

A behavior-preserving move and a dependency substitution have different failure
modes and different evidence. Interleaving them means a test failure cannot be
attributed. The phase order separates them.

## 12. Acceptance criteria

1. No production `.rs` file exceeds 500 lines except those listed in section 5
   with a recorded reason.
2. No inline `#[cfg(test)]` module remains in a production file over 500 lines.
3. `checker/mod.rs`, `llvm/mod.rs`, `snacc-runtime/src/lib.rs`,
   `cargo-snacc/src/main.rs`, and `snacc-workbench/src/lib.rs` are decomposed as
   in section 6.
4. Runtime symbol names and import declarations come from one table each.
5. `snacc-runtime` generates its symbol identifiers and `force_link` from its
   macros; the exported symbol set is unchanged.
6. `paste` and `cargo_metadata` are the only added dependencies.
7. No language rule, diagnostic text, ABI version, generated template, symbol
   name, or emitted IR changes.
8. The test suite reports the same number of passing tests as before the
   refactor, and every comparative check in section 10 holds.
9. A repository test enforces the file-size rule and its exception table.
10. `cargo fmt --all -- --check` and `cargo check --workspace --all-targets`
    pass.

## 13. Deferred work

- Replacing the workbench's hand-rolled HTTP with a crate, as its own
  behavior-changing RFC.
- Splitting `apps/cargo-snacc/tests/cargo_hosted.rs` (1,589 lines); test files
  are excluded from the size rule until navigating them is a demonstrated
  problem.
- Reducing `expr` lowering below its natural match size, which requires a
  different lowering strategy rather than a file move.

## 14. References

- [`LANGUAGE.md`](../../LANGUAGE.md)
- [RFC 006: Rust workspace organization](archive/006-workspace-organization.md)
- [RFC 007: Rust bridge signature verification](archive/007-bridge-signature-verification.md)



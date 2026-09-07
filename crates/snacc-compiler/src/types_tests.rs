use super::*;

/// Collects declarations for `source`, asserting there were no
/// declaration-collection errors -- these tests are about the resulting
/// [`Types`] table, not about rejecting malformed input.
fn collected(source: &str) -> Collected {
    let program = crate::parse(source).unwrap_or_else(|d| panic!("{source} should parse: {d:?}"));
    let mut errors = Vec::new();
    let collected = collect(&program, &mut errors);
    assert!(
        errors.is_empty(),
        "expected no declaration errors for {source}, got: {errors:?}"
    );
    collected
}

// Specification 016 section 4.1: `Box<T>`'s identity is exactly its
// pointee type, interned the same way `SumTable` interns member sets.

#[test]
fn box_pointees_intern_to_the_same_id_for_the_same_pointee_type() {
    let collected = collected(
        "type A is struct value: Box<Int64>, end\n\
         type B is struct value: Box<Int64>, end",
    );
    let types = &collected.types;
    let a = types.top_level("A").expect("A exists");
    let b = types.top_level("B").expect("B exists");
    let a_ty = types.def(a).fields().expect("A is a struct")[0].1;
    let b_ty = types.def(b).fields().expect("B is a struct")[0].1;
    assert_eq!(
        a_ty, b_ty,
        "two 'Box<Int64>' fields must intern to the same 'Ty::Box' id"
    );
    let Ty::Box(id) = a_ty else {
        panic!("expected a Box field, got {a_ty:?}")
    };
    assert_eq!(types.box_pointee(id), Ty::Int64);
}

// Specification 016 section 5.1: a box occurrence terminates the by-value
// layout graph, so a recursive type crossing a box edge has a finite
// layout while an unbroken direct cycle still does not.

#[test]
fn a_box_edge_breaks_an_otherwise_infinite_value_layout() {
    let program = crate::parse("type Node is struct next: Node, end")
        .expect("a self-referential field parses");
    let mut errors = Vec::new();
    collect(&program, &mut errors);
    assert!(
        !errors.is_empty(),
        "an unbroken direct value-layout cycle must still be rejected"
    );

    // The identical shape, broken by one Box edge, has a finite layout.
    collected("type Node is struct next: Box<Node>, end");
}

// Specification 016 section 5.3: `Box<T>` is unconditionally move-only,
// and a struct or union propagates move-only status from any field or
// member, transitively through further nesting -- the same structural
// fixed point `supports_equality` already computes for equality.

#[test]
fn a_box_is_always_move_only_regardless_of_a_copyable_pointee() {
    let collected = collected("type Holder is struct value: Box<Int64>, end");
    let types = &collected.types;
    let holder = types.top_level("Holder").expect("Holder exists");
    let box_ty = types.def(holder).fields().expect("Holder is a struct")[0].1;
    assert!(
        types.is_move_only(box_ty),
        "'Box<Int64>' must be move-only even though 'Int64' is copyable"
    );
}

#[test]
fn move_only_is_structural_and_transitive() {
    let collected = collected(
        "type Leaf is struct value: Int64, end\n\
         type Boxy is struct payload: Box<Leaf>, end\n\
         type Wrapper is struct inner: Boxy, end\n\
         type Choice is union\n\
         \x20   | A is struct value: Box<Leaf>, end\n\
         \x20   | B is struct value: Int64, end\n\
         end",
    );
    let types = &collected.types;
    let leaf = Ty::User(types.top_level("Leaf").expect("Leaf exists"));
    let boxy = Ty::User(types.top_level("Boxy").expect("Boxy exists"));
    let wrapper = Ty::User(types.top_level("Wrapper").expect("Wrapper exists"));
    let choice = types.top_level("Choice").expect("Choice exists");
    let b_member = Ty::User(types.member(choice, "B").expect("Choice.B exists"));

    assert!(
        !types.is_move_only(leaf),
        "a plain 'Int64' field is copyable"
    );
    assert!(
        types.is_move_only(boxy),
        "a direct 'Box<T>' field makes its struct move-only"
    );
    assert!(
        types.is_move_only(wrapper),
        "move-only propagates transitively through further struct nesting"
    );
    assert!(
        types.is_move_only(Ty::User(choice)),
        "a union is move-only when any member's payload is"
    );
    assert!(
        !types.is_move_only(b_member),
        "one move-only member does not make every member move-only"
    );
}

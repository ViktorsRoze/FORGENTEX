# Forgentex Ownership Model

Stage 5 introduces the first ownership and borrowing rules for Forgentex. The
goal is not to be Rust-complete; the goal is to make unsafe memory behavior
visible and reject the most important classes of mistakes in the current demo
language.

## Value Categories

The checker classifies values into these categories:

- copy values: `bool`, primitive numbers, slices, and raw pointers.
- owned values: structs, arrays, and named generic values such as `Option[T]`
  and `Result[T,E]`.
- shared borrows: `&[T]`.
- mutable borrows: `&mut[T]`.
- raw pointers: `*constT` and `*mutT`.

Owned values move when they are passed to an owned parameter, returned by value,
assigned into another owned local, or stored into an owned struct field. A moved
local cannot be used again unless it is assigned a new value.

```forgentex
struct Packet:
    value: u8

fn bad(packet: Packet) -> u8:
    let moved: Packet = packet
    return packet.value      # use_after_move
```

Copy values can be read repeatedly.

The v0.14 ownership increment tracks moves through owned struct fields. Moving
`pair.left` does not move `pair.right`, but it does leave `pair` partially moved
until `pair.left` is assigned again.

```forgentex
struct Pair:
    left: Packet
    right: Packet

fn ok(pair: Pair) -> u8:
    let left: Packet = pair.left
    return pair.right.value  # accepted

fn bad(pair: Pair) -> Pair:
    let left: Packet = pair.left
    return pair              # use_after_move: partially moved value
```

## Borrows

Slices are the v0 borrow surface:

```text
&[T]      shared borrow
&mut[T]   mutable borrow
```

Mutating an indexed value requires a mutable slice or mutable raw pointer target.
Safe code cannot mutate through `&[T]`.

Calls are checked for obvious aliasing of mutable borrows. Passing the same root
value to two mutable-borrow parameters, or to a mutable and shared borrow in the
same call, is rejected.

```forgentex
fn fill(left: &mut [u8], right: &mut [u8]) -> void:
    return

fn bad(xs: &mut [u8]) -> void:
    fill(xs, xs)             # mutable_borrow_alias
```

The checker is intentionally conservative. Stage 27 resolves local borrow aliases
back to their original root before call alias checks, so `let alias: &mut [u8] =
xs; fill(xs, alias)` is rejected. It still does not prove that two slice
expressions describe disjoint memory.

The v0.17 ownership increment also treats local borrow aliases as lexical locks
for indexed writes. Writing through the original root while another live alias
points at that root is rejected until the alias is reassigned.

```forgentex
fn bad(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    xs[0] = 1                # mutable_borrow_alias

fn ok(xs: &mut [u8]) -> void:
    let alias: &mut [u8] = xs
    alias[0] = 1             # accepted
```

## Reference Returns

V0 has no lifetime syntax. The checker infers one safe case: returning a borrow
that is directly tied to a function parameter or a local alias of a parameter.

```forgentex
fn same(xs: &[u8]) -> &[u8]:
    return xs                # accepted
```

The v0.15 ownership increment also summarizes simple borrow-return helpers, so
parameter-origin evidence can flow through direct calls.

```forgentex
fn also_same(xs: &[u8]) -> &[u8]:
    return same(xs)          # accepted
```

The v0.16 ownership increment can also join multiple proven parameter-origin
returns through simple conditional control flow.

```forgentex
fn choose(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:
    if flag:
        return xs
    return ys

fn caller(flag: bool, xs: &[u8], ys: &[u8]) -> &[u8]:
    return choose(flag, xs, ys)  # accepted
```

Returning a reference from an unknown call, local temporary, field, index, or
complex control-flow origin is rejected because the lifetime is not proven.
Later stages can relax this with explicit lifetime parameters and deeper
interprocedural reasoning.

## Raw Pointers

Raw pointer values can appear in types, but raw pointer behavior is unsafe.

Safe functions reject:

- indexing a raw pointer, because indexing is a dereference.
- passing a raw pointer to another function.
- returning a raw pointer.

Use `unsafe fn` for review-only raw pointer behavior in v0.

```forgentex
fn bad(ptr: *const u8) -> u8:
    return ptr[0]            # raw_pointer_dereference_requires_unsafe

unsafe fn ok(ptr: *const u8) -> u8:
    return ptr[0]
```

## Deferred Work

These pieces are intentionally deferred:

- explicit borrow expressions such as `&x`.
- named lifetime parameters.
- proving disjoint slice ranges and other advanced alias proofs.
- full interprocedural lifetime inference.
- traits such as `Copy` or `Clone`.
- raw pointer alias validation beyond unsafe gating.

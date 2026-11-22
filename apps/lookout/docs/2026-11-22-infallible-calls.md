# Checking infallible calls

*Investigation of 2026-11-22, prompted by an allowance of `clippy::unwrap_used` whose reason states a
relation between two default radii. It records what makes each such reason in the workspace true,
and how a build, a test, or a proof can check it. What holds after a choice belongs in the Rust
rules, `.claude/rules/rust.md`. The rest goes stale.*

This doc covers the `unwrap` and `expect` calls that the Rust rules permit because they cannot fail,
and the reasons given for them. It is for whoever writes such a call, or the reason beside it.

The findings hold for Rust 1.100.0 and its Clippy. The counts hold for the working tree of the slice
in progress on 2026-11-22, which pins Rust 1.100.0. The behaviour of the compiler and of Cargo was
measured on Rust 1.97.0. The release notes of Rust 1.98.0 to 1.100.0 change none of it. Clippy's
test detection is read from the source of Clippy 1.100.

## A reason goes unchecked

The Rust rules reserve `unwrap()` and `expect()` for calls that are infallible by construction. The
workspace sets `clippy::unwrap_used` and `clippy::expect_used` to warn, and its `clippy.toml`
exempts test code from both. Each call outside a test states why it cannot fail, in one of two
forms:

- **A lint reason.** `#[allow(clippy::unwrap_used, reason = "...")]` sits on the narrowest item that
  holds an `unwrap`. The compiler shows a lint reason only beside a lint it emits, and `allow` emits
  none ([lint reasons](https://doc.rust-lang.org/reference/attributes/diagnostics.html#lint-reasons)).
- **An `expect` message.** `NonZeroU32::new(5_000).expect("five thousand is not zero")` states the
  reason at the call, and a panic prints it. The workspace keeps this form for calls that run at
  compile time, which neither lint flags.

Nothing checks either form. A change to a constant, a constructor, or a library can make a reason
false. The first sign is then a panic on the input that breaks it. A check, in this doc, is whatever
fails before that panic: a build, a test, or a proof.

## What makes the workspace's reasons true

Lookout's crates hold 181 calls to `unwrap` or `expect` outside integration tests and inline test
modules. Of these, 65 sit in modules gated on `cfg(test)`, 26 in crates that only tests use, 5 in
build scripts, and 38 in binaries. In each of those, a panic is how the code reports a failure, as
the Rust rules permit. The remaining 47, in library code, fall into five kinds by what makes their
reason true:

| What makes the reason true | Example | Calls | What fails when the reason breaks |
|---|---|---|---|
| A constant, evaluated at compile time | `NonZeroU32::new(5_000).expect("five thousand is not zero")` in a `const` item | 10 | The build |
| A constant, evaluated at run time | `GersId::new(id).unwrap()`, under "the tests below parse each constant id above" | 17 | A test that reaches the call with each input the source fixes |
| A check that an earlier constructor made | `TimeDelta::from_std(self.0).unwrap()`, under "Span::new refuses a span that a TimeDelta cannot hold" | 12 | A test, on the inputs it uses |
| A contract of a library or a format | `digest[..8].try_into().unwrap()`, under "a SHA-256 digest holds 32 bytes" | 7 | A test, on the inputs it uses |
| No claim: the panic is intended | `tokio::runtime::Runtime::new().unwrap()`, for the store's shared runtime | 1 | Not applicable |

The 37 calls evaluated at run time sit under 33 allowances, since a function that builds an
example holds several. Two more allowances cover no hand-written call. One covers the module that a
generator writes. The other sits on `GpsReadingRowV2::example`, which builds its device id through
`DeviceId::from` and calls nothing that can fail.

The prompting allowance is of the second kind. The platform core builds the predictor from
`DEFAULT_WATCHING` and `DEFAULT_ALERTING`, and `CrowFlies::new` refuses an alerting radius beyond the
watching radius. The device core's tests build the model, so they check the two constants, and the
build does not.

Three reasons name their check:

- **The device's carried crossings.** The reason reads "the build fails on a carried set that
  holds_points rejects", and `const _: () = assert!(holds_points(&PACKED.0), ..)` makes the build
  fail so. `holds_points` checks the header and the length. `PointSet::new` also checks that every
  point lies on the globe, which the reason leaves out. The device core's tests check the
  coordinates, since they call it.
- **The Overture division ids.** The reason reads "the tests below parse each constant id above",
  and `every_country_names_a_division_that_parses_as_a_gers_id` parses them.
- **The Motis client.** The reason reads "DEFAULT_BASE_URL is a fixed, valid URL, and the tests
  build a client from it". The three tests that build the default client end in `end_to_end`. The
  default and `no-docker` nextest profiles skip such tests, so the check runs only in the
  end-to-end profile.

In each, only the reader links the reason to its check. Renaming or removing the check leaves the
reason pointing at nothing.

## What the compiler checks

The initialiser of a `const` item, and a `const { }` block in an expression, run at compile time
([constant evaluation](https://doc.rust-lang.org/reference/const_eval.html)). A panic there fails the
build with error E0080, which prints the panic's message:

```text
error[E0080]: evaluation panicked: the default alerting radius lies within the default watching radius
```

An `expect` evaluated at compile time therefore turns its reason into a check: the reason becomes
the message the build fails with. Clippy's `unwrap_used` and `expect_used` skip such a call, under
the settings `allow-unwrap-in-consts` and `allow-expect-in-consts`, both `true` by default
([lint configuration](https://doc.rust-lang.org/clippy/lint_configuration.html)). The call needs no
allowance.

The exemption follows the evaluation, not the function. An `expect` in the body of a `const fn`
still fails `expect_used`, since a `const fn` also runs at run time. The `const fn` returns an
`Option`, and the `const` item or block calls `expect` on it. `HalfWidth` has this shape.
`HalfWidth::checked` is a `const fn` returning `Option<Self>`, `HalfWidth::new` maps its `None` to a
`thiserror` error, and `DEFAULT_HALF_WIDTH` calls `checked(45.0).expect(..)` in a `const` item.

What constant evaluation accepts:

| Construct | Accepted | Stable since |
|---|---|---|
| `Option::unwrap` and `Option::expect` | Yes | 1.83 |
| Float arithmetic and comparison in a `const fn` | Yes | 1.82 |
| A `const { }` block in an expression | Yes | 1.79 |
| `assert!` and `panic!` | Yes | 1.57 |
| `Result::unwrap` and `Result::expect` | No: error E0015, since both format the error | Not stable |
| Allocation, as by `String`, `Vec`, or `Url::parse` | No | Not stable |
| A function that is not a `const fn` | No | Not applicable |

A relation between constants, with no value to build, takes `const _: () = assert!(..)`. It does
what the `static_assertions` crate's `const_assert!` does, with no dependency.

Constant evaluation reaches a call whose input is a number, a byte array, or a struct of them. Of
the 17 constants evaluated at run time, three are numbers: the default radii and two percentages.
Two more are byte arrays, the carried and the empty point sets, but `PointSet::new` casts them with
`bytemuck`, which is not `const`. The other 12 build a `String` newtype, a URL, or an encoded
polyline, and each allocates.

A call whose error type has no values cannot fail, and its type shows it. Clippy's `unwrap_used` and
`expect_used` skip such a call ([rust-clippy#11252](https://github.com/rust-lang/rust-clippy/pull/11252)).
Since Rust 1.100, `Infallible` is an alias of the never type `!`
([Rust 1.100.0](https://blog.rust-lang.org/2026/11/12/Rust-1.100.0/)). None of the 37 calls has such
an error type.

## What a test or a proof checks

Outside constant evaluation, a reason gets one of five checks:

| Check | What a pass shows | Over which inputs | What it costs |
|---|---|---|---|
| A unit test | The call returns | The inputs the test names | A test per call |
| A property test, with [proptest](https://docs.rs/proptest/latest/proptest/) | The call returns | A sample from a strategy, 256 cases by default | A strategy per input type |
| A proof harness, with [Kani](https://model-checking.github.io/kani/) | No panic is reachable | Every value of the input types, with loops unrolled to a bound | A harness under `cfg(kani)`, run by `cargo kani` on a compiler of its own |
| [`#[no_panic]`](https://docs.rs/no-panic) | The compiler removed every panic from the function | Every input | A linked build at an optimisation level that removes the panic |
| A deductive verifier: Prusti, Creusot, Verus, or Flux | A specification holds | Every input | A specification on the function and its callees, and the verifier's own toolchain |

A unit test checks a constant evaluated at run time completely. It needs only to reach the call with
each input the source fixes. A call backed by an earlier check or a contract takes inputs from a
wider set. A unit test checks the inputs it names, and a property test the inputs it samples.

Kani is a bounded model checker built on CBMC. A harness builds its inputs with `kani::any()`,
limits them with `kani::assume` to what the constructor accepts, and calls the function. Kani then
proves that no admitted input reaches a panic, or reports an input that does. Function contracts,
`#[kani::requires]` and `#[kani::ensures]`, need the unstable `-Zfunction-contracts` flag
([function contracts](https://model-checking.github.io/kani/rfc/rfcs/0009-function-contracts.html)).
Each Kani release follows a recent nightly Rust, and `cargo kani setup` installs that compiler under
`~/.kani` ([installation](https://model-checking.github.io/kani/install-guide.html)). The workspace
stays on stable Rust, since a harness compiles only under `cfg(kani)`.

If the compiler cannot prove a function free of panics, `#[no_panic]` fails the link. It names the
function, not the call. It needs `panic = "unwind"`, does not apply to a `const fn`, and fails a
debug build for code that needs optimisation to drop its panics.

The deductive verifiers prove a specification written for each function, by translating the program
for an SMT solver or for Why3
([Corten, appendix E](https://arxiv.org/pdf/2609.04372)).

## What Clippy checks of an allowance

Clippy cannot check a reason, but it checks the allowance that carries one:

- **`#[expect]`.** `#[expect(lint, reason = "..")]` suppresses a lint as `allow` does, and raises
  `unfulfilled_lint_expectations` once the lint stops firing
  ([Rust 1.81.0](https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/)). An allowance that outlives its
  call then warns, as the one on `GpsReadingRowV2::example` would.
- **`allow_attributes`.** This restriction lint flags an outer `#[allow]`, and suggests `#[expect]`.
- **`allow_attributes_without_reason`.** This restriction lint flags an `allow` or an `expect` with
  no reason.

The settings `allow-unwrap-in-tests` and `allow-expect-in-tests`, both `true` in the workspace's
`clippy.toml`, exempt what Clippy counts as test code. That is a `#[test]` function, an item under a
`cfg(test)`, and, since Clippy 1.100, a crate whose root file's path starts with `tests`
([rust-clippy#16786](https://github.com/rust-lang/rust-clippy/pull/16786)). Clippy's changelog names
only `unwrap_used` for the last, but both lints share the check.

Cargo passes rustc a workspace member's root relative to the workspace root, such as
`crates/motis/tests/poll_docker.rs`. That path starts with `crates`, so none of lookout's 18
integration tests counts as test code. Each keeps the allowance at its root that the Rust rules
prescribe. A module gated on `cfg(any(test, feature = "fixtures"))` does not count either, since
Clippy matches only a `test` at the top of the gate. The workspace keeps its fixtures in crates that
only tests use, which allow both lints at their root, so no such module holds a call.

A search of 2026-11-22 found no crate or lint that ties a reason to a test or a proof.
[safe_unwrap](https://docs.rs/safe_unwrap) and [suwrap](https://docs.rs/suwrap) restate the reason
at the call, as an `expect` message does, and check nothing.

## Options

- **Evaluate constants at compile time.** A type gains a `const fn checked` that returns
  `Option<Self>` beside its `new`, as `HalfWidth` has. Each constant of the type is then built
  through `checked` in a `const` item or a `const { }` block. Two values with a relation between
  them, such as the watching and alerting radii, become one type with the same pair of
  constructors. The constant's `expect` needs no allowance, and a wrong constant fails the build
  with its reason. This removes three allowances, on the default radii and on the two percentages,
  and leaves the 12 constants that allocate.
- **Keep what the earlier check computed.** A constructor that converts, decodes, or measures its
  input keeps the result. A later method then has nothing to unwrap. A span keeps the `TimeDelta`
  it checked, and a polyline keeps the line it decoded. A session keeps its first sample apart from
  the rest. This removes the call rather than checking it, for most of the 12 calls of that kind.
  It follows the Rust rule that refuses invalid input where it enters
  ([Parse, don't validate](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/)).
- **Name the test in the reason.** Some constants are beyond constant evaluation. Each gets a unit
  test that reaches the call with each input the source fixes. The reason names the test, as the
  Overture division ids do, and the test checks the call completely. Nothing checks that the named
  test exists, that it reaches this call, or that a profile runs it by default.
- **Prove the call with Kani.** A harness gives the function every input its constructor admits,
  and Kani reports any input that reaches the panic. This serves a call that keeping the checked
  form cannot remove, backed by an earlier check or a contract. It adds a tool with a compiler of
  its own, run apart from `cargo nextest`. Its time on the workspace's float and geometry code is
  unmeasured.
- **Make each allowance expire.** `#[expect]` replaces `#[allow]`, and `allow_attributes` and
  `allow_attributes_without_reason` hold the workspace to it. An allowance whose call has gone then
  warns, as the one on `GpsReadingRowV2::example` would today. If Clippy came to count an
  integration test as test code, that test's root allowance would warn too. This checks the
  allowance rather than its reason, and combines with each other option.

Evaluating constants at compile time serves the prompting allowance, and removes it. Keeping the
checked form serves most calls backed by an earlier check. Naming the test suits the constants that
allocate, string newtypes and URLs among them. It needs each named test in the default profile.
Kani waits for a call that keeping the checked form cannot remove and a test samples poorly.
Expiring allowances apply to every allowance that remains, and report the one that covers nothing.

## The default radii, evaluated at compile time

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radii {
    watching: Watching,
    alerting: Alerting,
}

impl Radii {
    pub const fn checked(watching: Watching, alerting: Alerting) -> Option<Self> {
        if alerting.radius.as_metres() > watching.radius.as_metres() {
            None
        } else {
            Some(Self { watching, alerting })
        }
    }

    pub fn new(watching: Watching, alerting: Alerting) -> Result<Self, AlertingBeyondWatching> {
        Self::checked(watching, alerting).ok_or(AlertingBeyondWatching {
            watching: watching.radius,
            alerting: alerting.radius,
        })
    }
}

pub const DEFAULT_RADII: Radii = Radii::checked(DEFAULT_WATCHING, DEFAULT_ALERTING)
    .expect("the default alerting radius lies within the default watching radius");
```

`CrowFlies::new` takes a `Radii` and returns `Self`, since the pair of radii is its only failure.
The evaluation builds its radii from its settings through `Radii::new`, and propagates the error it
propagates today. The platform core passes `DEFAULT_RADII`, so its call needs neither an `unwrap`
nor an allowance. With the default alerting radius set to 9 km, the build stops with E0080 and the
reason as its message. The tests of the refusal move from `CrowFlies` to `Radii`.

## Open questions

- Whether `PointSet::new` can become a `const fn`, so that the carried crossings build in one
  `const` item. The compile-time assertion and the run-time `unwrap` would then be one check, and it
  would cover the coordinates too.
- Whether Kani proves the workspace's float and geometry code, the haversine distance included, in
  a time CI accepts.
- Whether a string newtype gains a `const fn` that checks a `&'static str`. Its constants would
  then check at compile time, and become a `String` at run time without a fallible call.
- Whether Clippy comes to count a workspace member's integration tests as test code. Each
  integration test's root allowance would then suppress nothing.

## Not covered

- Panics other than `unwrap` and `expect`, such as an index out of bounds or `panic!` itself. Kani
  reports those too, and Clippy has restriction lints of its own for them, `indexing_slicing` and
  `panic`.
- The calls in binaries, build scripts, and test code, where a panic is how the code reports a
  failure.

## Approaches considered and rejected

**A lint that requires each reason to name a test.** A [Dylint](https://github.com/trailofbits/dylint)
library can check that a reason names a test that exists. It checks the name, not what the reason
states, and it is a lint to maintain against the compiler's internal interfaces.

**`#[no_panic]` as the check.** It fails the link on any panic the optimiser keeps. It names the
function rather than the call, and its result changes with the optimisation level.

**Deductive verifiers.** A reason here states one comparison or one conversion. A specification of
the function and its callees outweighs it, and each verifier adds a toolchain.

**`unwrap_unchecked`.** It removes the panic by making a false reason undefined behaviour, and it
needs `unsafe`.

**`debug_assert!` beside the call.** It checks the reason on the inputs a debug run meets, which the
`unwrap` already checks.

**The `static_assertions` crate.** `const _: () = assert!(..)` does what its `const_assert!` does,
with no dependency.

## References

Constant evaluation:

- [Constant evaluation](https://doc.rust-lang.org/reference/const_eval.html), in the Rust Reference
  — what runs at compile time, and where
- [Rust 1.79.0](https://blog.rust-lang.org/2024/06/13/Rust-1.79.0/) — `const { }` blocks in
  expressions
- [Rust 1.82.0 release notes](https://releases.rs/docs/1.82.0) — float arithmetic in a `const fn`
- [Rust 1.83.0](https://blog.rust-lang.org/2024/11/28/Rust-1.83.0/) — `Option::expect` and further
  functions in constant evaluation
- [Rust 1.100.0](https://blog.rust-lang.org/2026/11/12/Rust-1.100.0/) — the never type, and
  `Infallible` as its alias
- [static_assertions](https://docs.rs/static_assertions) — compile-time assertions as a crate

Lints:

- [Lint reasons and `#[expect]`](https://doc.rust-lang.org/reference/attributes/diagnostics.html),
  in the Rust Reference, and [RFC 2383](https://rust-lang.github.io/rfcs/2383-lint-reasons.html) —
  what a reason is, and when the compiler shows it
- [Rust 1.81.0](https://blog.rust-lang.org/2024/09/05/Rust-1.81.0/) — `#[expect]` and lint reasons
  on stable Rust
- [Clippy's lint configuration](https://doc.rust-lang.org/clippy/lint_configuration.html) and
  [rust-clippy#14200](https://github.com/rust-lang/rust-clippy/pull/14200) — the exemptions for
  constant evaluation and for tests
- [rust-clippy#16786](https://github.com/rust-lang/rust-clippy/pull/16786) — a crate whose root path
  starts with `tests` counted as test code, from Clippy 1.100
- [rust-clippy#11252](https://github.com/rust-lang/rust-clippy/pull/11252) — no lint on unwrapping a
  type with no values
- [Clippy's lints](https://rust-lang.github.io/rust-clippy/master/index.html) — `unwrap_used`,
  `expect_used`, `allow_attributes`, and `allow_attributes_without_reason`
- [Dylint](https://github.com/trailofbits/dylint) — lints of a project's own, loaded from dynamic
  libraries

Tests and proofs:

- [proptest](https://docs.rs/proptest/latest/proptest/) — property-based tests
- [Kani](https://model-checking.github.io/kani/), its
  [installation guide](https://model-checking.github.io/kani/install-guide.html), and its
  [function contracts](https://model-checking.github.io/kani/rfc/rfcs/0009-function-contracts.html)
  — bounded model checking of Rust
- [Kani: A Model Checker for Rust](https://arxiv.org/pdf/2607.01504) — Kani's design, and contracts
  that take a proof from freedom from panics to functional correctness
- [no-panic](https://docs.rs/no-panic) — a link-time check that a function cannot panic
- [Corten: Foundational Verification of Rust Programs](https://arxiv.org/pdf/2609.04372), appendix
  E — Verus, Prusti, Creusot, and Flux compared
- [safe_unwrap](https://docs.rs/safe_unwrap) and [suwrap](https://docs.rs/suwrap) — reasons
  restated at the call

Design:

- [Parse, don't validate](https://lexi-lambda.github.io/blog/2019/11/05/parse-don-t-validate/)
  (King, 2019) — a check that returns a more precise type, so later code has nothing to check again
- [Using unwrap() in Rust is Okay](https://burntsushi.net/unwrap/) (Gallant) — a panic as the report
  of a broken internal invariant

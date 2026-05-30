# Lazy Mutation Property Tests Design

## Context

Issue #104 asks for a focused hardening pass around `qjson.decode()` lazy table
proxies after reads, traversal, mutation, materialization, and serialization.
The runtime surface is Lua-side code in `lua/qjson/table.lua`; this work should
primarily add tests and CI coverage, not change production behavior unless the
new tests expose a real bug such as stale encoding, crashes, hangs, or an
undocumented ambiguous error.

The existing suite already has deterministic encode property tests in
`tests/lua/encode_property_spec.lua` and many hand-written lazy/ordered encode
cases in `tests/lua/lazy_table_spec.lua` and `tests/lua/ordered_encode_spec.lua`.
The missing piece is an independent ordered oracle that can check mutation
sequences while preserving object key order.

## Supported Boundary

The property suite covers the public lazy table contract:

- object field read, write, and delete through `t.k`
- array read, write, append, and hole creation through `t[i]`
- traversal through `qjson.pairs(t)` and `qjson.ipairs(t)`
- length through `qjson.len(t)`
- serialization through `qjson.encode(t)`
- full conversion through `qjson.materialize(t)`

The suite documents these Lua primitives as out of scope because they bypass
the proxy contract: `rawset`, `rawget`, `next`, and direct metatable mutation.

## Oracle Model

Add a Lua test-only ordered model in `tests/lua/lazy_mutation_property_spec.lua`.
It uses explicit node tags instead of plain Lua tables:

- `object`: ordered `entries = { { key, value }, ... }`
- `array`: ordered `items = { ... }`, with an explicit sparse-array policy for
  holes created by assigning `nil`
- `null`: the qjson/cjson null sentinel
- scalar values: string, boolean, and number

Object mutation follows the library's current ordered-encode contract:

- unmodified duplicate keys can be preserved by the proxy fast path
- once a parent object is mutated or materialized for mutation, duplicate keys
  collapse to first-appearance key order with last-wins values
- deleting and reinserting a key moves that key to the end

The model has helper functions for read, write, delete, traversal, length,
materialization conversion, and JSON encoding. The encoding helper is
independent test code, not `qjson.encode`.

## Equality

Define `semantic_equal(model, value)` in the test file and document it next to
the implementation.

The equality policy is:

- key order is significant for ordered model vs ordered model comparisons and
  for encoded JSON decoded back into the ordered model
- materialized Lua tables are compared structurally because plain Lua objects
  cannot preserve key order; order-sensitive checks use the independent model
  encoder/parser path
- `qjson.null` and `cjson.null` are equivalent null sentinels
- empty arrays are distinct from empty objects through `qjson.empty_array_mt`
- numbers are compared by numeric value after LuaJIT/cjson decoding; explicit
  deterministic regressions cover large integers, `-0`, floats, and scientific
  notation so encoder normalization cannot hide known-risk cases

The checkpoint assertions are partly circular because two of them route through
qjson. The independent ordered model is the real oracle; qjson round trips are
used as extra consistency checks.

## Stateful Generator

Add deterministic randomized cases with environment controls:

- `QJSON_MUT_PROP_CASES`, default small enough for `make test`
- `QJSON_MUT_PROP_SEED`, fixed default printed on failure
- `QJSON_MUT_PROP_STEPS`, fixed small default for CI

The generator biases toward small readable trees and transition-heavy sequences
instead of uniform operation selection. It maintains handles to top-level and
nested nodes so the sequence can deliberately create hybrid trees containing
clean lazy proxies, cached children, mutated nodes, materialized outputs, and
plain replacement values.

Randomized operations include:

- read a field or array element before mutation
- traverse an object or array before mutation
- mutate a top-level field
- mutate a nested field through a cached child proxy
- delete an object key
- add a new object key
- delete then reinsert the same key
- replace a container with a scalar
- replace a scalar with an object or array
- mutate and append array elements
- self-assignment with `t.k = t.k`
- assign one child proxy under a second key to pin alias semantics
- call `qjson.encode()` mid-sequence, then continue mutating
- call `qjson.materialize()` mid-sequence, then continue mutating

High-risk operations that intentionally leave the normal stateful model, such
as mutation-created cycles, sparse array holes, and mutation during active
iteration, are pinned as deterministic regressions instead of randomized
operations so their currently documented semantics stay explicit.

Every checkpoint failure prints the seed, case number, source JSON, operation
trace, checkpoint name, model JSON, qjson encoded output when relevant, and the
materialized value when useful. A simple trace shrinker retries prefixes of the
operation trace and reports the shortest failing prefix when practical; if the
failure is not prefix-reproducible, it reports the full trace.

## Deterministic Regression Cases

Keep existing deterministic lazy and ordered encode cases, and add issue #104
specific tests for:

- unmodified proxies re-emitting original JSON bytes
- read child, mutate or materialize parent, then mutate the old child proxy
- nested child mutation marking ancestors dirty
- deleting or replacing a cached child container
- duplicate scalar and container keys, including mutated earlier duplicate
- escaped object keys and escaped string values
- null sentinel and empty array handling
- user keys named `_keys` and `_values`
- aliasing: assigning the same child proxy under two keys preserves shared Lua
  identity semantics
- cycles: `qjson.encode` returns a bounded max-depth error rather than hanging
  or crashing
- mutation during `qjson.pairs` and `qjson.ipairs`
- number fidelity for large integers, floats, `-0`, and scientific notation
- GC lifetime after cached child access and `collectgarbage()`
- idempotence of repeated `qjson.encode(lazy)`
- child `qjson.materialize` not polluting the source proxy

## CI And Commands

Extend the Makefile with a dedicated target for the lazy mutation property
suite while keeping `make test` deterministic:

- `make test` runs the new suite through the existing `tests/lua` busted glob
  with fixed defaults
- `make lua-mutation-property-test` runs just the new suite and accepts
  `QJSON_MUT_PROP_*` overrides
- the existing `lua-property-test` target stays focused on encode/materialize
  round-trip generation

Add an on-demand and scheduled GitHub Actions stress workflow for the new suite.
It uses random or caller-supplied seeds and larger case/step counts, separate
from the PR-length CI gate.

## Acceptance

The work is complete when:

- `tests/lua/lazy_mutation_property_spec.lua` exists and runs under busted
- the ordered oracle and `semantic_equal` policy are documented in the test file
- failures are reproducible from logged seed/source/trace/checkpoint details
- aliasing, cycles, mutation during iteration, number fidelity, GC lifetime,
  idempotence, and materialize non-pollution are covered
- `make test`, `make lua-lint`, and the focused mutation property target pass
- a PR is opened against `api7/lua-qjson` referencing issue #104

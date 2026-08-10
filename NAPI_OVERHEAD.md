# Cost of a Rust -> JS call

Everything below is measured by `benchmark/napi-call/run.js` against the variant ladder in
`src/bench/napi_call.rs`. Reproduction recipe and caveats are at the end.

The first version of this document estimated these numbers from napi-rs's sources. Two of those
estimates were wrong in ways that mattered, and are corrected here.

## Headline

A bare Rust -> JS crossing costs **~50 ns**, and **~98% of that is the V8 entry itself** - not
argument marshalling, not allocation. But in the real metadata call shapes the crossing is a small
minority of the cost: **converting the arguments dominates by an order of magnitude.**

| what | ns/item | Rust allocs/item |
| --- | --- | --- |
| plain `extern "C"` indirect call (C-ABI floor) | 1.0 | 0 |
| crossing, no arguments | 50.6 | **0** |
| \+ one `f64` | 61.6 | 1 (8 B) |
| \+ one `i32` | 60.6 | 1 (8 B) |
| \+ one short `&str` | 80.4 | 1 (8 B) |
| \+ one `ComplexType` (native type) | 273.1 | 2 (17 B) |
| \+ one `SocketAddrWrapper` | 705.6 | 6 (52 B) |
| \+ one 16-byte `Buffer` | 787.7 | 3 (72 B) |
| real `addColumn(columns, name, typ, kind)` | 333.1 | 2 (41 B) |
| real `addHost(...)` without the `hostId` `Buffer` | 782.1 | 6 (84 B) |
| **real `addHost(hostMap, address, datacenter, rack, hostId)`** | **1735.5** | **8 (148 B)** |
| `napi_new_instance` instead of `napi_call_function` | 59.4 | 0 |
| re-borrowing the callback from its registry per item | 87.4 | 1 (8 B) |

## What was right

**The per-invocation `Vec<napi_value>` is real, and exactly as described.** The allocation counter
shows precisely **1 allocation of 8 bytes** appearing the moment a call has any arguments at all
(`function.rs:232`), and precisely **0** for a no-argument call - confirming that
`JsValuesTupleIntoVec`'s zero-sized special case really does yield a non-allocating `vec![]`. The
counts are internally consistent to the byte: a 4-argument call allocates 32 B for the vector plus
9 B for `ComplexType`, which is the 41 B measured for `addColumn`.

**`Buffer` is the worst single argument, for exactly the predicted reason.** `Buffer::from(&[u8])`
copies into a `Vec` and `to_napi_value` then does `Box::into_raw(Box::new(val))` - and the counter
shows **2 allocations beyond the argument vector**, as predicted from `buffer.rs:492-540`.

**The boundary really is ~1.5-2 orders of magnitude above the C# driver's.** A crossing costs **49x**
a plain `extern "C"` indirect call. That C-ABI number is a *lower bound* for C#, which adds a
GC-mode transition on top, so the true ratio is somewhat below 49x - but not by an order of
magnitude.

**Hoisting the registry borrow out of the per-item loop was worth doing.** Re-borrowing per item
costs **+26.8 ns**, i.e. **53% of an entire extra crossing**, from the `Mutex` plus
`napi_get_reference_value`. `JsFnRegistry`'s "borrow once, call many" contract is load-bearing.

## What was wrong

**1. The `Vec` was called "15-25% of an empty call" and treated as the thing worth fixing. It is
0.6% of a real call.**

The share of a *bare* crossing is right (~22%), but that framing is useless: no real call site is
argument-free. In `addHost`, the argument vector is **11 ns of 1735 ns** and **8 bytes of 148 B** -
one of eight allocations. A `call_raw` over a stack `[napi_value; N]` with a cached `undefined`
would therefore buy **under 1%**. **It is not worth writing**, and the earlier recommendation to
write it first was wrong.

**2. "Per-argument V8 object creation likely dwarfs it" was right, but badly understated, and
pointed at the wrong argument.**

Primitives are nearly free (`i32` and `f64` are indistinguishable, ~10 ns including the vector) and
even a short string is only +19 ns. The expensive arguments are the ones whose `ToNapiValue` builds
a **JS object and sets named properties**:

- `SocketAddrWrapper` - **+644 ns, 5 allocations.** It renders the IP to a Rust `String`, creates an
  object, and sets three named properties. This was never mentioned in the original analysis and is
  nearly as expensive as the `Buffer`.
- `ComplexType` - **+212 ns, 1 allocation**, for the *cheapest* case (a native type). Collection and
  UDT types recurse into subtypes on top of that.
- `Buffer` - **+726 ns.**

Together, `SocketAddrWrapper` and `Buffer` are **~1370 of `addHost`'s 1735 ns: 79% of the cost of
converting one host.** Those two are the optimisation targets; the argument vector is noise beside
them.

**3. `Buffer`'s cost is not a constant - it scales with how many buffers are live in one handle
scope.** Holding total work fixed and varying items per conversion:

| items per conversion | 16 | 128 | 1024 |
| --- | --- | --- | --- |
| `call_buffer` ns/item | 780 | 841 | 938 |

No other argument type behaves this way. Each external buffer registers a `drop_buffer` finalizer
and external-memory accounting with V8, so pressure grows with the number of unfinalized buffers.
Real conversions are cluster-sized, so the lower end of that range applies - but a conversion over
thousands of items would degrade.

## Structural comparison with the C# driver

The C# driver's per-item callback is

```rust
pub(crate) unsafe fn ffi_callback_for_each<Ctx: Copy, T>(
    context: Ctx,
    callback: unsafe extern "C" fn(Ctx, T) -> FFIMaybeException,
    iter: impl Iterator<Item = T>,
) -> FFIMaybeException
```

`callback` points into JIT-compiled managed code: one indirect call plus a GC-mode transition stub.
Arguments travel **in registers by C ABI**, so `FFIStr` is `(ptr, len)` - a string crosses
**borrowed, with no copy and no allocation**. There is no handle scope and no per-item object
construction on the Rust side.

| | C# <-> Rust | Node-API | + napi-rs |
| --- | --- | --- | --- |
| call overhead | indirect call + GC transition | V8 entry (~50 ns) | \+ `Vec` alloc + `napi_get_undefined` (~10 ns) |
| string argument | `(ptr,len)`, borrowed, free | V8 heap alloc + copy (~19 ns) | same |
| bytes argument | `(ptr,len)`, borrowed, free | external ArrayBuffer + GC finalizer (~730 ns) | same |
| struct argument | registers | object + named property sets (~640 ns) | same |
| argument marshalling | registers | stack array | heap `Vec` (8 B/call) |
| error per item | register + branch | status + branch | status + branch |

Two things follow, and the second is the important one:

1. **Prefer primitives over composite arguments.** The gap between C# and Node is small for
   primitives and enormous for anything requiring a JS object. Passing an IP as a pre-formatted
   string, or a UUID as two `f64`s instead of a `Buffer`, would recover most of `addHost`'s cost.
2. **`ffi_callback_for_each` does not port with its cost profile intact.** In C# a per-item callback
   is nearly free, so per-item granularity is right everywhere. Here a crossing is ~50 ns before any
   arguments, so the callback pattern belongs at **metadata** granularity - hosts and columns,
   hundreds of items, tens or hundreds of microseconds, invisible - and would be catastrophic at
   row or cell granularity. That is exactly why `get_rows` hands over one `Buffer` and lets JS parse
   it, and it should stay that way. **In a hot path, the lever is fewer crossings carrying more
   data, not cheaper crossings.**

## Reproducing

```sh
# Timing (quote these numbers):
npm run build
taskset -c 2 node --expose-gc --allow-natives-syntax benchmark/napi-call/run.js --trials=25

# Allocation counts (a separate build; the counting allocator's atomics inflate timings ~5%):
npx napi build --platform --release --features bench-alloc
taskset -c 2 node --expose-gc --allow-natives-syntax benchmark/napi-call/run.js --alloc
npm run build   # restore the ordinary artifact
```

`--allow-natives-syntax` is not optional in practice: it lets the harness verify via
`%GetOptimizationStatus` that V8 actually tiered up each callback. An earlier iteration of this
benchmark silently measured V8's **interpreter** for the multi-argument variants, because one shared
JS callback was receiving several argument types and going polymorphic. Each variant now has its own
monomorphic callback, and the harness prints a warning if any callback is cold.

**Caveats on the numbers above.**

- Machine: AMD Ryzen 7 7745HX, Node v22.22.1 / V8 12.4, `performance` governor, boost **enabled**
  (a source of variance). Absolutes are machine-specific; the **deltas** are the finding.
- Shape: 1000 conversions x 128 items. Each conversion gets its own handle scope and one callback
  borrow, mirroring a real conversion. Do not use `--items` below ~128: at 16 the per-conversion
  overhead is amortised over too few items and the run becomes noise-dominated (MAD of 16 ns on a
  50 ns quantity).
- `perf stat` could not be used as an independent cross-check: `perf_event_paranoid` is 4 on this
  machine and raising it needs root. The allocation counter serves that role instead - it confirms
  the *mechanism* behind the ~10 ns argument-vector delta exactly, which a cycle count could not.
- DHAT and callgrind were deliberately not used. DHAT intercepts every `malloc` in the process (V8
  zones, parser, ICU, libuv), in which one 8-byte `Vec` per call is a rounding error. Callgrind
  needs `--smc-check=all-non-file` for V8's JIT and slows execution 50-100x, so the callbacks would
  never tier up - and `--jitless` would "fix" that by removing the very JS entry being measured.

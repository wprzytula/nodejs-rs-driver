I read the actual sources rather than working from memory. Here's the comparison.

## What each layer actually costs per call

### A. Plain Node-API (`napi_call_function`)

Rust→JS has an irreducible floor, and it isn't allocation:

1. **The V8 entry.** `napi_call_function` runs node's `NAPI_PREAMBLE` (env checks, pending-exception check, GC-state check), reinterprets each `napi_value` to a `v8::Local` (free), then `v8::Function::Call`. That last one builds a JS entry frame, enters the context, and does a stack/interrupt check. This is the dominant cost and it is **per call, not per argument**.
2. **Per-argument V8 object creation.** Each arg must first exist as a `napi_value`, i.e. one `napi_create_*` call, each with its own preamble, each consuming a HandleScope slot. Strings additionally allocate in the V8 heap **and copy** — there is no borrowed-string concept across this boundary at all.
3. **HandleScope growth.** In an N-item loop every created handle stays live until the scope closes, so a long loop wants a per-iteration scope — another pair of napi calls, or unbounded handle-block growth.
4. **Argument array**: stack, free.
5. **Errors**: `napi_pending_exception` in the return status, branch-checked; `napi_get_and_clear_last_exception` only on the error path. Cheap.

### B. napi-rs on top

Two additions per invocation, both in `Function::call` (`napi-3.6.1/src/bindgen_runtime/js_values/function.rs:225-247`):

1. `args.into_vec(self.env)?` — **one `Vec<napi_value>` malloc/free pair per call.** This is what you asked about.
2. **An extra full N-API call per invocation**: `napi_get_undefined` just to produce `this`. Nobody caches it. It's arguably as expensive as the `Vec`.

Everything else is free or unavoidable, and some of it is better than I'd assumed:

- `Function::from_napi_value` is a pure struct build with **no validation** (`function.rs:147`), so the `retagged()` trick in `JsCallback` genuinely costs zero — no `napi_typeof` per call.
- The **inbound** direction is already allocation-free: `#[napi]` generates `CallbackInfo<N>` with `args: [napi_value; N]` on the stack (`callback_info.rs:18-23`). The asymmetry is outbound-only.
- `FnArgs` is fully monomorphised, no dynamic dispatch.
- But some `ToNapiValue` impls are far worse than a `Vec`: `Buffer::from(&[u8])` copies into a `Vec`, then creates an **external ArrayBuffer with a registered finalizer**, which the V8 GC must later run. Per host_id that's much more expensive than the arg vector we're trying to delete.

**One trap I found**: `apply` (`function.rs:249`) uses `check_status!`, not `check_pending_exception!`. So passing the target object as `this` — the natural way to drop one argument conversion — would silently degrade a thrown JS exception into a generic `Error`, breaking the first-error-wins fidelity the callback tests assert. Any `apply`-based optimisation has to hand-roll the call.

### C. C# ↔ Rust in the C# driver

```rust
pub(crate) unsafe fn ffi_callback_for_each<Ctx: Copy, T>(
    context: Ctx,
    callback: unsafe extern "C" fn(Ctx, T) -> FFIMaybeException,
    iter: impl Iterator<Item = T>,
) -> FFIMaybeException
```

That `callback` is a raw pointer to JIT-compiled managed code. Per invocation: an indirect call plus a GC-mode transition stub — a handful of instructions. Arguments go **in registers by C ABI**; `FFIStr` is `(ptr, len)`, so a string crosses **borrowed, with zero copies and zero allocation**. The error channel is a register plus a branch. No handle scope, no GC roots, no per-item object construction on the Rust side.

## The comparison, and what follows from it

|                    | C# ↔ Rust                     | Node-API (plain)                        | + napi-rs                                 |
| ------------------ | ----------------------------- | --------------------------------------- | ----------------------------------------- |
| Call overhead      | indirect call + GC transition | V8 entry frame + preamble               | same + `Vec` alloc + `napi_get_undefined` |
| String arg         | `(ptr,len)`, borrowed, free   | V8 heap alloc + copy                    | same                                      |
| Bytes arg          | `(ptr,len)`, borrowed, free   | external ArrayBuffer + **GC finalizer** | same                                      |
| Arg marshalling    | registers                     | stack array                             | heap `Vec`                                |
| Error per item     | register + branch             | status + branch                         | status + branch                           |
| Order of magnitude | **single-digit ns**           | **tens to low hundreds of ns**          | + maybe 20–40 ns                          |

Numbers are structural estimates, not measurements — I'd want a microbenchmark before quoting any of them.

Two conclusions matter more than the `Vec`:

1. **The `Vec` is real but it is not the main cost.** It's plausibly 15–25% of an empty call and less in practice. The `napi_get_undefined` is comparable, and per-argument V8 object creation likely dwarfs both. Removing the `Vec` is still worth doing — it's cheap, it's a generic win across every call site, and it makes the boundary honest — but it won't move a profile on its own.

2. **`ffi_callback_for_each` does not port with its cost profile intact.** In C# a per-item callback is nearly free, so per-item granularity is the correct default everywhere. In Node the per-crossing floor is 1–2 orders of magnitude higher, which means the callback pattern is right at **metadata** granularity (hosts, columns — hundreds of items, ~100µs, invisible) and would be catastrophic at row/value granularity. That's precisely why `get_rows` hands over one `Buffer` and lets JS parse it, and it should stay that way. If you want Rust↔JS "as cheap as possible" in the hot path, the lever is *fewer crossings with more data each*, not cheaper crossings.

So for a prototype I'd suggest attacking, in order: (a) a `call_raw` over a stack `[napi_value; N]` that also caches `undefined` per-env, killing both per-call overheads and giving correct `check_pending_exception!` semantics even with a `this`; (b) getting `host_id` across without the external-Buffer finalizer. Tell me if you want both or just (a), and whether you want a microbenchmark first so the numbers above stop being estimates.

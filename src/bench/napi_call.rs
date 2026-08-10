//! A ladder of Rust -> JS call variants, each isolating one component of the boundary cost.
//!
//! # Shape of a measurement
//!
//! Every entry point takes `conversions` and `items`, and models what the metadata code actually
//! does: one *conversion* borrows the callback once, opens one N-API handle scope, and streams
//! `items` items across the boundary inside it - see
//! [`crate::metadata::host::cache_host_map`]. The reported cost is per item.
//!
//! Both loops running inside Rust is what makes the numbers meaningful: the single outer JS -> Rust
//! call amortises to nothing, so what remains is the inner Rust -> JS crossing.
//!
//! The per-conversion handle scope is not a detail. Every `napi_value` created for an argument
//! occupies a slot in the enclosing scope until it closes, so looping a few hundred thousand times
//! in *one* scope would pile up a few hundred thousand live handles and drag V8 GC pressure into
//! the measurement - an artifact that a real conversion, bounded by the number of nodes or columns,
//! never encounters.
//!
//! # Each variant has its own JS callback
//!
//! Deliberately, no two variants share a registered JS function, even where the arity matches. A
//! single JS function receiving an `f64` from one variant and a `Buffer` from another would see
//! polymorphic type feedback, get deoptimised, and quietly turn every number into a measurement of
//! V8's interpreter instead of the boundary.
//!
//! # What the differences isolate
//!
//! | difference | isolates |
//! |---|---|
//! | `call_unit - noop` | the whole cost of a crossing |
//! | `call_unit - extern_c` | V8 entry vs. a plain C-ABI indirect call (the C# driver's floor) |
//! | `call_f64 - call_unit` | one `Vec<napi_value>` allocation + the cheapest possible argument |
//! | `call_i32 - call_f64` | `napi_create_int32` vs. `napi_create_double` |
//! | `call_str - call_f64` | `napi_create_string_utf8`: V8 heap allocation + copy |
//! | `call_buffer - call_f64` | `Buffer`: 2 Rust allocations + external ArrayBuffer + GC finalizer |
//! | `add_host - add_host_no_buffer` | the same, in the real 5-argument `addHost` shape |
//! | `i32_borrow_per_item - call_i32` | the registry `Mutex` + `napi_get_reference_value` hop |
//! | `new_instance_unit - call_unit` | `napi_new_instance` vs. `napi_call_function` |

use std::ffi::c_void;
use std::hint::black_box;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use napi::Env;
use napi::bindgen_prelude::{Buffer, FnArgs, Function, Object};
use scylla::cluster::metadata::NativeType;
use scylla::frame::response::result::ColumnType;

use crate::errors::{ConvertedResult, JsResult, with_custom_error_sync};
use crate::types::type_helpers::SocketAddrWrapper;
use crate::types::type_wrappers::ComplexType;
use crate::utils::js_fn_registry::{JsCallback, JsFnRegistry};

/// Defines one per-environment registry for a benchmark callback, with a `#[napi]` registration
/// function and a borrow helper returning the raw [`JsCallback`].
///
/// Unlike `define_js_callback!` in [`crate::utils::js_ctor`], this deliberately does *not* pin the
/// argument type down: [`JsCallback::call`] is already generic over the argument tuple, and each
/// variant here passes exactly one shape anyway.
macro_rules! bench_callback {
    (
        $(#[$doc:meta])*
        static_name: $static_name:ident,
        register_fn: $register_fn:ident,
        borrow_fn: $borrow_fn:ident,
        what: $what:literal,
    ) => {
        $(#[$doc])*
        static $static_name: JsFnRegistry = JsFnRegistry::new($what);

        $(#[$doc])*
        ///
        /// Must be registered with a JS function used by no other variant, so that its type
        /// feedback stays monomorphic.
        #[napi]
        pub fn $register_fn(
            #[napi(ts_arg_type = "(...args: any[]) => void")] callback: Function<(), ()>,
            env: Env,
        ) -> napi::Result<()> {
            $static_name.register(callback, env)
        }

        fn $borrow_fn<'env>(env: &'env Env) -> napi::Result<JsCallback<'env>> {
            Ok(JsCallback::new($static_name.borrow(env)?))
        }
    };
}

bench_callback!(
    /// Callback taking no arguments at all.
    static_name: BENCH_CB_UNIT,
    register_fn: bench_register_cb_unit,
    borrow_fn: borrow_cb_unit,
    what: "benchmark no-argument callback",
);

bench_callback!(
    /// Callback taking a single `f64`.
    static_name: BENCH_CB_F64,
    register_fn: bench_register_cb_f64,
    borrow_fn: borrow_cb_f64,
    what: "benchmark f64 callback",
);

bench_callback!(
    /// Callback taking a single `i32`.
    static_name: BENCH_CB_I32,
    register_fn: bench_register_cb_i32,
    borrow_fn: borrow_cb_i32,
    what: "benchmark i32 callback",
);

bench_callback!(
    /// Callback taking a single string.
    static_name: BENCH_CB_STR,
    register_fn: bench_register_cb_str,
    borrow_fn: borrow_cb_str,
    what: "benchmark string callback",
);

bench_callback!(
    /// Callback taking a single `Buffer`.
    static_name: BENCH_CB_BUFFER,
    register_fn: bench_register_cb_buffer,
    borrow_fn: borrow_cb_buffer,
    what: "benchmark Buffer callback",
);

bench_callback!(
    /// Callback taking a single `SocketAddrWrapper`.
    static_name: BENCH_CB_SOCKET_ADDR,
    register_fn: bench_register_cb_socket_addr,
    borrow_fn: borrow_cb_socket_addr,
    what: "benchmark SocketAddrWrapper callback",
);

bench_callback!(
    /// Callback taking a single `ComplexType`.
    static_name: BENCH_CB_COMPLEX_TYPE,
    register_fn: bench_register_cb_complex_type,
    borrow_fn: borrow_cb_complex_type,
    what: "benchmark ComplexType callback",
);

bench_callback!(
    /// Callback with the real `addColumn(columns, name, typ, kind)` signature.
    static_name: BENCH_CB_ADD_COLUMN,
    register_fn: bench_register_cb_add_column,
    borrow_fn: borrow_cb_add_column,
    what: "benchmark addColumn callback",
);

bench_callback!(
    /// Callback with the real `addHost(hostMap, address, datacenter, rack, hostId)` signature.
    static_name: BENCH_CB_ADD_HOST,
    register_fn: bench_register_cb_add_host,
    borrow_fn: borrow_cb_add_host,
    what: "benchmark addHost callback",
);

bench_callback!(
    /// Callback with the `addHost` signature, but receiving a number in place of the `hostId`
    /// `Buffer`. Separate from [`BENCH_CB_ADD_HOST`] purely to keep both monomorphic.
    static_name: BENCH_CB_ADD_HOST_NO_BUFFER,
    register_fn: bench_register_cb_add_host_no_buffer,
    borrow_fn: borrow_cb_add_host_no_buffer,
    what: "benchmark addHost-without-Buffer callback",
);

/// A JS class constructed with no arguments, used to price `napi_new_instance`.
static BENCH_CTOR: JsFnRegistry = JsFnRegistry::new("benchmark no-argument constructor");

/// Registers the no-argument JS class whose construction [`bench_new_instance_unit`] measures.
#[napi]
pub fn bench_register_ctor(
    #[napi(ts_arg_type = "new () => any")] ctor: new_instance_ctor::Ctor,
    env: Env,
) -> napi::Result<()> {
    BENCH_CTOR.register(ctor, env)
}

/// Keeps the registration signature readable without leaking a bare `Function` type alias into the
/// module's namespace.
mod new_instance_ctor {
    pub type Ctor = napi::bindgen_prelude::Function<'static, (), ()>;
}

/// Runs one conversion per iteration, each in its own handle scope, borrowing the callback once and
/// invoking it `items` times.
///
/// This is a macro rather than a function because the borrowed [`JsCallback`] is tied to the handle
/// scope it was obtained in: a generic `fn` would have to name that per-scope lifetime in a
/// higher-ranked bound its closure argument cannot express.
macro_rules! per_conversion {
    ($env:expr, $conversions:expr, $items:expr, $borrow:expr, |$cb:ident, $i:ident| $body:expr) => {
        for _ in 0..$conversions {
            $env.run_in_scope(|| {
                let $cb = $borrow($env)?;
                for $i in 0..$items {
                    let _ = $i;
                    $body?;
                }
                Ok(())
            })?;
        }
    };
}

// ---------------------------------------------------------------------------------------------
// Variant 1: the floor. A Rust loop that builds each item's arguments and crosses nothing.
// ---------------------------------------------------------------------------------------------

/// Baseline: the loops and the per-item argument construction, with no boundary crossing and no
/// handle scope. Subtracting this from any other variant removes the harness itself.
#[napi(ts_return_type = "void")]
pub fn bench_noop(conversions: u32, items: u32) {
    for _ in 0..conversions {
        for i in 0..items {
            black_box(FnArgs::from((i as i32,)));
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Variant 2: the C-ABI floor, standing in for the C# driver's `extern "C"` callback.
// ---------------------------------------------------------------------------------------------

/// The callee for [`bench_extern_c`]: an `extern "C"` function shaped like the C# driver's per-item
/// callback - opaque context pointer, one payload argument, integer status return.
///
/// `#[inline(never)]` is mandatory; inlined, this would measure nothing.
#[inline(never)]
extern "C" fn bench_extern_callee(_ctx: *const c_void, value: i32) -> u32 {
    // Just enough work to keep the call from being provably useless.
    (value as u32) & 1
}

/// The callee behind a `static`, so reading it through [`black_box`] yields a pointer LLVM cannot
/// resolve back to [`bench_extern_callee`].
static BENCH_EXTERN_FN: extern "C" fn(*const c_void, i32) -> u32 = bench_extern_callee;

/// Calls a plain `extern "C"` function pointer once per item.
///
/// This is a *lower bound* for the C# comparison, not the C# number: the C# driver's callback is a
/// pointer into JIT-compiled managed code, so it pays this plus a GC-mode transition stub. Any real
/// C# measurement must land at or above this.
#[napi(ts_return_type = "void")]
pub fn bench_extern_c(conversions: u32, items: u32) {
    let ctx: *const c_void = black_box(std::ptr::null());
    let mut acc: u32 = 0;
    for _ in 0..conversions {
        // Read once per conversion, mirroring how the C# driver receives the callback pointer once,
        // and made opaque so the indirect call survives optimisation.
        let callee = black_box(BENCH_EXTERN_FN);
        for i in 0..items {
            acc = acc.wrapping_add(callee(ctx, i as i32));
        }
    }
    black_box(acc);
}

// ---------------------------------------------------------------------------------------------
// Variants 3-7: the crossing itself, and one argument type at a time.
// ---------------------------------------------------------------------------------------------

/// Crossing the boundary with no arguments: the pure per-call cost, being the V8 entry plus the
/// `napi_get_undefined` napi-rs issues to produce `this`.
///
/// Notably this does *not* allocate: `JsValuesTupleIntoVec`'s blanket impl special-cases
/// zero-sized types to a non-allocating `vec![]`.
#[napi(ts_return_type = "void")]
pub fn bench_call_unit(conversions: u32, items: u32, env: &Env) -> JsResult<()> {
    with_custom_error_sync(|| {
        per_conversion!(env, conversions, items, borrow_cb_unit, |cb, i| cb.call(()));
        ConvertedResult::Ok(())
    })
}

/// One `f64` argument: the cheapest possible N-API value, since a double needs no heap object.
///
/// Against [`bench_call_unit`] this prices "having any arguments at all", which is where the
/// per-call `Vec<napi_value>` allocation first appears.
#[napi(ts_return_type = "void")]
pub fn bench_call_f64(conversions: u32, items: u32, env: &Env) -> JsResult<()> {
    with_custom_error_sync(|| {
        per_conversion!(env, conversions, items, borrow_cb_f64, |cb, i| cb
            .call(FnArgs::from((i as f64,))));
        ConvertedResult::Ok(())
    })
}

/// One `i32` argument.
#[napi(ts_return_type = "void")]
pub fn bench_call_i32(conversions: u32, items: u32, env: &Env) -> JsResult<()> {
    with_custom_error_sync(|| {
        per_conversion!(env, conversions, items, borrow_cb_i32, |cb, i| cb
            .call(FnArgs::from((i as i32,))));
        ConvertedResult::Ok(())
    })
}

/// One short `&str` argument: prices `napi_create_string_utf8`, i.e. a V8 heap allocation plus a
/// copy. The literal is short on purpose - that is what the metadata paths actually pass.
#[napi(ts_return_type = "void")]
pub fn bench_call_str(conversions: u32, items: u32, env: &Env) -> JsResult<()> {
    with_custom_error_sync(|| {
        per_conversion!(env, conversions, items, borrow_cb_str, |cb, i| cb
            .call(FnArgs::from(("some_column_name",))));
        ConvertedResult::Ok(())
    })
}

/// One 16-byte `Buffer`, built per item exactly as `host_id` is in `cache_host_map`.
///
/// napi-rs's sources predict this is the most expensive single argument by a wide margin:
/// `Buffer::from(&[u8])` copies into a `Vec`, `to_napi_value` then does
/// `Box::into_raw(Box::new(val))` - a second Rust allocation - and `napi_create_external_buffer`
/// registers a `drop_buffer` finalizer the V8 GC must run later.
#[napi(ts_return_type = "void")]
pub fn bench_call_buffer(conversions: u32, items: u32, env: &Env) -> JsResult<()> {
    with_custom_error_sync(|| {
        let host_id = [0xABu8; 16];
        per_conversion!(env, conversions, items, borrow_cb_buffer, |cb, i| cb
            .call(FnArgs::from((Buffer::from(host_id.as_slice()),))));
        ConvertedResult::Ok(())
    })
}

/// One `SocketAddrWrapper` argument, the second argument of the real `addHost` call.
///
/// Its `ToNapiValue` renders the IP to a Rust `String`, creates a JS object, and sets three named
/// properties on it - so this prices a small "plain options object" argument, the pattern used
/// wherever Rust hands JS a struct without a dedicated class.
#[napi(ts_return_type = "void")]
pub fn bench_call_socket_addr(conversions: u32, items: u32, env: &Env) -> JsResult<()> {
    with_custom_error_sync(|| {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9042);
        per_conversion!(env, conversions, items, borrow_cb_socket_addr, |cb, i| cb
            .call(FnArgs::from((SocketAddrWrapper::from(addr),))));
        ConvertedResult::Ok(())
    })
}

/// One `ComplexType` argument, the third argument of the real `addColumn` call.
///
/// A native (non-parametrised) type is used, which is the cheapest case: its `ToNapiValue` still
/// creates a JS object and sets properties, and collection or UDT types recurse into subtypes on top
/// of that.
#[napi(ts_return_type = "void")]
pub fn bench_call_complex_type(conversions: u32, items: u32, env: &Env) -> JsResult<()> {
    with_custom_error_sync(|| {
        let typ = ColumnType::Native(NativeType::Text);
        per_conversion!(env, conversions, items, borrow_cb_complex_type, |cb, i| cb
            .call(FnArgs::from((ComplexType::new_borrowed(&typ),))));
        ConvertedResult::Ok(())
    })
}

// ---------------------------------------------------------------------------------------------
// Variants 10-12: the real metadata call shapes.
// ---------------------------------------------------------------------------------------------

/// The real `addColumn(columns, name, typ, kind)` shape from [`crate::metadata::state`], including
/// the `ComplexType` argument, whose `ToNapiValue` builds a JS object and sets properties on it.
#[napi(ts_return_type = "void")]
pub fn bench_call_add_column(conversions: u32, items: u32, env: &Env) -> JsResult<()> {
    with_custom_error_sync(|| {
        let typ = ColumnType::Native(NativeType::Text);
        for _ in 0..conversions {
            env.run_in_scope(|| {
                let cb = borrow_cb_add_column(env)?;
                // Created once per conversion, like the `columns` object the real code fills in.
                let target = Object::new(env)?;
                for _ in 0..items {
                    cb.call(FnArgs::from((
                        target,
                        "some_column_name",
                        ComplexType::new_borrowed(&typ),
                        0u32,
                    )))?;
                }
                Ok(())
            })?;
        }
        ConvertedResult::Ok(())
    })
}

/// The real `addHost(hostMap, address, datacenter, rack, hostId)` shape from `cache_host_map`,
/// `Buffer` included.
#[napi(ts_return_type = "void")]
pub fn bench_call_add_host(conversions: u32, items: u32, env: &Env) -> JsResult<()> {
    with_custom_error_sync(|| {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9042);
        let host_id = [0xABu8; 16];
        for _ in 0..conversions {
            env.run_in_scope(|| {
                let cb = borrow_cb_add_host(env)?;
                let target = Object::new(env)?;
                for _ in 0..items {
                    cb.call(FnArgs::from((
                        target,
                        SocketAddrWrapper::from(addr),
                        Some("datacenter1"),
                        Some("rack1"),
                        Buffer::from(host_id.as_slice()),
                    )))?;
                }
                Ok(())
            })?;
        }
        ConvertedResult::Ok(())
    })
}

/// [`bench_call_add_host`] with the `hostId` `Buffer` replaced by an `f64`, every other argument and
/// the arity held identical. The difference between the two is the `Buffer` cost in situ.
#[napi(ts_return_type = "void")]
pub fn bench_call_add_host_no_buffer(conversions: u32, items: u32, env: &Env) -> JsResult<()> {
    with_custom_error_sync(|| {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9042);
        for _ in 0..conversions {
            env.run_in_scope(|| {
                let cb = borrow_cb_add_host_no_buffer(env)?;
                let target = Object::new(env)?;
                for _ in 0..items {
                    cb.call(FnArgs::from((
                        target,
                        SocketAddrWrapper::from(addr),
                        Some("datacenter1"),
                        Some("rack1"),
                        0.0f64,
                    )))?;
                }
                Ok(())
            })?;
        }
        ConvertedResult::Ok(())
    })
}

// ---------------------------------------------------------------------------------------------
// Variants 11-12: two documented micro-optimisations, put to the test.
// ---------------------------------------------------------------------------------------------

/// [`bench_call_i32`] but re-borrowing the callback from its registry on every item.
///
/// `JsFnRegistry`'s documentation claims hoisting the borrow out of the per-item path matters,
/// because it removes a `Mutex` acquisition and a `napi_get_reference_value` per item. This variant
/// is what makes that claim falsifiable.
#[napi(ts_return_type = "void")]
pub fn bench_call_i32_borrow_per_item(conversions: u32, items: u32, env: &Env) -> JsResult<()> {
    with_custom_error_sync(|| {
        for _ in 0..conversions {
            env.run_in_scope(|| {
                for i in 0..items {
                    borrow_cb_i32(env)?.call(FnArgs::from((i as i32,)))?;
                }
                Ok(())
            })?;
        }
        ConvertedResult::Ok(())
    })
}

/// Constructing a no-argument JS class per item, to price `napi_new_instance` against
/// [`bench_call_unit`]'s `napi_call_function`.
///
/// This is the difference between the `define_js_ctor!` and `define_js_callback!` strategies for a
/// single item, with everything else held equal.
#[napi(ts_return_type = "void")]
pub fn bench_new_instance_unit(conversions: u32, items: u32, env: &Env) -> JsResult<()> {
    with_custom_error_sync(|| {
        for _ in 0..conversions {
            env.run_in_scope(|| {
                let ctor: Function<'_, (), ()> = BENCH_CTOR.borrow(env)?;
                for _ in 0..items {
                    black_box(ctor.new_instance(())?);
                }
                Ok(())
            })?;
        }
        ConvertedResult::Ok(())
    })
}

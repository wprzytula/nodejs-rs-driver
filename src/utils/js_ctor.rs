use napi::Env;
use napi::bindgen_prelude::{Buffer, FnArgs, FromNapiValue, Function, JsValue, Object, Unknown};
use std::collections::HashMap;

use crate::types::type_helpers::SocketAddrWrapper;
use crate::types::type_wrappers::ComplexType;
use crate::utils::js_fn_registry::{JsCallback, JsFnRegistry};
use crate::utils::js_instance::JsInstance;

/// Zero-sized marker types naming each JS class that Rust constructs directly.
/// They exist only to parametrize `JsInstance` and, in turn, `NapiRef`.
pub mod js_constructible_class {
    /// Test-only marker for `TestJsClass(name, value)`, used by `crate::tests::napi_ref_tests`.
    pub enum TestJsClass {}
    pub enum TableMetadata {}
    pub enum MaterializedView {}
    pub enum UdtField {}
    pub enum Udt {}
    pub enum Strategy {}
    pub enum HostMap {}
    pub enum TracingEvent {}
    pub enum QueryTrace {}
}

/// Arguments passed to the test-only `TestJsClass(name, value)` constructor.
type TestJsClassCtorArgs<'a> = FnArgs<(&'a str, i32)>;

/// Arguments passed to the test-only `testAppend(name, value)` callback, used by
/// `crate::tests::js_callback_tests` to exercise `define_js_callback!`.
pub(crate) type TestAppendArgs<'a> = FnArgs<(&'a str, i32)>;

/// Columns of a table/materialized view: a plain JS object mapping column name to `ColumnMetadata`.
///
/// Rust creates the (empty) object itself with `Object::new`, then fills it in one column at a time
/// through the `addColumn` callback - see [`AddColumnArgs`]. Neither side ever holds an
/// intermediate collection of columns.
type ColumnsArg<'a> = Object<'a>;

/// Arguments passed to the `addColumn(columns, name, typ, kind)` callback, which constructs one
/// `ColumnMetadata` and stores it on the `columns` object under `name`.
///
/// Constructing the `ColumnMetadata` on the JS side means one boundary crossing per column instead
/// of two (one to build the instance, one to hand it over inside a collection).
pub(crate) type AddColumnArgs<'a> = FnArgs<(ColumnsArg<'a>, &'a str, ComplexType<'a>, u32)>;

/// Arguments passed to `TableMetadata(columns, partitionKey, clusteringKey, partitioner)`.
type TableMetadataCtorArgs<'a> = FnArgs<(
    ColumnsArg<'a>,
    &'a Vec<String>,
    &'a Vec<String>,
    Option<&'a str>,
)>;

/// Arguments passed to
/// `MaterializedView(columns, partitionKey, clusteringKey, partitioner, tableName)`.
type MaterializedViewCtorArgs<'a> = FnArgs<(
    ColumnsArg<'a>,
    &'a Vec<String>,
    &'a Vec<String>,
    Option<&'a str>,
    &'a str,
)>;

/// Arguments passed to `UdtField(name, typ)`.
type UdtFieldCtorArgs<'a> = FnArgs<(&'a str, ComplexType<'a>)>;

/// Arguments passed to `Udt(name, keyspace, fields)`.
type UdtCtorArgs<'a> = FnArgs<(
    &'a str,
    &'a str,
    Vec<JsInstance<'a, js_constructible_class::UdtField>>,
)>;

/// Arguments passed to `Strategy(kind, replicationFactor, datacenterRepfactors, name, data)`.
/// Only the field(s) relevant to `kind` are set (`Some`); the rest are `None`.
type StrategyCtorArgs<'a> = FnArgs<(
    u32,
    Option<u32>,
    Option<HashMap<&'a str, u32>>,
    Option<&'a str>,
    Option<HashMap<&'a str, &'a str>>,
)>;

/// Arguments passed to `HostMap()`.
///
/// The map is constructed empty and filled in one node at a time through the `addHost` callback -
/// see [`AddHostArgs`] - so no array of hosts is ever built, on either side of the boundary.
///
/// napi-rs has no zero-length `FnArgs`, but its blanket `JsValuesTupleIntoVec` impl special-cases
/// zero-sized types into an empty argument list, so `()` really does call `new HostMap()`.
type HostMapCtorArgs = ();

/// Arguments passed to the `addHost(hostMap, address, datacenter, rack, hostId)` callback, which
/// constructs one `Host` and inserts it into `hostMap`.
///
/// `address` is the plain `{ address, port, family }` options object that `net.SocketAddress`
/// accepts; the JS side turns it into a `net.SocketAddress`. Passing the options instead of an
/// already-built `SocketAddress` saves one boundary crossing per node.
pub(crate) type AddHostArgs<'a> = FnArgs<(
    JsInstance<'a, js_constructible_class::HostMap>,
    SocketAddrWrapper,
    Option<&'a str>,
    Option<&'a str>,
    Buffer,
)>;

/// Arguments passed to `TracingEvent(id, activity, source, elapsed, thread)`.
///
/// `id` is the raw 16-byte timeuuid, and `source` is the raw 4- or 16-byte IP address.
pub(crate) type TracingEventCtorArgs<'a> = FnArgs<(
    Buffer,
    Option<&'a str>,
    Option<Buffer>,
    Option<i32>,
    Option<&'a str>,
)>;

/// Arguments passed to
/// `QueryTrace(requestType, coordinator, parameters, startedAt, duration, clientAddress, events)`.
///
/// `coordinator`/`clientAddress` are raw IP address bytes (converted to `InetAddress` inside the
/// JS constructor), and `events` is an array of already-built `TracingEvent` instances.
pub(crate) type QueryTraceCtorArgs<'a> = FnArgs<(
    Option<&'a str>,
    Option<Buffer>,
    Option<HashMap<String, String>>,
    Option<i64>,
    Option<i32>,
    Option<Buffer>,
    Vec<JsInstance<'a, js_constructible_class::TracingEvent>>,
)>;

/// Defines a per-environment constructor registry for a single pure-JS class, together with:
/// - a `#[napi]` `register_*_ctor` function that JS calls once per environment, at module load
///   time, to hand Rust a reference to the class's constructor;
/// - a `pub(crate)` `build_*` function that constructs a new instance of that class directly,
///   given the constructor arguments.
///
/// The per-environment bookkeeping lives in [`JsFnRegistry`]; see its documentation for why the
/// stored `FunctionRef`s have their `Args` type erased to `()` and have to be re-tagged here.
///
/// `Function::new_instance` takes the exact same `Args` type the `Function` handle was typed with,
/// so `$build_fn` re-tags the handle to the call's real argument type via
/// `Function::from_napi_value` before constructing.
///
/// The `Return` type parameter of the underlying `Function` is always ignored: it is only used by
/// `Function::call`, but we always construct instances with `Function::new_instance`, so we set it
/// to arbitrary `()`.
macro_rules! define_js_ctor {
    (
        $(#[$doc:meta])*
        static_name: $static_name:ident,
        register_fn: $register_fn:ident,
        build_fn: $build_fn:ident,
        args: $args_ty:ty,
        class_name: $class_name:ident,
    ) => {
        $(#[$doc])*
        static $static_name: JsFnRegistry =
            JsFnRegistry::new(concat!(stringify!($class_name), " constructor"));

        /// Registers the JS class constructor, so that Rust can later construct fully-formed JS instances
        /// directly. It is called exactly once per N-API environment (i.e. once per `worker_threads` worker,
        /// and once on the main thread), by the corresponding module on load, before any cluster metadata
        /// is accessed in that environment.
        #[napi]
        pub fn $register_fn(
            #[napi(ts_arg_type = "new (...args: any[]) => any")] ctor: Function<(), ()>,
            env: Env,
        ) -> napi::Result<()> {
            $static_name.register(ctor, env)
        }

        /// Constructs a JS class instance directly, by calling its constructor, registered via register_fn.
        /// The returned object's lifetime is tied to the `&'env Env` borrow passed in: the `napi_value` is
        /// only guaranteed to remain a valid, live GC root for as long as that handle scope is on the stack.
        /// This prevents the returned object from silently outliving the native call.
        pub(crate) fn $build_fn<'env>(
            env: &'env Env,
            args: $args_ty,
        ) -> napi::Result<JsInstance<'env, js_constructible_class::$class_name>> {
            let ctor: Function<'_, (), ()> = $static_name.borrow(env)?;
            // Re-tag the constructor's phantom `Args` type parameter to this call's real
            // (possibly borrowed) argument type.
            let ctor: Function<'_, $args_ty, ()> =
                unsafe { Function::from_napi_value(env.raw(), ctor.raw())? };
            let instance: Unknown = ctor.new_instance(args)?;
            Ok(JsInstance::from_object(Object::from_raw(env.raw(), instance.raw())))
        }
    };
}

/// Defines a per-environment registry for a single JS *callback* - a plain function that Rust
/// invokes once per item to let JS accumulate a collection itself.
///
/// This is the N-API counterpart of the C# driver's `ffi_callback_for_each`: instead of Rust
/// building a `Vec` of JS handles and handing the finished array to a JS constructor, Rust streams
/// the items across the boundary one at a time and JS decides what to build out of them. Nothing
/// is collected on either side; peak extra memory is a single item.
///
/// It generates:
/// - a `#[napi]` `register_*` function that JS calls once per environment, at module load time;
/// - a handle type `$handle`, obtained once via `$borrow_fn` and then invoked per item, which
///   restores the callback's real argument type on top of the registry's erased handle;
/// - `$handle::call` for a single item and `$handle::for_each` for a whole iterator.
///
/// A JS exception thrown by the callback aborts the iteration and is returned as `Err` - the same
/// first-error-wins behaviour as `FFIMaybeException` in the C# driver.
macro_rules! define_js_callback {
    (
        $(#[$doc:meta])*
        static_name: $static_name:ident,
        register_fn: $register_fn:ident,
        borrow_fn: $borrow_fn:ident,
        handle: $handle:ident,
        args: $args_ty:ident,
    ) => {
        $(#[$doc])*
        static $static_name: JsFnRegistry = JsFnRegistry::new(stringify!($handle));

        /// Registers the JS callback. Called exactly once per N-API environment, by the
        /// corresponding module on load, before any cluster metadata is accessed.
        #[napi]
        pub fn $register_fn(
            #[napi(ts_arg_type = "(...args: any[]) => void")] callback: Function<(), ()>,
            env: Env,
        ) -> napi::Result<()> {
            $static_name.register(callback, env)
        }

        $(#[$doc])*
        ///
        /// Obtained once per conversion via the corresponding `borrow_*` function, then invoked
        /// once per item.
        pub(crate) struct $handle<'env>(JsCallback<'env>);

        impl $handle<'_> {
            /// Invokes the callback for a single item.
            pub(crate) fn call<'a>(&self, args: $args_ty<'a>) -> napi::Result<()> {
                self.0.call(args)
            }

            /// Feeds every item of `iter` to the callback, one at a time, so that no intermediate
            /// collection is built on either side of the boundary. Stops at the first JS
            /// exception and returns it.
            pub(crate) fn for_each<'a>(
                &self,
                iter: impl IntoIterator<Item = $args_ty<'a>>,
            ) -> napi::Result<()> {
                iter.into_iter().try_for_each(|args| self.call(args))
            }
        }

        /// Borrows the registered callback into the current handle scope, so that the registry
        /// lookup is paid once per conversion rather than once per item.
        pub(crate) fn $borrow_fn<'env>(env: &'env Env) -> napi::Result<$handle<'env>> {
            Ok($handle(JsCallback::new($static_name.borrow(env)?)))
        }
    };
}

define_js_ctor!(
    /// `TestJsClass(name, value)` - test-only class used by `crate::tests::napi_ref_tests`.
    static_name: TEST_JS_CLASS_CTOR,
    register_fn: register_test_js_class_ctor,
    build_fn: build_test_js_class,
    args: TestJsClassCtorArgs<'_>,
    class_name: TestJsClass,
);

define_js_callback!(
    /// `testAppend(name, value)` - test-only callback used by `crate::tests::js_callback_tests`.
    static_name: TEST_APPEND_CALLBACK,
    register_fn: register_test_append_callback,
    borrow_fn: borrow_test_append_callback,
    handle: TestAppendCallback,
    args: TestAppendArgs,
);

define_js_callback!(
    /// `addColumn(columns, name, typ, kind)` - constructs one `ColumnMetadata` and stores it on
    /// the `columns` object under `name`.
    static_name: ADD_COLUMN_CALLBACK,
    register_fn: register_add_column_callback,
    borrow_fn: borrow_add_column_callback,
    handle: AddColumnCallback,
    args: AddColumnArgs,
);

define_js_ctor!(
    /// `TableMetadata(columns, partitionKey, clusteringKey, partitioner)`
    /// `columns` is an array of `[name, ColumnMetadata]`
    static_name: TABLE_METADATA_CTOR,
    register_fn: register_table_metadata_ctor,
    build_fn: build_table_metadata,
    args: TableMetadataCtorArgs<'_>,
    class_name: TableMetadata,
);

define_js_ctor!(
    /// `MaterializedView(columns, partitionKey, clusteringKey, partitioner, tableName)`
    /// `columns` is an array of `[name, ColumnMetadata]`
    static_name: MATERIALIZED_VIEW_CTOR,
    register_fn: register_materialized_view_ctor,
    build_fn: build_materialized_view,
    args: MaterializedViewCtorArgs<'_>,
    class_name: MaterializedView,
);

define_js_ctor!(
    /// `UdtField(name, typ)`
    static_name: UDT_FIELD_CTOR,
    register_fn: register_udt_field_ctor,
    build_fn: build_udt_field,
    args: UdtFieldCtorArgs<'_>,
    class_name: UdtField,
);

define_js_ctor!(
    /// `Udt(name, keyspace, fields)`
    /// `fields` is an array of `UdtField` instances
    static_name: UDT_CTOR,
    register_fn: register_udt_ctor,
    build_fn: build_udt,
    args: UdtCtorArgs<'_>,
    class_name: Udt,
);

define_js_ctor!(
    /// `Strategy(kind, replicationFactor, datacenterRepfactors, name, data)`
    static_name: STRATEGY_CTOR,
    register_fn: register_strategy_ctor,
    build_fn: build_strategy,
    args: StrategyCtorArgs<'_>,
    class_name: Strategy,
);

define_js_ctor!(
    /// `HostMap()` - constructed empty, then filled through the `addHost` callback below.
    static_name: HOST_MAP_CTOR,
    register_fn: register_host_map_ctor,
    build_fn: build_host_map,
    args: HostMapCtorArgs,
    class_name: HostMap,
);

define_js_callback!(
    /// `addHost(hostMap, address, datacenter, rack, hostId)` - constructs one `Host` (and its
    /// `net.SocketAddress`) and inserts it into `hostMap`.
    static_name: ADD_HOST_CALLBACK,
    register_fn: register_add_host_callback,
    borrow_fn: borrow_add_host_callback,
    handle: AddHostCallback,
    args: AddHostArgs,
);

define_js_ctor!(
    /// `TracingEvent(id, activity, source, elapsed, thread)`
    static_name: TRACING_EVENT_CTOR,
    register_fn: register_tracing_event_ctor,
    build_fn: build_tracing_event,
    args: TracingEventCtorArgs<'_>,
    class_name: TracingEvent,
);

define_js_ctor!(
    /// `QueryTrace(requestType, coordinator, parameters, startedAt, duration, clientAddress, events)`
    /// `events` is an array of `TracingEvent` instances
    static_name: QUERY_TRACE_CTOR,
    register_fn: register_query_trace_ctor,
    build_fn: build_query_trace,
    args: QueryTraceCtorArgs<'_>,
    class_name: QueryTrace,
);

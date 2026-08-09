use std::net::SocketAddr;

use napi::Env;
use napi::bindgen_prelude::{Buffer, FnArgs};
use scylla::cluster::ClusterState;

use crate::errors::{ConvertedError, JsResult, with_custom_error_sync};
use crate::metadata::state::ClusterSnapshot;
use crate::session::SessionWrapper;
use crate::types::type_helpers::SocketAddrWrapper;
use crate::utils::js_ctor::{borrow_add_host_callback, build_host_map, js_constructible_class};
use crate::utils::js_instance::JsInstance;
use crate::utils::napi_ref::NapiRef;

/// Builds a JS `HostMap` holding a `Host` for every node known via `cluster_state`, pinning the
/// map with a `NapiRef`.
///
/// The map is created empty and then filled one node at a time through the `addHost` callback, so
/// nothing in between is collected: no `Vec` of `Host` handles on the Rust side, and no array of
/// hosts on the JS side. Each node crosses the boundary exactly once, carrying its address as the
/// plain options object `net.SocketAddress` accepts - the JS callback builds the `SocketAddress`
/// and the `Host` itself. This is the same shape as the C# driver's `ffi_callback_for_each`-based
/// metadata conversion.
///
/// Pinning the `HostMap` alone (rather than each `Host` individually) is enough to keep every
/// `Host` alive, since the map strongly references all of them. It also means
/// `SessionWrapper::get_all_hosts` hands back one already-assembled object instead of rebuilding
/// a map on the JS side per call, for as long as the cluster state doesn't change.
pub(crate) fn cache_host_map(
    cluster_state: &ClusterState,
    env: &Env,
) -> napi::Result<NapiRef<js_constructible_class::HostMap>> {
    let host_map = build_host_map(env, ())?;

    // Borrowed once, invoked once per node.
    let add_host = borrow_add_host_callback(env)?;
    add_host.for_each(cluster_state.get_nodes_info().iter().map(|node| {
        FnArgs::from((
            host_map,
            SocketAddrWrapper::from(SocketAddr::new(node.address.ip(), node.address.port())),
            node.datacenter.as_deref(),
            node.rack.as_deref(),
            Buffer::from(node.host_id.as_bytes().as_slice()),
        ))
    }))?;

    NapiRef::new(env, host_map)
}

#[napi]
impl SessionWrapper {
    /// Returns all nodes known by the Rust driver as a `HostMap`, keyed by address, for the
    /// current cluster state (refreshing the cached cluster state snapshot first, if the Rust
    /// driver has produced a newer one since the last access). The same JS `HostMap` object is
    /// returned across calls, for as long as the underlying cluster state doesn't change.
    #[napi(ts_return_type = "import('../lib/host').HostMap")]
    pub fn get_all_hosts<'env>(
        &self,
        env: &'env Env,
    ) -> JsResult<JsInstance<'env, js_constructible_class::HostMap>> {
        with_custom_error_sync(|| {
            self.with_cluster_snapshot(env, |cluster_snapshot: &ClusterSnapshot| {
                cluster_snapshot
                    .host_map
                    .get(env)
                    .map_err(ConvertedError::from)
            })
        })
    }
}

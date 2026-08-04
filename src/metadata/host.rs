use std::net::SocketAddr;
use std::sync::Arc;

use napi::Env;
use napi::bindgen_prelude::{Buffer, FnArgs};
use scylla::cluster::{ClusterState, Node};

use crate::errors::{ConvertedError, JsResult, with_custom_error_sync};
use crate::metadata::state::ClusterSnapshot;
use crate::session::SessionWrapper;
use crate::types::type_helpers::SocketAddrWrapper;
use crate::utils::js_ctor::{
    HostCtorArgs, build_host, build_host_map, build_socket_address, js_constructible_class,
};
use crate::utils::js_instance::JsInstance;
use crate::utils::napi_ref::NapiRef;

/// Builds a JS `Host` object for every node known via `cluster_state` and collects them into a
/// single JS `HostMap`, pinning only that map with a `NapiRef`.
///
/// Pinning the `HostMap` alone (rather than each `Host` individually) is enough to keep every
/// `Host` alive, since the map strongly references all of them. It also means
/// `SessionWrapper::get_all_hosts` hands back one already-assembled object instead of rebuilding
/// a map on the JS side per call, for as long as the cluster state doesn't change.
pub(crate) fn cache_host_map(
    cluster_state: &ClusterState,
    env: &Env,
) -> napi::Result<NapiRef<js_constructible_class::HostMap>> {
    let hosts = cluster_state
        .get_nodes_info()
        .iter()
        .map(|node| build_host(env, host_ctor_args(node, env)?))
        .collect::<napi::Result<Vec<_>>>()?;

    let host_map = build_host_map(env, FnArgs::from((hosts,)))?;
    NapiRef::new(env, host_map)
}

/// Builds the arguments passed to the JS Host constructor for the given node.
fn host_ctor_args<'a>(node: &'a Arc<Node>, env: &'a Env) -> napi::Result<HostCtorArgs<'a>> {
    let address = SocketAddr::new(node.address.ip(), node.address.port());
    let address = build_socket_address(env, FnArgs::from((SocketAddrWrapper::from(address),)))?;

    Ok(FnArgs::from((
        address,
        node.datacenter.as_deref(),
        node.rack.as_deref(),
        Buffer::from(node.host_id.as_bytes().as_slice()),
    )))
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

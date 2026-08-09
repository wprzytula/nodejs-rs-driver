// @ts-nocheck
"use strict";

const events = require("events");
const _nodeNet = require("node:net");

const { throwNotSupported } = require("./new-utils");
const Uuid = require("./types/uuid");
const _rust = require("../index");

function socketAddressKey(address) {
    return address.family === "ipv6"
        ? `[${address.address}]:${address.port}`
        : `${address.address}:${address.port}`;
}

/**
 * Represents a Cassandra node.
 * @extends EventEmitter
 */
class Host extends events.EventEmitter {
    /**
     * Creates a new Host instance.
     *
     * Instances of this class are constructed directly from the native code when reading cluster metadata.
     * @param {_nodeNet.SocketAddress} address
     * @param {String|null} datacenter
     * @param {String|null} rack
     * @param {Buffer} hostId
     * @internal
     * @ignore
     */
    constructor(address, datacenter, rack, hostId) {
        super();
        /**
         * Gets the ip address and port number of the node.
         *
         * Use {@link Host#toString} to get the conventional `ip:port` string form.
         * @type {_nodeNet.SocketAddress}
         */
        this.address = address;

        /**
         * Gets string containing the Cassandra version.
         * @type {String}
         */
        this.cassandraVersion = null;

        /**
         * Gets data center name of the node.
         * @type {String}
         */
        this.datacenter = datacenter;

        /**
         * Gets rack name of the node.
         * @type {String}
         */
        this.rack = rack;

        /**
         * Gets the tokens assigned to the node.
         * @type {Array<any>}
         */
        this.tokens = null;

        /**
         * Gets the id of the host.
         *
         * This identifier is used by the server for internal communication / gossip.
         * @type {Uuid}
         */
        this.hostId = Uuid.fromRust(hostId);
    }

    /**
     * @deprecated Not supported by the driver. Usage will throw an error.
     */
    get dseVersion() {
        throwNotSupported("Host.dseVersion");
        return null;
    }

    set dseVersion(_) {
        throwNotSupported("Host.dseVersion");
    }

    /**
     * @deprecated Not supported by the driver. Usage will throw an error.
     */
    get workloads() {
        throwNotSupported("Host.workloads");
        return null;
    }

    set workloads(_) {
        throwNotSupported("Host.workloads");
    }

    /**
     * This endpoint is not yet implemented, and its usage will throw an error
     *
     * Determines if the node is UP now (seen as UP by the driver).
     * @returns {boolean}
     */
    isUp() {
        throw new Error(`TODO: Not implemented`);
    }

    /**
     * This endpoint is not yet implemented, and its usage will throw an error
     *
     * Determines if the host can be considered as UP.
     * Deprecated: Use {@link Host#isUp()} instead.
     * @returns {boolean}
     */
    canBeConsideredAsUp() {
        throw new Error(`TODO: Not implemented`);
    }

    /**
     * This endpoint is not yet implemented, and its usage will throw an error
     *
     * Returns an array containing the Cassandra Version as an Array of Numbers having the major version in the first
     * position.
     * @returns {Array.<Number>}
     */
    getCassandraVersion() {
        // We never set the version when creating object from Rust,
        // so we will explicitly throw an error, when someone attempts to get the version
        // to avoid any confusion
        throw new Error(`TODO: Not implemented`);
        // if (!this.cassandraVersion) {
        //     return utils.emptyArray;
        // }
        // return this.cassandraVersion
        //     .split("-")[0]
        //     .split(".")
        //     .map((x) => parseInt(x, 10));
    }

    /**
     * @deprecated Not supported by the driver. Usage will throw an error.
     */
    getDseVersion() {
        throwNotSupported("Host.getDseVersion");
    }

    /**
     * Returns the string representation of the host's address.
     * @internal
     * @ignore
     */
    addressToString() {
        return socketAddressKey(this.address);
    }
}

/**
 * Represents an associative-array of {@link Host hosts} that can be iterated.
 * It creates an internal copy when adding or removing, making it safe to iterate using the values()
 * method within async operations.
 * @extends events.EventEmitter
 */
class HostMap extends events.EventEmitter {
    #items;
    #itemsByIp;
    #values;

    /**
     * Creates a new, empty HostMap instance.
     *
     * Instances of this class are constructed directly from the native code when reading cluster
     * metadata. The map is created empty and then filled in one host at a time through
     * {@link HostMap#_addFromRust}, so that no intermediate array of hosts is built.
     * @internal
     * @ignore
     */
    constructor() {
        super();

        this.#items = new Map();
        this.#itemsByIp = new Map();
        this.#values = null;

        Object.defineProperty(this, "length", {
            get: () => this.values().length,
            enumerable: true,
        });

        /**
         * Emitted when a host is added to the map
         * @event HostMap#add
         */
        /**
         * Emitted when a host is removed from the map
         * @event HostMap#remove
         */
    }

    /**
     * Adds a host to the map, keyed by its id and by its address.
     *
     * Called from the native code, once per node, while a cluster metadata snapshot is being
     * built. Not part of the public API - {@link HostMap#set} is intentionally unsupported.
     * @param {Host} host
     * @internal
     * @ignore
     */
    _addFromRust(host) {
        this.#items.set(host.hostId, host);
        this.#itemsByIp.set(socketAddressKey(host.address), host.hostId);
    }

    /**
     * Executes a provided function once per map element.
     * @param callback
     */
    forEach(callback) {
        const items = this.#items;
        for (const [key, value] of items) {
            callback(value, key);
        }
    }

    /**
     * Gets a {@link Host host} by key or undefined if not found.
     * @param {Uuid | _nodeNet.SocketAddress | String} key
     * @returns {Host}
     */
    get(key) {
        if (!(key instanceof Uuid)) {
            if (key instanceof _nodeNet.SocketAddress) {
                key = socketAddressKey(key);
            }
            key = this.#itemsByIp.get(key);
        }
        return this.#items.get(key);
    }

    /**
     * Returns an array of host addresses.
     * @returns {Array<Uuid>}
     */
    keys() {
        return Array.from(this.#items.keys());
    }

    /**
     * @deprecated Not supported by the driver. Usage will throw an error.
     */
    remove() {
        throwNotSupported("HostMap.remove");
    }

    /**
     * @deprecated Not supported by the driver. Usage will throw an error.
     */
    removeMultiple() {
        throwNotSupported("HostMap.removeMultiple");
    }

    /**
     * @deprecated Not supported by the driver. Usage will throw an error.
     */
    set() {
        throwNotSupported("HostMap.set");
    }

    /**
     * Returns a shallow copy of the values of the map.
     * @returns {Array.<Host>}
     */
    values() {
        if (!this.#values) {
            // Cache the values
            this.#values = Object.freeze(Array.from(this.#items.values()));
        }

        return this.#values;
    }

    /**
     * @deprecated Not supported by the driver. Usage will throw an error.
     */
    clear() {
        throwNotSupported("HostMap.clear");
    }

    inspect() {
        return this.#items;
    }

    toJSON() {
        return Object.fromEntries(
            Array.from(this.#items.values(), (host) => [host.hostId, host]),
        );
    }
}

module.exports = {
    Host,
    HostMap,
};

/**
 * Builds one {@link Host} and inserts it into `hostMap`.
 *
 * Rust calls this once per node while building a cluster metadata snapshot, instead of handing
 * over a finished array of hosts. That way neither side ever holds an intermediate collection,
 * and each node crosses the native boundary exactly once: `address` arrives as the plain options
 * object that `net.SocketAddress` accepts, and both the address and the host are constructed here.
 * @param {HostMap} hostMap
 * @param {{ address: String, port: Number, family: String }} address
 * @param {String|null} datacenter
 * @param {String|null} rack
 * @param {Buffer} hostId
 * @internal
 * @ignore
 */
function addHost(hostMap, address, datacenter, rack, hostId) {
    hostMap._addFromRust(
        new Host(new _nodeNet.SocketAddress(address), datacenter, rack, hostId),
    );
}

// Registers the HostMap constructor and the per-host callback, so that Rust can build the whole
// host map directly when reading cluster metadata.
_rust.registerHostMapCtor(HostMap);
_rust.registerAddHostCallback(addHost);

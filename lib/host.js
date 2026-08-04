// @ts-nocheck
"use strict";

const events = require("events");

const { throwNotSupported } = require("./new-utils");
const Uuid = require("./types/uuid");
const _rust = require("../index");

/**
 * Represents a Cassandra node.
 * @extends EventEmitter
 */
class Host extends events.EventEmitter {
    /**
     * Creates a new Host instance.
     *
     * Instances of this class are constructed directly from the native code when reading cluster metadata.
     * @param {String} address
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
         * @type {String}
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
}

/**
 * Represents an associative-array of {@link Host hosts} that can be iterated.
 * It creates an internal copy when adding or removing, making it safe to iterate using the values()
 * method within async operations.
 * @extends events.EventEmitter
 */
class HostMap extends events.EventEmitter {
    #items;
    #values;

    /**
     * Creates a new HostMap instance.
     *
     * Instances of this class are constructed directly from the native code when reading cluster
     * metadata, which passes the already-built {@link Host} instances to key by their address.
     * @param {Array.<Host>} hosts
     * @internal
     * @ignore
     */
    constructor(hosts) {
        super();

        this.#items = new Map();
        this.#values = null;

        for (const host of hosts) {
            this.#items.set(host.address, host);
        }

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
     * @param {String} key
     * @returns {Host}
     */
    get(key) {
        return this.#items.get(key);
    }

    /**
     * Returns an array of host addresses.
     * @returns {Array.<String>}
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
        return Object.fromEntries(this.#items);
    }
}

module.exports = {
    Host,
    HostMap,
};

// Registers the Host and HostMap constructors, so that Rust can construct fully-formed
// instances directly when reading cluster metadata.
_rust.registerHostCtor(Host);
_rust.registerHostMapCtor(HostMap);

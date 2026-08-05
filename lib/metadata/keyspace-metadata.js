// @ts-nocheck
"use strict";

// Imports for the purpose of type hints in JS docs.
// eslint-disable-next-line no-unused-vars
const { TableMetadata } = require("./table-metadata");
// eslint-disable-next-line no-unused-vars
const { MaterializedView } = require("./materialized-view");
// eslint-disable-next-line no-unused-vars
const { Udt } = require("./user-defined-type");

const _rust = require("../../index");

/**
 * Identifies the replication strategy variant.
 * @readonly
 * @enum {number}
 * @alias module:metadata~StrategyKind
 */
const StrategyKind = {
    /**
     * Deprecated in ScyllaDB.
     *
     * **Use only for a single datacenter and one rack.**
     *
     * Places the first replica on a node determined by the partitioner.
     * Additional replicas are placed on the next nodes clockwise in the ring
     * without considering topology (rack or datacenter location).
     */
    SimpleStrategy: 0,
    /**
     * Use this strategy when you have (or plan to have) your cluster deployed across
     * multiple datacenters. This strategy specifies how many replicas you want in each
     * datacenter.
     *
     * `NetworkTopologyStrategy` places replicas in the same datacenter by walking the ring
     * clockwise until reaching the first node in another rack. It attempts to place replicas
     * on distinct racks because nodes in the same rack (or similar physical grouping) often
     * fail at the same time due to power, cooling, or network issues.
     */
    NetworkTopologyStrategy: 1,
    /**
     * Used for internal purposes, e.g. for system tables.
     */
    LocalStrategy: 2,
    /**
     * Unknown other strategy, which is not supported by the driver.
     */
    Other: 3,
};

/**
 * Describes the replication strategy used by a keyspace.
 * @alias module:metadata~Strategy
 */
class Strategy {
    /**
     * Identifies which strategy variant this is.
     * @type {StrategyKind}
     */
    kind;

    /**
     * Replication factor, i.e. how many replicas of each piece of data there are.
     * (only set when {@link kind} is {@link StrategyKind.SimpleStrategy}).
     * @type {number?}
     */
    replicationFactor;

    /**
     * Replication factors of datacenters with given names, i.e. how many replicas of each piece
     * of data there are in each datacenter.
     * (only set when {@link kind} is {@link StrategyKind.NetworkTopologyStrategy}).
     * @type {Object.<String, number>?}
     */
    datacenterRepfactors;

    /**
     * Name of the strategy (only set when {@link kind} is {@link StrategyKind.Other}).
     * @type {String?}
     */
    name;

    /**
     * Additional parameters of the strategy, which the driver does not understand.
     * (only set when {@link kind} is {@link StrategyKind.Other}).
     * @type {Object.<String, String>?}
     */
    data;

    /**
     * Constructs a Strategy instance.
     *
     * Instances of this class are constructed directly from the native code
     * when reading cluster metadata. Only the field(s) relevant to `kind`
     * are set; the rest are `null`.
     * @param {StrategyKind} kind
     * @param {number|null} replicationFactor
     * @param {Object.<String, number>|null} datacenterRepfactors
     * @param {String|null} name
     * @param {Object.<String, String>|null} data
     * @internal
     * @ignore
     */
    constructor(kind, replicationFactor, datacenterRepfactors, name, data) {
        this.kind = kind;
        this.replicationFactor = replicationFactor ?? null;
        this.datacenterRepfactors = datacenterRepfactors ?? null;
        this.name = name ?? null;
        this.data = data ?? null;
    }
}

/**
 * Describes a keyspace in the cluster.
 *
 * This is a thin wrapper over the native `KeyspaceWrapper`: it holds the native wrapper
 * produced by the native layer and exposes its metadata through the driver's public API.
 * @alias module:metadata~KeyspaceMetadata
 */
class KeyspaceMetadata {
    /**
     * Native keyspace wrapper this instance delegates to.
     * @type {_rust.KeyspaceWrapper}
     * @private
     */
    #wrapper;
    #strategy;
    #tables;
    #views;
    #userDefinedTypes;

    /**
     * Creates a KeyspaceMetadata instance backed by a native keyspace wrapper.
     * @param {_rust.KeyspaceWrapper} wrapper
     * @internal
     * @ignore
     */
    constructor(wrapper) {
        this.#wrapper = wrapper;
    }

    /**
     * Replication strategy used by the keyspace.
     * @type {Strategy}
     */
    get strategy() {
        if (!this.#strategy) {
            this.#strategy = this.#wrapper.strategy;
        }
        return this.#strategy;
    }

    /**
     * Whether the keyspace has durable writes enabled.
     * @type {Boolean}
     */
    get durableWrites() {
        return this.#wrapper.durableWrites;
    }

    /**
     * Tables in the keyspace, keyed by table name.
     * @type {Object.<String, TableMetadata>}
     */
    get tables() {
        if (!this.#tables) {
            this.#tables = this.#wrapper.tables;
        }
        return this.#tables;
    }

    /**
     * Materialized views in the keyspace, keyed by view name.
     * @type {Object.<String, MaterializedView>}
     */
    get views() {
        if (!this.#views) {
            this.#views = this.#wrapper.views;
        }
        return this.#views;
    }

    /**
     * User-defined types in the keyspace, keyed by type name.
     * @type {Object.<String, Udt>}
     */
    get userDefinedTypes() {
        if (!this.#userDefinedTypes) {
            this.#userDefinedTypes = this.#wrapper.userDefinedTypes;
        }
        return this.#userDefinedTypes;
    }

    /**
     * Creates a KeyspaceMetadata instance backed by a native keyspace wrapper.
     * @param {_rust.KeyspaceWrapper} keyspaceWrapper
     * @returns {KeyspaceMetadata}
     * @internal
     * @ignore
     */
    static fromRust(keyspaceWrapper) {
        return new KeyspaceMetadata(keyspaceWrapper);
    }
}

module.exports = { KeyspaceMetadata, Strategy, StrategyKind };

// Registers the Strategy constructor, so that Rust can construct fully-formed instances
// directly when reading cluster metadata. StrategyKind is a plain object of numeric
// constants, not a class, so unlike Strategy it has nothing to register: Rust and JS
// simply agree on the same numeric values
_rust.registerStrategyCtor(Strategy);

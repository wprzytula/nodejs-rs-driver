"use strict";
const { assert } = require("chai");
const { SocketAddress } = require("net");
const { Host, HostMap } = require("../../lib/host");
const Uuid = require("../../lib/types/uuid");

/**
 * Builds a real `Host` instance the same way the native code would (constructor is only
 * meant to be invoked by Rust in production), given an already-built address.
 */
function makeHostAt(address, datacenter, rack, hostIdByte) {
    const hostId = Buffer.alloc(16, hostIdByte);
    return new Host(address, datacenter, rack, hostId);
}

/**
 * Builds a `HostMap` the same way the native code would: created empty, then filled in one host
 * at a time through `_addFromRust`.
 */
function makeHostMap(hosts) {
    const hostMap = new HostMap();
    for (const host of hosts) {
        hostMap._addFromRust(host);
    }
    return hostMap;
}

/**
 * Convenience wrapper building the `net.SocketAddress` for the host too, mirroring what the
 * `addHost` callback does on the JS side when called from Rust.
 *
 * `family` has to be passed explicitly for IPv6, since `net.SocketAddress` defaults to `ipv4` and
 * rejects an IPv6 literal under that family. Rust always sends it explicitly.
 */
function makeHost(ip, port, datacenter, rack, hostIdByte) {
    const address = new SocketAddress({
        address: ip,
        port,
        family: ip.includes(":") ? "ipv6" : "ipv4",
    });
    return makeHostAt(address, datacenter, rack, hostIdByte);
}

describe("Host", function () {
    describe("constructor", function () {
        it("should populate address, datacenter, rack and hostId as given", function () {
            const address = new SocketAddress({
                address: "127.0.0.1",
                port: 9042,
            });
            const host = makeHostAt(address, "dc1", "rack1", 7);

            assert.strictEqual(host.address, address);
            assert.instanceOf(host.address, SocketAddress);
            assert.strictEqual(host.address.address, "127.0.0.1");
            assert.strictEqual(host.address.port, 9042);
            assert.strictEqual(host.datacenter, "dc1");
            assert.strictEqual(host.rack, "rack1");
            assert.instanceOf(host.hostId, Uuid);
            assert.isTrue(host.hostId.getBuffer().equals(Buffer.alloc(16, 7)));
        });

        it("should allow null datacenter and rack", function () {
            const host = makeHost("127.0.0.1", 9042, null, null, 0);

            assert.isNull(host.datacenter);
            assert.isNull(host.rack);
        });
    });

    describe("addressToString()", function () {
        it("should render an IPv4 address as ip:port", function () {
            const host = makeHost("127.0.0.1", 9042, "dc1", "rack1", 1);

            assert.strictEqual(host.addressToString(), "127.0.0.1:9042");
        });

        it("should bracket an IPv6 address", function () {
            const host = makeHost("::1", 9042, "dc1", "rack1", 1);

            assert.strictEqual(host.addressToString(), "[::1]:9042");
        });
    });
});

describe("HostMap", function () {
    describe("constructor", function () {
        it("should build an empty HostMap when no hosts are added", function () {
            const hostMap = makeHostMap([]);

            assert.instanceOf(hostMap, HostMap);
            assert.strictEqual(hostMap.length, 0);
            assert.deepEqual(hostMap.keys(), []);
            assert.deepEqual(hostMap.values(), []);
        });

        it("should build a HostMap keyed by the host ids", function () {
            const host1 = makeHost("127.0.0.1", 9042, "dc1", "rack1", 1);
            const host2 = makeHost("127.0.0.2", 9042, "dc1", "rack2", 2);
            const hostMap = makeHostMap([host1, host2]);

            assert.strictEqual(hostMap.length, 2);
            assert.sameMembers(hostMap.keys(), [host1.hostId, host2.hostId]);
            assert.sameMembers(hostMap.values(), [host1, host2]);
        });
    });

    describe("instance behavior", function () {
        let host1;
        let host2;
        let hostMap;

        beforeEach(function () {
            host1 = makeHost("127.0.0.1", 9042, "dc1", "rack1", 1);
            host2 = makeHost("127.0.0.2", 9042, "dc1", "rack2", 2);
            hostMap = makeHostMap([host1, host2]);
        });

        it("get() should return the host for a known Uuid key", function () {
            assert.strictEqual(hostMap.get(host1.hostId), host1);
            assert.strictEqual(hostMap.get(host2.hostId), host2);
        });

        it("get() should return the host for a known SocketAddress key", function () {
            assert.strictEqual(hostMap.get(host1.address), host1);
            assert.strictEqual(hostMap.get(host2.address), host2);
        });

        it("get() should return the host for a known address string key", function () {
            assert.strictEqual(hostMap.get("127.0.0.1:9042"), host1);
            assert.strictEqual(hostMap.get("127.0.0.2:9042"), host2);
        });

        it("get() should return undefined for an unknown address", function () {
            const absent = new SocketAddress({
                address: "10.0.0.1",
                port: 9042,
            });

            assert.isUndefined(hostMap.get(absent));
        });

        it("forEach() should invoke the callback once per host with (value, key)", function () {
            const seen = [];
            hostMap.forEach((value, key) => seen.push([key, value]));

            assert.lengthOf(seen, 2);
            assert.sameDeepMembers(seen, [
                [host1.hostId, host1],
                [host2.hostId, host2],
            ]);
        });

        it("values() should return a frozen, cached array", function () {
            const values1 = hostMap.values();
            const values2 = hostMap.values();

            assert.isFrozen(values1);
            assert.strictEqual(values1, values2);
            assert.sameMembers(values1, [host1, host2]);
        });

        it("toJSON() should return a plain object keyed by the Uuid string", function () {
            const json = hostMap.toJSON();
            const hostId1 = host1.hostId;
            const hostId2 = host2.hostId;

            assert.deepEqual(json, {
                [hostId1]: host1,
                [hostId2]: host2,
            });
        });

        it("inspect() should return the internal Map", function () {
            const inspected = hostMap.inspect();

            assert.instanceOf(inspected, Map);
            assert.strictEqual(inspected.get(host1.hostId), host1);
        });

        it("length getter should reflect the number of hosts", function () {
            assert.strictEqual(hostMap.length, 2);
        });
    });
});

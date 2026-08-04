"use strict";
const assert = require("chai").assert;
const { SocketAddress } = require("net");

const helper = require("../../test-helper");
const { Host, HostMap } = require("../../../lib/host");
const Uuid = require("../../../lib/types/uuid");

describe("Client#hosts", function () {
    this.timeout(120000);

    describe("with a single node", function () {
        const setupInfo = helper.setup("1:0");

        describe("when the client is connected", function () {
            // Connecting to a single-node cluster should populate `client.hosts` with exactly
            // one entry, and that entry should be a real `Host` instance (not a plain object).
            it("should return a HostMap with real Host instances", function (done) {
                const hosts = setupInfo.client.hosts;
                assert.instanceOf(hosts, HostMap);
                assert.strictEqual(hosts.length, 1);

                const host = hosts.values()[0];
                assert.instanceOf(host, Host);
                done();
            });

            // Verify `host.address` is a well-formed `net.SocketAddress` with a non-empty
            // address string, a numeric port, and a recognized address family.
            it("should populate the host address as a SocketAddress", function (done) {
                const host = setupInfo.client.hosts.values()[0];
                assert.instanceOf(host.address, SocketAddress);
                assert.isString(host.address.address);
                assert.isNumber(host.address.port);
                assert.oneOf(host.address.family, ["ipv4", "ipv6"]);
                done();
            });

            // The single-node test cluster is set up under datacenter "dc1"; verify that name,
            // and that a rack is also present.
            it("should populate the host datacenter and rack", function (done) {
                const host = setupInfo.client.hosts.values()[0];
                assert.strictEqual(host.datacenter, "dc1");
                assert.isString(host.rack);
                done();
            });

            // `host.hostId` should be a `Uuid` wrapping the node's raw 16-byte host ID
            it("should populate the host id as a Uuid", function (done) {
                const host = setupInfo.client.hosts.values()[0];
                assert.instanceOf(host.hostId, Uuid);
                assert.strictEqual(host.hostId.getBuffer().length, 16);
                done();
            });

            // Verify that `HostMap` is internally keyed by hostId
            it("should expose the host keyed by its Uuid in the HostMap", function (done) {
                const hosts = setupInfo.client.hosts;
                const host = hosts.values()[0];
                assert.strictEqual(hosts.get(host.hostId), host);
                assert.deepEqual(hosts.keys(), [host.hostId]);
                done();
            });

            // `forEach(callback)` should invoke `callback(value, key)` once per entry, and those
            // (key, value) pairs should line up with what `values()`/`keys()` report separately.
            it("should iterate hosts via forEach() consistently with values()/keys()", function (done) {
                const hosts = setupInfo.client.hosts;
                const seen = [];
                hosts.forEach((host, id) => seen.push([id, host]));

                assert.lengthOf(seen, hosts.length);
                seen.forEach(([id, host]) => {
                    assert.strictEqual(host.hostId, id);
                });
                done();
            });

            // Verify that `client.hosts` are cached and return the same `Host` instance across repeated accesses.
            it("should return the same underlying Host instance across repeated Client#hosts accesses", function (done) {
                const hostA = setupInfo.client.hosts.values()[0];
                const hostB = setupInfo.client.hosts.values()[0];
                assert.strictEqual(hostA, hostB);
                done();
            });
        });
    });

    describe("with a multi-datacenter cluster", function () {
        const setupInfo = helper.setup("2:1");

        // The cluster is set up with 2 nodes in one datacenter and 1 in another; verify
        // `client.hosts` reports the combined total across both datacenters.
        it("should report the correct total number of hosts", function (done) {
            assert.strictEqual(setupInfo.client.hosts.length, 3);
            done();
        });

        // Verify each node is reported under the datacenter it was actually started in, not just
        // that a `datacenter` field is present.
        it("should assign hosts to their correct datacenter", function (done) {
            const dc1HostA = helper.findHost(setupInfo.client, 1, true);
            const dc1HostB = helper.findHost(setupInfo.client, 2, true);
            const dc2Host = helper.findHost(setupInfo.client, 3, true);

            assert.strictEqual(dc1HostA.datacenter, "dc1");
            assert.strictEqual(dc1HostB.datacenter, "dc1");
            assert.strictEqual(dc2Host.datacenter, "dc2");
            done();
        });

        // Verify that every host is assigned a rack, datacenter and hostId.
        it("should give every host a rack, datacenter and hostId", function (done) {
            const hosts = setupInfo.client.hosts.values();
            hosts.forEach((host) => {
                assert.isString(host.rack);
                assert.isString(host.datacenter);
                assert.instanceOf(host.hostId, Uuid);
            });
            done();
        });

        // Verify no two hosts in the cluster are reported with the same hostId.
        it("should give every host a distinct hostId", function (done) {
            const hosts = setupInfo.client.hosts.values();
            const hostIds = hosts.map((h) => h.hostId.toString());
            assert.strictEqual(new Set(hostIds).size, hosts.length);
            done();
        });
    });
});

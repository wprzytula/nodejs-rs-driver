"use strict";

const { assert } = require("chai");

const rust = require("../../index");

/**
 * Items handed over by the native side through the `testAppend` callback, in order.
 * Reset before each test.
 * @type {Array<{ name: string, value: number }>}
 */
let received = [];

/**
 * If set, the callback throws this error once it has been called that many times. Used to check
 * that a JS exception aborts the native-side iteration instead of being swallowed.
 * @type {number|null}
 */
let throwAfter = null;

describe("define_js_callback!", function () {
    before(function () {
        rust.registerTestAppendCallback(function (name, value) {
            if (throwAfter !== null && received.length === throwAfter) {
                throw new Error("callback failed");
            }
            received.push({ name, value });
        });
    });

    beforeEach(function () {
        received = [];
        throwAfter = null;
    });

    it("should reject re-registration in the same environment", function () {
        assert.throws(
            () => rust.registerTestAppendCallback(() => {}),
            /TestAppendCallback is already registered/,
        );
    });

    it("should hand over every item of the Rust iterator, in order", function () {
        rust.testsStreamToAppendCallback(4);

        assert.deepEqual(received, [
            { name: "item", value: 0 },
            { name: "item", value: 1 },
            { name: "item", value: 2 },
            { name: "item", value: 3 },
        ]);
    });

    it("should hand over nothing for an empty iterator", function () {
        rust.testsStreamToAppendCallback(0);

        assert.deepEqual(received, []);
    });

    it("should stop at the first thrown exception and propagate it", function () {
        throwAfter = 2;

        assert.throws(
            () => rust.testsStreamToAppendCallback(10),
            /callback failed/,
        );

        // Iteration must have stopped at the throwing item: the two items before it got
        // through, and nothing after it was attempted.
        assert.deepEqual(received, [
            { name: "item", value: 0 },
            { name: "item", value: 1 },
        ]);
    });
});

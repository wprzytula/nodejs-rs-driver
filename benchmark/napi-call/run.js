"use strict";

/**
 * Microbenchmark for the Rust -> JS call boundary.
 *
 * Puts measured numbers behind the source-derived claims in `NAPI_OVERHEAD.md`. Each variant lives
 * in `src/bench/napi_call.rs` and runs `conversions` x `items` iterations *inside Rust*, so the one
 * outer JS -> Rust call amortises away and what is measured is the inner Rust -> JS crossing.
 *
 * A "conversion" is one handle scope with one callback borrow, streaming `items` items - the shape
 * of a real metadata conversion. `items` is deliberately cluster-sized rather than huge: looping
 * hundreds of thousands of times inside a single handle scope would accumulate that many live
 * `napi_value` handles and drag V8 GC pressure into the measurement.
 *
 * Every variant gets its own JS callback. Sharing one function across variants would feed it
 * arguments of several types, turn its type feedback polymorphic, and get it deoptimised - at which
 * point the numbers describe V8's interpreter rather than the boundary.
 *
 * Usage:
 *   taskset -c 2 node --expose-gc --allow-natives-syntax benchmark/napi-call/run.js
 *   taskset -c 2 node benchmark/napi-call/run.js --alloc     # needs --features bench-alloc
 *   node benchmark/napi-call/run.js --only=call_unit,call_i32
 *
 * Flags:
 *   --alloc          also report Rust allocations per item (requires the bench-alloc build)
 *   --only=a,b       run only the named variants
 *   --items=N        items per conversion    (default 128)
 *   --conversions=N  conversions per trial   (default 1000)
 *   --trials=N       recorded trials         (default 15)
 *   --warmup=N       discarded warmup trials (default 10)
 */

const os = require("node:os");
const fs = require("node:fs");

const rust = require("../../index");

// ---------------------------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------------------------

const argv = process.argv.slice(2);

/** @param {string} name @param {number} fallback @returns {number} */
function numFlag(name, fallback) {
    const raw = argv.find((a) => a.startsWith(`--${name}=`));
    if (raw === undefined) return fallback;
    const value = Number(raw.slice(name.length + 3));
    if (!Number.isInteger(value) || value <= 0) {
        throw new Error(`--${name} must be a positive integer, got ${raw}`);
    }
    return value;
}

const ITEMS = numFlag("items", 128);
const CONVERSIONS = numFlag("conversions", 1000);
const TRIALS = numFlag("trials", 15);
const WARMUP = numFlag("warmup", 10);
const REPS = ITEMS * CONVERSIONS;
const WANT_ALLOC = argv.includes("--alloc");
const ONLY = (() => {
    const raw = argv.find((a) => a.startsWith("--only="));
    return raw === undefined
        ? null
        : new Set(raw.slice("--only=".length).split(","));
})();

// ---------------------------------------------------------------------------------------------
// Callbacks - one per variant, each monomorphic
//
// Every callback has an observable side effect so neither V8 nor LLVM can treat the work as dead,
// and each touches its arguments so that creating them is not dead either.
// ---------------------------------------------------------------------------------------------

let sink = 0;

function cbUnit() {
    sink++;
}

function cbF64(x) {
    sink += x;
}

function cbI32(x) {
    sink += x;
}

function cbStr(s) {
    sink += s.length;
}

function cbBuffer(b) {
    sink += b.length;
}

function cbSocketAddr(a) {
    sink += a.port;
}

function cbComplexType(t) {
    sink += t.baseType | 0;
}

function cbAddColumn(columns, name, typ, kind) {
    sink += name.length + kind + (typ.baseType | 0);
    // Deliberately not stored on `columns`: this prices the crossing, and an ever-growing object
    // would drag rehashing into the measurement.
}

function cbAddHost(hostMap, address, datacenter, rack, hostId) {
    sink += address.port + datacenter.length + rack.length + hostId.length;
}

function cbAddHostNoBuffer(hostMap, address, datacenter, rack, hostId) {
    sink += address.port + datacenter.length + rack.length + hostId;
}

class BenchClass {
    constructor() {
        sink++;
    }
}

rust.benchRegisterCbUnit(cbUnit);
rust.benchRegisterCbF64(cbF64);
rust.benchRegisterCbI32(cbI32);
rust.benchRegisterCbStr(cbStr);
rust.benchRegisterCbBuffer(cbBuffer);
rust.benchRegisterCbSocketAddr(cbSocketAddr);
rust.benchRegisterCbComplexType(cbComplexType);
rust.benchRegisterCbAddColumn(cbAddColumn);
rust.benchRegisterCbAddHost(cbAddHost);
rust.benchRegisterCbAddHostNoBuffer(cbAddHostNoBuffer);
rust.benchRegisterCtor(BenchClass);

// ---------------------------------------------------------------------------------------------
// Variant table
//
// `callback` is the JS function whose optimisation status must be checked for that variant; null
// means the variant performs no crossing.
// ---------------------------------------------------------------------------------------------

const VARIANTS = [
    {
        name: "noop",
        fn: rust.benchNoop,
        callback: null,
        note: "loops + arg construction only",
    },
    {
        name: "extern_c",
        fn: rust.benchExternC,
        callback: null,
        note: "C-ABI indirect call (C# floor)",
    },
    {
        name: "call_unit",
        fn: rust.benchCallUnit,
        callback: cbUnit,
        note: "crossing, no args",
    },
    {
        name: "call_f64",
        fn: rust.benchCallF64,
        callback: cbF64,
        note: "+ 1 f64 (no heap object)",
    },
    {
        name: "call_i32",
        fn: rust.benchCallI32,
        callback: cbI32,
        note: "+ 1 i32",
    },
    {
        name: "call_str",
        fn: rust.benchCallStr,
        callback: cbStr,
        note: "+ 1 short &str",
    },
    {
        name: "call_buffer",
        fn: rust.benchCallBuffer,
        callback: cbBuffer,
        note: "+ 1 16-byte Buffer",
    },
    {
        name: "call_socket_addr",
        fn: rust.benchCallSocketAddr,
        callback: cbSocketAddr,
        note: "+ 1 SocketAddrWrapper (options obj)",
    },
    {
        name: "call_complex_type",
        fn: rust.benchCallComplexType,
        callback: cbComplexType,
        note: "+ 1 ComplexType (native)",
    },
    {
        name: "add_column",
        fn: rust.benchCallAddColumn,
        callback: cbAddColumn,
        note: "real addColumn shape",
    },
    {
        name: "add_host_no_buffer",
        fn: rust.benchCallAddHostNoBuffer,
        callback: cbAddHostNoBuffer,
        note: "addHost, number hostId",
    },
    {
        name: "add_host",
        fn: rust.benchCallAddHost,
        callback: cbAddHost,
        note: "real addHost shape",
    },
    {
        name: "new_instance_unit",
        fn: rust.benchNewInstanceUnit,
        callback: null,
        note: "napi_new_instance",
    },
    {
        name: "i32_borrow_per_item",
        fn: rust.benchCallI32BorrowPerItem,
        callback: cbI32,
        note: "re-borrows per item",
    },
].filter((v) => ONLY === null || ONLY.has(v.name));

if (VARIANTS.length === 0) throw new Error("--only matched no variants");

// ---------------------------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------------------------

/** @param {number[]} xs @returns {number} */
function median(xs) {
    const s = [...xs].sort((a, b) => a - b);
    const mid = s.length >> 1;
    return s.length % 2 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

/**
 * Median absolute deviation. Reported instead of a standard deviation because one descheduled trial
 * would dominate the latter, and the typical call is what matters here, not the worst one.
 * @param {number[]} xs @returns {number}
 */
function mad(xs) {
    const m = median(xs);
    return median(xs.map((x) => Math.abs(x - m)));
}

// ---------------------------------------------------------------------------------------------
// Optimisation status
// ---------------------------------------------------------------------------------------------

/** @type {((f: Function) => number) | null} */
const getOptimizationStatus = (() => {
    try {
        // eslint-disable-next-line no-eval
        return eval("(f) => %GetOptimizationStatus(f)");
    } catch {
        return null;
    }
})();

/** Bit 4 of V8's OptimizationStatus is kOptimized. */
const K_OPTIMIZED = 1 << 4;

/**
 * @param {Function | null} f
 * @returns {"optimized" | "NOT-OPTIMIZED" | "unknown" | "n/a"}
 */
function optimisationState(f) {
    if (f === null) return "n/a";
    if (getOptimizationStatus === null) return "unknown";
    return (getOptimizationStatus(f) & K_OPTIMIZED) !== 0
        ? "optimized"
        : "NOT-OPTIMIZED";
}

// ---------------------------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------------------------

const gc = global.gc;

/**
 * Runs one variant and returns nanoseconds per item.
 *
 * The optimisation status is sampled immediately after the last recorded trial, not at the end of
 * the whole run: by then V8 may have flushed or deoptimised a callback that was hot at the time.
 * @param {{fn: (conversions: number, items: number) => void, callback: Function | null}} variant
 */
function measure(variant) {
    for (let t = 0; t < WARMUP; t++) variant.fn(CONVERSIONS, ITEMS);

    const samples = [];
    for (let t = 0; t < TRIALS; t++) {
        // Collect before timing, so garbage from the previous variant - notably Buffer finalizers -
        // is not charged to this one.
        if (gc) gc();
        const start = process.hrtime.bigint();
        variant.fn(CONVERSIONS, ITEMS);
        samples.push(Number(process.hrtime.bigint() - start) / REPS);
    }

    return {
        ns: median(samples),
        mad: mad(samples),
        optimised: optimisationState(variant.callback),
    };
}

/**
 * Rust allocations per item, or null if this build has no counting allocator.
 *
 * The counters are process-global, so an empty-region calibration subtracts whatever the readout
 * itself costs. It cannot subtract allocations from tokio worker threads - which is fine here, since
 * this process never opens a session.
 * @param {(conversions: number, items: number) => void} fn
 */
function measureAllocs(fn) {
    if (!rust.benchAllocReset || !rust.benchAllocCounters) return null;

    rust.benchAllocReset();
    const [baseCount, baseBytes] = rust.benchAllocCounters();

    rust.benchAllocReset();
    fn(CONVERSIONS, ITEMS);
    const [count, bytes] = rust.benchAllocCounters();

    return {
        perItem: (count - baseCount) / REPS,
        bytesPerItem: (bytes - baseBytes) / REPS,
    };
}

// ---------------------------------------------------------------------------------------------
// Environment - the absolutes are meaningless without it
// ---------------------------------------------------------------------------------------------

/** @param {string} path @returns {string | null} */
function readSysfs(path) {
    try {
        return fs.readFileSync(path, "utf8").trim();
    } catch {
        return null;
    }
}

function reportEnvironment() {
    const cpus = os.cpus();
    const noTurbo = readSysfs("/sys/devices/system/cpu/intel_pstate/no_turbo");
    const boost = readSysfs("/sys/devices/system/cpu/cpufreq/boost");
    let turbo = "not exposed";
    if (noTurbo !== null)
        turbo = noTurbo === "1" ? "disabled" : "enabled (adds variance)";
    else if (boost !== null)
        turbo = boost === "0" ? "disabled" : "enabled (adds variance)";

    console.log("Environment");
    console.log(
        `  node            ${process.version} (V8 ${process.versions.v8})`,
    );
    console.log(
        `  cpu             ${cpus[0] ? cpus[0].model.trim() : "?"} x${cpus.length}`,
    );
    console.log(`  boost/turbo     ${turbo}`);
    console.log(
        `  governor        ${readSysfs("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor") ?? "unknown"}`,
    );
    console.log(
        `  shape           ${CONVERSIONS} conversions x ${ITEMS} items = ${REPS.toLocaleString("en-US")} items/trial`,
    );
    console.log(`  trials          ${TRIALS} recorded, ${WARMUP} warmup`);
    console.log(
        `  gc between      ${gc ? "yes" : "NO - rerun with --expose-gc"}`,
    );
    console.log(
        `  opt status      ${getOptimizationStatus ? "available" : "unavailable - rerun with --allow-natives-syntax"}`,
    );
    console.log(
        `  alloc counters  ${rust.benchAllocCounters ? "present" : "absent - build with --features bench-alloc"}`,
    );
    if (WANT_ALLOC && !rust.benchAllocCounters) {
        throw new Error(
            "--alloc requested but this build has no counting allocator; rebuild with --features bench-alloc",
        );
    }
    console.log("");
}

// ---------------------------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------------------------

reportEnvironment();

const results = new Map();
for (const v of VARIANTS) {
    if (typeof v.fn !== "function") {
        throw new Error(
            `variant ${v.name} is missing from the native addon - rebuild with npm run build`,
        );
    }
    const timing = measure(v);
    const allocs = WANT_ALLOC ? measureAllocs(v.fn) : null;
    results.set(v.name, { ...v, ...timing, allocs });
}

// Each crossing bumps `sink` at least once, so this is a lower bound that still catches a variant
// silently doing nothing.
const crossingVariants = VARIANTS.filter((v) => v.callback !== null).length;
const expected =
    crossingVariants * REPS * (WARMUP + TRIALS + (WANT_ALLOC ? 1 : 0));
if (crossingVariants > 0 && sink < expected) {
    throw new Error(
        `callbacks ran ${sink} times, expected at least ${expected} - iterations were skipped`,
    );
}

const nameWidth = Math.max(...VARIANTS.map((v) => v.name.length));

console.log("Cost per item (median +/- MAD, ns)");
for (const r of results.values()) {
    const ns = r.ns.toFixed(1).padStart(8);
    const dev = r.mad.toFixed(1).padStart(5);
    const alloc = r.allocs
        ? `  ${r.allocs.perItem.toFixed(2).padStart(5)} alloc  ${r.allocs.bytesPerItem.toFixed(1).padStart(6)} B`
        : "";
    const opt =
        r.optimised === "optimized" || r.optimised === "n/a"
            ? ""
            : `  [callback ${r.optimised}]`;
    console.log(
        `  ${r.name.padEnd(nameWidth)}  ${ns} +/- ${dev}${alloc}   ${r.note}${opt}`,
    );
}
console.log("");

const deoptimised = [...results.values()].filter(
    (r) => r.optimised === "NOT-OPTIMIZED",
);
if (deoptimised.length > 0) {
    console.log(
        `WARNING: ${deoptimised.map((r) => r.name).join(", ")} ran with a cold callback.`,
    );
    console.log(
        "  Those rows measure V8's interpreter, not the boundary. Raise --warmup or --conversions.\n",
    );
}

// ---------------------------------------------------------------------------------------------
// Deltas - the actual findings
// ---------------------------------------------------------------------------------------------

const DELTAS = [
    ["cost of one crossing", "call_unit", "noop"],
    ["V8 entry vs C-ABI call", "call_unit", "extern_c"],
    ["having any args (Vec + f64)", "call_f64", "call_unit"],
    ["i32 vs f64 arg", "call_i32", "call_f64"],
    ["&str vs f64 arg", "call_str", "call_f64"],
    ["SocketAddrWrapper vs f64 arg", "call_socket_addr", "call_f64"],
    ["ComplexType vs f64 arg", "call_complex_type", "call_f64"],
    ["Buffer vs f64 arg", "call_buffer", "call_f64"],
    ["Buffer inside addHost", "add_host", "add_host_no_buffer"],
    ["new_instance vs call", "new_instance_unit", "call_unit"],
    ["re-borrowing per item", "i32_borrow_per_item", "call_i32"],
];

const printable = DELTAS.filter(([, a, b]) => results.has(a) && results.has(b));
if (printable.length > 0) {
    const unit = results.get("call_unit");
    const labelWidth = Math.max(...printable.map(([label]) => label.length));
    console.log("Deltas (ns per item, and as a share of a bare crossing)");
    for (const [label, a, b] of printable) {
        const d = results.get(a).ns - results.get(b).ns;
        const share = unit
            ? `${((d / unit.ns) * 100).toFixed(0).padStart(5)}% of call_unit`
            : "";
        console.log(
            `  ${label.padEnd(labelWidth)}  ${d.toFixed(1).padStart(8)}   ${share}`,
        );
    }
    console.log("");
}

if (results.has("call_unit") && results.has("extern_c")) {
    const ratio =
        results.get("call_unit").ns /
        Math.max(results.get("extern_c").ns, 1e-9);
    console.log(
        `A Node crossing costs ${ratio.toFixed(0)}x a C-ABI indirect call - a lower bound for the C# comparison.`,
    );
}

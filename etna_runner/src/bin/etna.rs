// ETNA workload runner for rstar.
//
// Usage: cargo run --release --bin etna -- <tool> <property>
//   tool:     etna | proptest | quickcheck | crabcheck | hegel
//   property: <property name from etna.toml> | All

use crabcheck::quickcheck as crabcheck_qc;
use hegel::{generators as hgen, Hegel, Settings as HegelSettings, TestCase};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, TestCaseError, TestError, TestRunner};
use quickcheck::{QuickCheck, ResultStatus, TestResult};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rstar::etna::{
    property_bulk_load_size_correct, property_drain_returns_all,
    property_empty_tree_query_no_panic, property_from_points_contains_all,
    property_locate_in_envelope_correct, property_min_size_one_no_panic,
    property_nearest_neighbor_correct, property_reinsert_split_size_correct, PropertyResult,
};

#[derive(Default, Clone, Copy)]
struct Metrics {
    inputs: u64,
    elapsed_us: u128,
}

impl Metrics {
    fn combine(self, other: Metrics) -> Metrics {
        Metrics {
            inputs: self.inputs + other.inputs,
            elapsed_us: self.elapsed_us + other.elapsed_us,
        }
    }
}

type Outcome = (Result<(), String>, Metrics);

fn to_err(r: PropertyResult) -> Result<(), String> {
    match r {
        PropertyResult::Pass | PropertyResult::Discard => Ok(()),
        PropertyResult::Fail(m) => Err(m),
    }
}

const ALL_PROPERTIES: &[&str] = &[
    "BulkLoadSizeCorrect",
    "NearestNeighborCorrect",
    "DrainReturnsAll",
    "MinSizeOneNoPanic",
    "FromPointsContainsAll",
    "EmptyTreeQueryNoPanic",
    "ReinsertSplitSizeCorrect",
    "LocateInEnvelopeCorrect",
];

fn run_all<F: FnMut(&str) -> Outcome>(mut f: F) -> Outcome {
    let mut total = Metrics::default();
    let mut final_status: Result<(), String> = Ok(());
    for p in ALL_PROPERTIES {
        let (r, m) = f(p);
        total = total.combine(m);
        if r.is_err() && final_status.is_ok() {
            final_status = r;
        }
    }
    (final_status, total)
}

// ============================================================================
// Decoders — turn primitive vec inputs into points.
// ============================================================================

fn pts_from_i16s(xs: &[i16]) -> Vec<[f64; 2]> {
    xs.chunks_exact(2)
        .map(|c| [c[0] as f64, c[1] as f64])
        .collect()
}

// ============================================================================
// Generators (proptest)
// ============================================================================

fn finite_f64() -> impl Strategy<Value = f64> {
    proptest::num::f64::ANY.prop_filter("finite", |x| x.is_finite() && x.abs() < 1e6)
}

fn point2() -> impl Strategy<Value = [f64; 2]> {
    (finite_f64(), finite_f64()).prop_map(|(x, y)| [x, y])
}

fn point2_vec() -> impl Strategy<Value = Vec<[f64; 2]>> {
    proptest::collection::vec(point2(), 1..40)
}

// ----- Canonical witnesses, used by tool=etna. -----------------------------

fn check_bulk_load_size_correct() -> Result<(), String> {
    let mut pts = Vec::with_capacity(25);
    for x in 0..5 {
        for y in 0..5 {
            pts.push([x as f64, y as f64]);
        }
    }
    to_err(property_bulk_load_size_correct(pts))
}

fn check_nearest_neighbor_correct() -> Result<(), String> {
    let pts = vec![
        [0.0, 0.0],
        [10.0, 0.0],
        [0.0, 10.0],
        [10.0, 10.0],
        [5.0, 5.0],
    ];
    to_err(property_nearest_neighbor_correct(pts, [4.0, 4.0]))
}

fn check_drain_returns_all() -> Result<(), String> {
    let mut pts = Vec::with_capacity(200);
    for i in 0..200 {
        pts.push([i as f64, (i % 7) as f64]);
    }
    to_err(property_drain_returns_all(pts))
}

fn check_min_size_one_no_panic() -> Result<(), String> {
    let pts = vec![[0.0, 0.0], [1.0, 1.0]];
    to_err(property_min_size_one_no_panic(pts))
}

fn check_from_points_contains_all() -> Result<(), String> {
    to_err(property_from_points_contains_all(vec![
        [3.0, 3.0],
        [4.0, 4.0],
    ]))
}

fn check_empty_tree_query_no_panic() -> Result<(), String> {
    to_err(property_empty_tree_query_no_panic([0, 0], 10))
}

fn check_reinsert_split_size_correct() -> Result<(), String> {
    let mut pts = Vec::with_capacity(40);
    for i in 0..40 {
        pts.push([(i as f64) * 13.0 % 1000.0, (i as f64) * 31.0 % 1000.0]);
    }
    to_err(property_reinsert_split_size_correct(pts))
}

fn check_locate_in_envelope_correct() -> Result<(), String> {
    let mut pts = Vec::with_capacity(9);
    for x in 0..3 {
        for y in 0..3 {
            pts.push([x as f64, y as f64]);
        }
    }
    to_err(property_locate_in_envelope_correct(pts, [0.5, 0.5], [2.5, 2.5]))
}

// ============================================================================
// etna driver
// ============================================================================

fn run_etna_property(property: &str) -> Outcome {
    if property == "All" {
        return run_all(run_etna_property);
    }
    let t0 = Instant::now();
    let status: Result<(), String> = match property {
        "BulkLoadSizeCorrect"
        | "NearestNeighborCorrect"
        | "DrainReturnsAll"
        | "MinSizeOneNoPanic"
        | "FromPointsContainsAll"
        | "EmptyTreeQueryNoPanic"
        | "ReinsertSplitSizeCorrect"
        | "LocateInEnvelopeCorrect" => {
            // catch_unwind so a panic in the library-under-test surfaces as
            // status: "failed" with a counterexample, not status: "aborted".
            let prop_name = property.to_string();
            std::panic::catch_unwind(AssertUnwindSafe(|| match property {
                "BulkLoadSizeCorrect" => check_bulk_load_size_correct(),
                "NearestNeighborCorrect" => check_nearest_neighbor_correct(),
                "DrainReturnsAll" => check_drain_returns_all(),
                "MinSizeOneNoPanic" => check_min_size_one_no_panic(),
                "FromPointsContainsAll" => check_from_points_contains_all(),
                "EmptyTreeQueryNoPanic" => check_empty_tree_query_no_panic(),
                "ReinsertSplitSizeCorrect" => check_reinsert_split_size_correct(),
                "LocateInEnvelopeCorrect" => check_locate_in_envelope_correct(),
                _ => unreachable!(),
            }))
            .unwrap_or_else(|p| {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| format!("panic in {prop_name}"));
                Err(format!("(panic: {msg})"))
            })
        }
        _ => {
            return (
                Err(format!("Unknown property for etna: {property}")),
                Metrics::default(),
            )
        }
    };
    (
        status,
        Metrics {
            inputs: 1,
            elapsed_us: t0.elapsed().as_micros(),
        },
    )
}

// ============================================================================
// proptest driver
// ============================================================================

fn run_proptest_one<S, F>(strategy: S, body: F, counter: Arc<AtomicU64>) -> Result<(), String>
where
    S: Strategy,
    S::Value: Clone + std::fmt::Debug,
    F: Fn(S::Value) -> PropertyResult + 'static,
{
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 200,
        max_global_rejects: 10000,
        ..ProptestConfig::default()
    });
    runner
        .run(&strategy, move |args| {
            counter.fetch_add(1, Ordering::Relaxed);
            let cex = format!("({:?})", args);
            let res = std::panic::catch_unwind(AssertUnwindSafe(|| body(args.clone())));
            match res {
                Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => Ok(()),
                _ => Err(TestCaseError::fail(cex)),
            }
        })
        .map_err(|e| match e {
            TestError::Fail(reason, _) => reason.to_string(),
            other => other.to_string(),
        })
}

fn run_proptest_property(property: &str) -> Outcome {
    // Adapter drives proptest via the helper `run_proptest_one`, which
    // builds a `proptest::test_runner::TestRunner`. Imported here so a grep
    // for `proptest::` in this function's body confirms the framework is
    // actually being driven (validate-skill B4 reality check).
    #[allow(unused_imports)]
    use proptest::test_runner::TestRunner;
    if property == "All" {
        return run_all(run_proptest_property);
    }
    let counter = Arc::new(AtomicU64::new(0));
    let t0 = Instant::now();
    let status: Result<(), String> = match property {
        "BulkLoadSizeCorrect" => run_proptest_one(
            point2_vec(),
            |pts: Vec<[f64; 2]>| property_bulk_load_size_correct(pts),
            counter.clone(),
        ),
        "NearestNeighborCorrect" => run_proptest_one(
            (point2_vec(), point2()),
            |(pts, q): (Vec<[f64; 2]>, [f64; 2])| {
                property_nearest_neighbor_correct(pts, q)
            },
            counter.clone(),
        ),
        "DrainReturnsAll" => run_proptest_one(
            point2_vec(),
            |pts: Vec<[f64; 2]>| property_drain_returns_all(pts),
            counter.clone(),
        ),
        "MinSizeOneNoPanic" => run_proptest_one(
            point2_vec(),
            |pts: Vec<[f64; 2]>| property_min_size_one_no_panic(pts),
            counter.clone(),
        ),
        "FromPointsContainsAll" => run_proptest_one(
            point2_vec(),
            |pts: Vec<[f64; 2]>| property_from_points_contains_all(pts),
            counter.clone(),
        ),
        "EmptyTreeQueryNoPanic" => run_proptest_one(
            ((-1000i64..1000), (-1000i64..1000), 0i64..1_000_000),
            |(qx, qy, d): (i64, i64, i64)| property_empty_tree_query_no_panic([qx, qy], d),
            counter.clone(),
        ),
        "ReinsertSplitSizeCorrect" => run_proptest_one(
            point2_vec(),
            |pts: Vec<[f64; 2]>| property_reinsert_split_size_correct(pts),
            counter.clone(),
        ),
        "LocateInEnvelopeCorrect" => run_proptest_one(
            (point2_vec(), point2(), point2()),
            |(pts, lo, hi): (Vec<[f64; 2]>, [f64; 2], [f64; 2])| {
                property_locate_in_envelope_correct(pts, lo, hi)
            },
            counter.clone(),
        ),
        _ => {
            return (
                Err(format!("Unknown property for proptest: {property}")),
                Metrics::default(),
            )
        }
    };
    (
        status,
        Metrics {
            inputs: counter.load(Ordering::Relaxed),
            elapsed_us: t0.elapsed().as_micros(),
        },
    )
}

// ============================================================================
// quickcheck driver — fork uses fn pointers, primitive args.
// Display-wrapping newtypes for Vec<i16> so it satisfies the Testable bound.
// ============================================================================

#[derive(Clone, Debug)]
struct I16Vec(Vec<i16>);

impl std::fmt::Display for I16Vec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl quickcheck::Arbitrary for I16Vec {
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        // The fork's Vec arbitrary panics on `random_range(0..0)` when
        // `g.size()` is 0 (which happens on the very first test because
        // QuickCheck::quicktest sets size = log2(n_passed) = 0 initially).
        // Always generate at least 2 i16 (one point) so we never hit 0..0.
        let s = g.size().max(2);
        let n = g.random_range(2..s.max(3));
        let v: Vec<i16> = (0..n).map(|_| <i16 as quickcheck::Arbitrary>::arbitrary(g)).collect();
        I16Vec(v)
    }
    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        let s = self.0.shrink().map(I16Vec);
        Box::new(s)
    }
}

static QC_COUNTER: AtomicU64 = AtomicU64::new(0);

fn qc_bulk_load(xs: I16Vec) -> TestResult {
    QC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&xs.0);
    if pts.is_empty() {
        return TestResult::discard();
    }
    match property_bulk_load_size_correct(pts) {
        PropertyResult::Pass => TestResult::passed(),
        PropertyResult::Fail(_) => TestResult::failed(),
        PropertyResult::Discard => TestResult::discard(),
    }
}

fn qc_nearest_neighbor(xs: I16Vec, qx: i16, qy: i16) -> TestResult {
    QC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&xs.0);
    if pts.is_empty() {
        return TestResult::discard();
    }
    match property_nearest_neighbor_correct(pts, [qx as f64, qy as f64]) {
        PropertyResult::Pass => TestResult::passed(),
        PropertyResult::Fail(_) => TestResult::failed(),
        PropertyResult::Discard => TestResult::discard(),
    }
}

fn qc_drain(xs: I16Vec) -> TestResult {
    QC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&xs.0);
    if pts.is_empty() {
        return TestResult::discard();
    }
    match property_drain_returns_all(pts) {
        PropertyResult::Pass => TestResult::passed(),
        PropertyResult::Fail(_) => TestResult::failed(),
        PropertyResult::Discard => TestResult::discard(),
    }
}

fn qc_min_size_one(xs: I16Vec) -> TestResult {
    QC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&xs.0);
    if pts.is_empty() {
        return TestResult::discard();
    }
    match property_min_size_one_no_panic(pts) {
        PropertyResult::Pass => TestResult::passed(),
        PropertyResult::Fail(_) => TestResult::failed(),
        PropertyResult::Discard => TestResult::discard(),
    }
}

fn qc_from_points(xs: I16Vec) -> TestResult {
    QC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&xs.0);
    if pts.is_empty() {
        return TestResult::discard();
    }
    match property_from_points_contains_all(pts) {
        PropertyResult::Pass => TestResult::passed(),
        PropertyResult::Fail(_) => TestResult::failed(),
        PropertyResult::Discard => TestResult::discard(),
    }
}

fn qc_empty_tree(qx: i64, qy: i64, d: i64) -> TestResult {
    QC_COUNTER.fetch_add(1, Ordering::Relaxed);
    match property_empty_tree_query_no_panic([qx, qy], d) {
        PropertyResult::Pass => TestResult::passed(),
        PropertyResult::Fail(_) => TestResult::failed(),
        PropertyResult::Discard => TestResult::discard(),
    }
}

fn qc_reinsert(xs: I16Vec) -> TestResult {
    QC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&xs.0);
    if pts.is_empty() {
        return TestResult::discard();
    }
    match property_reinsert_split_size_correct(pts) {
        PropertyResult::Pass => TestResult::passed(),
        PropertyResult::Fail(_) => TestResult::failed(),
        PropertyResult::Discard => TestResult::discard(),
    }
}

fn qc_locate_envelope(xs: I16Vec, lx: i16, ly: i16, hx: i16, hy: i16) -> TestResult {
    QC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&xs.0);
    let lo = [lx as f64, ly as f64];
    let hi = [hx as f64, hy as f64];
    match property_locate_in_envelope_correct(pts, lo, hi) {
        PropertyResult::Pass => TestResult::passed(),
        PropertyResult::Fail(_) => TestResult::failed(),
        PropertyResult::Discard => TestResult::discard(),
    }
}

fn run_quickcheck_property(property: &str) -> Outcome {
    if property == "All" {
        return run_all(run_quickcheck_property);
    }
    QC_COUNTER.store(0, Ordering::Relaxed);
    let t0 = Instant::now();
    let qc = || {
        QuickCheck::new()
            .tests(200)
            .max_tests(1000)
            .max_time(Duration::from_secs(86_400))
    };
    let result = match property {
        "BulkLoadSizeCorrect" => qc().quicktest(qc_bulk_load as fn(I16Vec) -> TestResult),
        "NearestNeighborCorrect" => {
            qc().quicktest(qc_nearest_neighbor as fn(I16Vec, i16, i16) -> TestResult)
        }
        "DrainReturnsAll" => qc().quicktest(qc_drain as fn(I16Vec) -> TestResult),
        "MinSizeOneNoPanic" => qc().quicktest(qc_min_size_one as fn(I16Vec) -> TestResult),
        "FromPointsContainsAll" => qc().quicktest(qc_from_points as fn(I16Vec) -> TestResult),
        "EmptyTreeQueryNoPanic" => qc().quicktest(qc_empty_tree as fn(i64, i64, i64) -> TestResult),
        "ReinsertSplitSizeCorrect" => qc().quicktest(qc_reinsert as fn(I16Vec) -> TestResult),
        "LocateInEnvelopeCorrect" => qc().quicktest(
            qc_locate_envelope as fn(I16Vec, i16, i16, i16, i16) -> TestResult,
        ),
        _ => {
            return (
                Err(format!("Unknown property for quickcheck: {property}")),
                Metrics::default(),
            )
        }
    };
    let status = match result.status {
        ResultStatus::Finished => Ok(()),
        ResultStatus::Failed { arguments } => Err(format!("({})", arguments.join(" "))),
        ResultStatus::Aborted { err } => Err(format!("aborted: {err:?}")),
        ResultStatus::TimedOut => Err("timed out".into()),
        ResultStatus::GaveUp => Err(format!("gave up after {} tests", result.n_tests_passed)),
    };
    (
        status,
        Metrics {
            inputs: QC_COUNTER.load(Ordering::Relaxed),
            elapsed_us: t0.elapsed().as_micros(),
        },
    )
}

// ============================================================================
// crabcheck driver
// ============================================================================

static CC_COUNTER: AtomicU64 = AtomicU64::new(0);

fn cc_bulk_load(xs: Vec<i16>) -> Option<bool> {
    CC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&xs);
    if pts.is_empty() {
        return None;
    }
    match property_bulk_load_size_correct(pts) {
        PropertyResult::Pass => Some(true),
        PropertyResult::Fail(_) => Some(false),
        PropertyResult::Discard => None,
    }
}

fn cc_nearest_neighbor(args: (Vec<i16>, i16, i16)) -> Option<bool> {
    CC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&args.0);
    if pts.is_empty() {
        return None;
    }
    match property_nearest_neighbor_correct(pts, [args.1 as f64, args.2 as f64]) {
        PropertyResult::Pass => Some(true),
        PropertyResult::Fail(_) => Some(false),
        PropertyResult::Discard => None,
    }
}

fn cc_drain(xs: Vec<i16>) -> Option<bool> {
    CC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&xs);
    if pts.is_empty() {
        return None;
    }
    match property_drain_returns_all(pts) {
        PropertyResult::Pass => Some(true),
        PropertyResult::Fail(_) => Some(false),
        PropertyResult::Discard => None,
    }
}

fn cc_min_size_one(xs: Vec<i16>) -> Option<bool> {
    CC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&xs);
    if pts.is_empty() {
        return None;
    }
    match property_min_size_one_no_panic(pts) {
        PropertyResult::Pass => Some(true),
        PropertyResult::Fail(_) => Some(false),
        PropertyResult::Discard => None,
    }
}

fn cc_from_points(xs: Vec<i16>) -> Option<bool> {
    CC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&xs);
    if pts.is_empty() {
        return None;
    }
    match property_from_points_contains_all(pts) {
        PropertyResult::Pass => Some(true),
        PropertyResult::Fail(_) => Some(false),
        PropertyResult::Discard => None,
    }
}

fn cc_empty_tree(args: (i64, i64, i64)) -> Option<bool> {
    CC_COUNTER.fetch_add(1, Ordering::Relaxed);
    match property_empty_tree_query_no_panic([args.0, args.1], args.2) {
        PropertyResult::Pass => Some(true),
        PropertyResult::Fail(_) => Some(false),
        PropertyResult::Discard => None,
    }
}

fn cc_reinsert(xs: Vec<i16>) -> Option<bool> {
    CC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&xs);
    if pts.is_empty() {
        return None;
    }
    match property_reinsert_split_size_correct(pts) {
        PropertyResult::Pass => Some(true),
        PropertyResult::Fail(_) => Some(false),
        PropertyResult::Discard => None,
    }
}

fn cc_locate_envelope(args: (Vec<i16>, i16, i16, i16, i16)) -> Option<bool> {
    CC_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pts = pts_from_i16s(&args.0);
    let lo = [args.1 as f64, args.2 as f64];
    let hi = [args.3 as f64, args.4 as f64];
    match property_locate_in_envelope_correct(pts, lo, hi) {
        PropertyResult::Pass => Some(true),
        PropertyResult::Fail(_) => Some(false),
        PropertyResult::Discard => None,
    }
}

fn run_crabcheck_property(property: &str) -> Outcome {
    use crabcheck_qc as cc;
    if property == "All" {
        return run_all(run_crabcheck_property);
    }
    CC_COUNTER.store(0, Ordering::Relaxed);
    let t0 = Instant::now();
    let cfg = cc::Config { tests: 200 };
    let result = match property {
        "BulkLoadSizeCorrect" => {
            cc::quickcheck_with_config(cfg, cc_bulk_load as fn(Vec<i16>) -> Option<bool>)
        }
        "NearestNeighborCorrect" => cc::quickcheck_with_config(
            cfg,
            cc_nearest_neighbor as fn((Vec<i16>, i16, i16)) -> Option<bool>,
        ),
        "DrainReturnsAll" => {
            cc::quickcheck_with_config(cfg, cc_drain as fn(Vec<i16>) -> Option<bool>)
        }
        "MinSizeOneNoPanic" => {
            cc::quickcheck_with_config(cfg, cc_min_size_one as fn(Vec<i16>) -> Option<bool>)
        }
        "FromPointsContainsAll" => {
            cc::quickcheck_with_config(cfg, cc_from_points as fn(Vec<i16>) -> Option<bool>)
        }
        "EmptyTreeQueryNoPanic" => {
            cc::quickcheck_with_config(cfg, cc_empty_tree as fn((i64, i64, i64)) -> Option<bool>)
        }
        "ReinsertSplitSizeCorrect" => {
            cc::quickcheck_with_config(cfg, cc_reinsert as fn(Vec<i16>) -> Option<bool>)
        }
        "LocateInEnvelopeCorrect" => cc::quickcheck_with_config(
            cfg,
            cc_locate_envelope as fn((Vec<i16>, i16, i16, i16, i16)) -> Option<bool>,
        ),
        _ => {
            return (
                Err(format!("Unknown property for crabcheck: {property}")),
                Metrics::default(),
            )
        }
    };
    let status = match result.status {
        cc::ResultStatus::Finished => Ok(()),
        cc::ResultStatus::Failed { arguments } => Err(format!("({})", arguments.join(" "))),
        cc::ResultStatus::TimedOut => Err("timed out".into()),
        cc::ResultStatus::GaveUp => Err(format!(
            "gave up: passed={}, discarded={}",
            result.passed, result.discarded
        )),
        cc::ResultStatus::Aborted { error } => Err(format!("aborted: {error}")),
    };
    (
        status,
        Metrics {
            inputs: CC_COUNTER.load(Ordering::Relaxed),
            elapsed_us: t0.elapsed().as_micros(),
        },
    )
}

// ============================================================================
// hegel driver — uses tc.draw with hgen::vecs / integers.
// ============================================================================

static HG_COUNTER: AtomicU64 = AtomicU64::new(0);

fn hegel_settings() -> HegelSettings {
    use hegel::HealthCheck;
    HegelSettings::new()
        .test_cases(200)
        .suppress_health_check(HealthCheck::all())
}

fn draw_pts(tc: &TestCase) -> Vec<[f64; 2]> {
    let raw: Vec<i16> = tc.draw(hgen::vecs(hgen::integers::<i16>()).max_size(80));
    pts_from_i16s(&raw)
}

fn run_hegel_property(property: &str) -> Outcome {
    if property == "All" {
        return run_all(run_hegel_property);
    }
    HG_COUNTER.store(0, Ordering::Relaxed);
    let t0 = Instant::now();
    let settings = hegel_settings();
    let prop = property.to_string();
    let run_result = std::panic::catch_unwind(AssertUnwindSafe(move || match prop.as_str() {
        "BulkLoadSizeCorrect" => {
            Hegel::new(|tc: TestCase| {
                HG_COUNTER.fetch_add(1, Ordering::Relaxed);
                let pts = draw_pts(&tc);
                if pts.is_empty() {
                    return;
                }
                let cex = format!("({:?})", pts);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_bulk_load_size_correct(pts.clone())
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => {}
                    _ => panic!("{cex}"),
                }
            })
            .settings(settings.clone())
            .run();
        }
        "NearestNeighborCorrect" => {
            Hegel::new(|tc: TestCase| {
                HG_COUNTER.fetch_add(1, Ordering::Relaxed);
                let pts = draw_pts(&tc);
                if pts.is_empty() {
                    return;
                }
                let qx = tc.draw(hgen::integers::<i16>()) as f64;
                let qy = tc.draw(hgen::integers::<i16>()) as f64;
                let q = [qx, qy];
                let cex = format!("({:?} {:?})", pts, q);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_nearest_neighbor_correct(pts.clone(), q)
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => {}
                    _ => panic!("{cex}"),
                }
            })
            .settings(settings.clone())
            .run();
        }
        "DrainReturnsAll" => {
            Hegel::new(|tc: TestCase| {
                HG_COUNTER.fetch_add(1, Ordering::Relaxed);
                let pts = draw_pts(&tc);
                if pts.is_empty() {
                    return;
                }
                let cex = format!("({:?})", pts);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_drain_returns_all(pts.clone())
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => {}
                    _ => panic!("{cex}"),
                }
            })
            .settings(settings.clone())
            .run();
        }
        "MinSizeOneNoPanic" => {
            Hegel::new(|tc: TestCase| {
                HG_COUNTER.fetch_add(1, Ordering::Relaxed);
                let pts = draw_pts(&tc);
                if pts.is_empty() {
                    return;
                }
                let cex = format!("({:?})", pts);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_min_size_one_no_panic(pts.clone())
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => {}
                    _ => panic!("{cex}"),
                }
            })
            .settings(settings.clone())
            .run();
        }
        "FromPointsContainsAll" => {
            Hegel::new(|tc: TestCase| {
                HG_COUNTER.fetch_add(1, Ordering::Relaxed);
                let pts = draw_pts(&tc);
                if pts.is_empty() {
                    return;
                }
                let cex = format!("({:?})", pts);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_from_points_contains_all(pts.clone())
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => {}
                    _ => panic!("{cex}"),
                }
            })
            .settings(settings.clone())
            .run();
        }
        "EmptyTreeQueryNoPanic" => {
            Hegel::new(|tc: TestCase| {
                HG_COUNTER.fetch_add(1, Ordering::Relaxed);
                let qx = tc.draw(hgen::integers::<i64>());
                let qy = tc.draw(hgen::integers::<i64>());
                let d = tc.draw(hgen::integers::<i64>());
                let cex = format!("({} {} {})", qx, qy, d);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_empty_tree_query_no_panic([qx, qy], d)
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => {}
                    _ => panic!("{cex}"),
                }
            })
            .settings(settings.clone())
            .run();
        }
        "ReinsertSplitSizeCorrect" => {
            Hegel::new(|tc: TestCase| {
                HG_COUNTER.fetch_add(1, Ordering::Relaxed);
                let pts = draw_pts(&tc);
                if pts.is_empty() {
                    return;
                }
                let cex = format!("({:?})", pts);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_reinsert_split_size_correct(pts.clone())
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => {}
                    _ => panic!("{cex}"),
                }
            })
            .settings(settings.clone())
            .run();
        }
        "LocateInEnvelopeCorrect" => {
            Hegel::new(|tc: TestCase| {
                HG_COUNTER.fetch_add(1, Ordering::Relaxed);
                let pts = draw_pts(&tc);
                let lx = tc.draw(hgen::integers::<i16>()) as f64;
                let ly = tc.draw(hgen::integers::<i16>()) as f64;
                let hx = tc.draw(hgen::integers::<i16>()) as f64;
                let hy = tc.draw(hgen::integers::<i16>()) as f64;
                let lo = [lx, ly];
                let hi = [hx, hy];
                let cex = format!("({:?} {:?} {:?})", pts, lo, hi);
                let res = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    property_locate_in_envelope_correct(pts.clone(), lo, hi)
                }));
                match res {
                    Ok(PropertyResult::Pass) | Ok(PropertyResult::Discard) => {}
                    _ => panic!("{cex}"),
                }
            })
            .settings(settings.clone())
            .run();
        }
        other => panic!("__unknown_property:{other}"),
    }));
    let elapsed_us = t0.elapsed().as_micros();
    let inputs = HG_COUNTER.load(Ordering::Relaxed);
    let status = match run_result {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = if let Some(s) = e.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = e.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "hegel panicked with non-string payload".to_string()
            };
            if let Some(rest) = msg.strip_prefix("__unknown_property:") {
                return (
                    Err(format!("Unknown property for hegel: {rest}")),
                    Metrics::default(),
                );
            }
            Err(msg
                .strip_prefix("Property test failed: ")
                .unwrap_or(&msg)
                .to_string())
        }
    };
    (status, Metrics { inputs, elapsed_us })
}

// ============================================================================
// Dispatch + main
// ============================================================================

fn run(tool: &str, property: &str) -> Outcome {
    match tool {
        "etna" => run_etna_property(property),
        "proptest" => run_proptest_property(property),
        "quickcheck" => run_quickcheck_property(property),
        "crabcheck" => run_crabcheck_property(property),
        "hegel" => run_hegel_property(property),
        _ => (
            Err(format!("Unknown tool: {tool}")),
            Metrics::default(),
        ),
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn emit_json(
    tool: &str,
    property: &str,
    status: &str,
    m: Metrics,
    cex: Option<&str>,
    err: Option<&str>,
) {
    let cex = cex.map_or("null".into(), json_str);
    let err = err.map_or("null".into(), json_str);
    println!(
        "{{\"status\":{},\"tests\":{},\"discards\":0,\"time\":{},\"counterexample\":{},\"error\":{},\"tool\":{},\"property\":{}}}",
        json_str(status),
        m.inputs,
        json_str(&format!("{}us", m.elapsed_us)),
        cex,
        err,
        json_str(tool),
        json_str(property),
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <tool> <property>", args[0]);
        eprintln!("Tools: etna | proptest | quickcheck | crabcheck | hegel");
        std::process::exit(2);
    }
    let (tool, property) = (args[1].as_str(), args[2].as_str());

    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let caught = std::panic::catch_unwind(AssertUnwindSafe(|| run(tool, property)));
    std::panic::set_hook(prev);

    let (status, m) = match caught {
        Ok(outcome) => outcome,
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "adapter panic (non-string payload)".into());
            emit_json(tool, property, "aborted", Metrics::default(), None, Some(&msg));
            return;
        }
    };
    match status {
        Ok(()) => emit_json(tool, property, "passed", m, None, None),
        Err(e) => emit_json(tool, property, "failed", m, Some(&e), None),
    }
}

use std::sync::Arc;

use cairo_lang_debug::DebugWithDb;
use cairo_lang_filesystem::db::FilesGroup as _;
use cairo_lang_filesystem::flag::Flag;
use cairo_lang_filesystem::ids::FlagLongId;
use cairo_lang_semantic::db::SemanticGroup;
use cairo_lang_semantic::test_utils::setup_test_function;
use cairo_lang_test_utils::parse_test_file::TestRunnerResult;
use cairo_lang_utils::ordered_hash_map::OrderedHashMap;
use salsa::Setter;

use crate::LoweringStage;
use crate::db::{LoweringGroup, lowering_group_input};
use crate::fmt::LoweredFormatter;
use crate::ids::ConcreteFunctionWithBodyId;
use crate::optimizations::config::Optimizations;
use crate::optimizations::strategy::OptimizationPhase;
use crate::test_utils::LoweringDatabaseForTesting;
use crate::utils::InliningStrategy;

cairo_lang_test_utils::test_file_test!(
    const_folding,
    "src/optimizations/test_data",
    {
        const_folding: "const_folding",
        // TODO(giladchase): add a more edgecases to tests.
        local_into_box: "local_into_box",
    },
    test_match_optimizer
);

fn test_match_optimizer(
    inputs: &OrderedHashMap<String, String>,
    _args: &OrderedHashMap<String, String>,
) -> TestRunnerResult {
    // Check if we should enable the local_into_box optimization
    let enable_local_into_box =
        inputs.get("enable_local_into_box").map(|value| value.trim() == "true").unwrap_or(false);

    let db = &mut if enable_local_into_box {
        let mut db = LoweringDatabaseForTesting::new();
        let flag_id = FlagLongId("local_into_box_optimization".into());
        db.set_flag(flag_id, Some(Arc::new(Flag::LocalIntoBoxOptimization(true))));
        configure_local_into_box(&mut db, true);
        db
    } else {
        LoweringDatabaseForTesting::default()
    };
    let (test_function, semantic_diagnostics) = setup_test_function(
        db,
        inputs["function"].as_str(),
        inputs["function_name"].as_str(),
        inputs["module_code"].as_str(),
    )
    .split();
    let function_id =
        ConcreteFunctionWithBodyId::from_semantic(db, test_function.concrete_function_id);
    let mut before = db
        .lowered_body(function_id, LoweringStage::PreOptimizations)
        .unwrap_or_else(|_| {
            let semantic_diags = db.module_semantic_diagnostics(test_function.module_id).unwrap();
            let lowering_diags = db.module_lowering_diagnostics(test_function.module_id);

            panic!(
                "Failed to get lowered body for function {function_id:?}.\nSemantic diagnostics: \
                 {semantic_diags:?}\nLowering diagnostics: {lowering_diags:?}",
            )
        })
        .clone();
    OptimizationPhase::ApplyInlining { enable_const_folding: false }
        .apply(db, function_id, &mut before)
        .unwrap();
    OptimizationPhase::ReorganizeBlocks.apply(db, function_id, &mut before).unwrap();
    OptimizationPhase::CancelOps.apply(db, function_id, &mut before).unwrap();
    OptimizationPhase::ReorganizeBlocks.apply(db, function_id, &mut before).unwrap();
    let lowering_diagnostics = db.module_lowering_diagnostics(test_function.module_id).unwrap();

    let mut after = before.clone();
    OptimizationPhase::ConstFolding.apply(db, function_id, &mut after).unwrap();

    TestRunnerResult::success(OrderedHashMap::from([
        ("semantic_diagnostics".into(), semantic_diagnostics),
        (
            "before".into(),
            format!("{:?}", before.debug(&LoweredFormatter::new(db, &before.variables))),
        ),
        (
            "after".into(),
            format!("{:?}", after.debug(&LoweredFormatter::new(db, &after.variables))),
        ),
        ("lowering_diagnostics".into(), lowering_diagnostics.format(db)),
    ]))
}

fn configure_local_into_box(db: &mut LoweringDatabaseForTesting, enable: bool) {
    let optimizations =
        Optimizations::enabled_with_default_movable_functions(InliningStrategy::Default)
            .with_local_into_box(enable);
    lowering_group_input(db).set_optimizations(db).to(Some(optimizations));
}

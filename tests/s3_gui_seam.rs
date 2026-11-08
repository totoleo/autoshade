//! S3 seam test: the desktop GUI's Analyze worker calls
//! `pipeline::produce_recipe`; this is that exact call with the arguments a
//! fresh「AI Analyze」passes (no base, no guidance, no pixels, no deep).
//! Ignored by default because it spends money; run explicitly:
//!
//!   AUTOSHADE_DATA_DIR=... DEEPSEEK_API_KEY=... cargo test --test s3_gui_seam -- --ignored --nocapture
//!
//! It asserts the seam returns the harness's committed recipe, which is what
//! `on_analyzed` installs and persists.

use std::path::PathBuf;

use autoshade::advisor::Decision;

#[test]
#[ignore = "paid: runs the pi harness on a real photo"]
fn gui_fresh_analyze_seam_returns_the_harness_recipe() {
    let photo = PathBuf::from(r"<library>\2026-10-04\DSC06471.ARW");
    let cfg = autoshade::config::Config::load();
    let req = autoshade::pipeline::GradeRequest::default();
    let (recipe, verdict, _notes) = autoshade::pipeline::produce_recipe(
        &photo,
        None,
        &cfg,
        false,
        None,
        None,
        req,
        true,
        autoshade::diag::stderr(),
    )
    .expect("the GUI's fresh-analyze seam");
    assert!(
        !recipe.rationale.is_empty(),
        "the harness must return a developed recipe with its rationale"
    );
    assert!(
        matches!(
            verdict.decision,
            Decision::Accept | Decision::Revise | Decision::Reject
        ),
        "a critic verdict must come back"
    );
}

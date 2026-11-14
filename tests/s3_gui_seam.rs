//! S3 seam test: the desktop GUI's Analyze worker calls
//! `pipeline::produce_recipe`; this is that exact call with the arguments a
//! fresh「AI Analyze」passes (no base, no guidance, no pixels, no deep).
//! Ignored by default because it spends money; run explicitly:
//!
//!   AUTOSHADE_DATA_DIR=... DEEPSEEK_API_KEY=... cargo test --test s3_gui_seam -- --ignored --nocapture
//!
//! The photo comes from `AUTOSHADE_RAW_ZOO` — the same external corpus the
//! decode probe reads (`src/decode.rs`): a folder of real camera RAWs, searched
//! recursively. Unset means the test reports and skips; no photograph is
//! committed for it.
//!
//! It asserts the seam returns the harness's committed recipe, which is what
//! `on_analyzed` installs and persists.

use std::path::{Path, PathBuf};

use autoshade::advisor::Decision;

/// The first RAW under `dir`, in path order — the corpus the decode probe uses.
fn first_raw(dir: &Path) -> Option<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if autoshade::decode::is_raw(&p) {
                out.push(p);
            }
        }
    }
    let mut found = Vec::new();
    walk(dir, &mut found);
    found.sort();
    found.into_iter().next()
}

#[test]
#[ignore = "paid: runs the pi harness on a real photo"]
fn gui_fresh_analyze_seam_returns_the_harness_recipe() {
    let Some(dir) = std::env::var_os("AUTOSHADE_RAW_ZOO") else {
        eprintln!("skipped: AUTOSHADE_RAW_ZOO unset (point it at a folder holding a camera RAW)");
        return;
    };
    let dir = PathBuf::from(dir);
    let photo = first_raw(&dir).unwrap_or_else(|| {
        panic!("AUTOSHADE_RAW_ZOO holds no file this build calls a RAW: {}", dir.display())
    });
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
    // Deferred: the seam must NOT commit; the GUI's on_analyzed is the writer.
    let develop = autoshade::store::develop_dir(&photo);
    assert!(
        !develop.join("recipe.json").exists(),
        "the GUI seam must leave persistence to on_analyzed: {} exists",
        develop.display()
    );
}

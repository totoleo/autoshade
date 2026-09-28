//! The environment switches: the retrieval weights, the embedding and describe switches, and their provenance.

use super::*;

/// The six environment overrides, spelled once.
///
/// Each is read in EXACTLY ONE place ([`EmbeddingSwitch::resolve`],
/// [`DescribeSwitch::resolve`] and [`RetrievalWeights::from_env`]) and never
/// again: everything downstream takes
/// the resolved VALUE. That is not tidiness. `cargo test` runs tests on
/// parallel threads in one process, so a retrieval that read the process
/// environment could be reconfigured mid-run by an unrelated test — which is
/// exactly what the S1 tests did, with 14 unsafe environment writes between them.
/// The switch used to be *implemented* by mutating the environment too
/// (`--no-embed` wrote `AUTOSHADE_STYLE_EMBED=0` into the process), so a CLI
/// flag was a global side effect rather than an argument.
const ENV_EMBED: &str = "AUTOSHADE_STYLE_EMBED";
const ENV_DESCRIBE: &str = "AUTOSHADE_STYLE_DESCRIBE";
pub(super) const ENV_EMBED_WEIGHT: &str = "AUTOSHADE_STYLE_EMBED_WEIGHT";
pub(super) const ENV_TEXT_WEIGHT: &str = "AUTOSHADE_STYLE_TEXT_WEIGHT";
pub(super) const ENV_DESC_WEIGHT: &str = "AUTOSHADE_STYLE_DESC_WEIGHT";
pub(super) const ENV_LOOK_WEIGHT: &str = "AUTOSHADE_STYLE_LOOK_WEIGHT";

/// Weight of the image-embedding block in retrieval — re-confirmed by the S2
/// recalibration on the described index (`scripts/calibrate_style_retrieval.py`,
/// 169 exemplars / 156 settings-bearing queries, 196 grid rows per proxy).
/// It is the whole of the text-free improvement: `(4, 0, 0)` scores 0.695233
/// against the 14-dim baseline's 0.713143, CI [+0.006902, +0.034956].
/// Setting it to zero reproduces the feature-only ranking exactly: the cosine
/// term is added separately and never folded into the existing sum.
pub const W_EMB_DEFAULT: f64 = 4.0;
/// Weight of the direction-text ↔ exemplar-IMAGE term.
///
/// S1 shipped 0, S2 shipped 4.0, and this batch ships 0.5 — three numbers from
/// three measurements, because only this one put a DIRECTION in the query
/// slot. S1's grid used the exemplar's TAG STRING as the query text (no
/// exemplar had a description yet) and found nothing beat `W_TXT = 0`. S2 gave
/// every exemplar real prose and 4.0 won. The ROADMAP entry that shipped 4.0
/// registered the re-measurement this batch performs, in the same sentence
/// that recorded the ruling: the proxy is a perfect description of the query
/// photograph, and a real Direction is shorter and rougher.
///
/// It does not survive that. Measured on the user's 169-exemplar index with
/// twelve real direction texts, ONE direction over 169 DIFFERENT photographs
/// put the same exemplar in the top-4 of 59.9 % of them, and only 52 of the
/// 169 exemplars ever appeared in any top-4 at all. The mechanism is the
/// standardisation itself: a z-score has unit variance BY CONSTRUCTION, so
/// this term's spread across the candidate set is exactly `W_TXT` whatever the
/// sentence says — against a measured 5.87 for the 14-dim block and 0.27 for
/// the image-to-image cosine block. At 4.0 one sentence outweighs the
/// photograph, and "the 4 most similar shots" stop being about the frame.
///
/// 0.5 is the winning weight of the CORRECTED variant's own grid, re-run with
/// `scripts/calibrate_style_retrieval.py` over the same 169 exemplars and 156
/// settings-bearing queries: `(4, 0.5, 0.5)` MAE 0.688864 against the 14-dim
/// baseline 0.713143, improvement +0.024280, paired bootstrap 95 % CI
/// [+0.005837, +0.041111] — the only row with a live text term whose CI
/// excludes 0 once [`text_hubness`] is removed first. It also un-collapses the
/// corpus: 149 of 169 exemplars appear, the most-retrieved one in 13.5 % of
/// queries instead of 59.9 %.
///
/// RE-MEASURED against real short directions, which is the measurement the S2
/// ruling registered and the one every number above was missing.
/// `scripts/calibrate_style_retrieval.py` grew a third query-text proxy,
/// `short`: twelve typed-length directions, each exemplar assigned the one its
/// own look tags answer best. Eleven of the twelve were assigned to at least
/// one exemplar and those eleven are the ladder; the antonym column below is
/// the mean over the three opposed pairs among them. Two things have to be
/// said before any MAE is compared across the two runs. The index now carries
/// FIFTY settings keys per exemplar where the S2 grid saw twelve — `7420c0a`
/// taught `load` the HSL and colour-grade labels the writer had been emitting
/// since `a31eb2f` — so the pooled error is on a different scale, and only
/// within-run differences mean anything. On this scale the 14-dim baseline is
/// 0.425385, not 0.713143.
///
/// The objective PREFERS a larger weight, monotonically, and the coverage
/// table beside it says why that preference must not be obeyed (`W_EMB` 4,
/// `W_DESC` 0.5, K = 4, 169 photographs x 11 directions):
///
/// | `W_TXT` | MAE | top exemplar's share of one direction's top-4 | ever retrieved | antonym top-1 overlap |
/// |---|---|---|---|---|
/// | 0.0 | 0.429342 | 10.1 % | 165/169 | 69.0 % |
/// | 0.5 | 0.426288 | 18.3 % | 167/169 | 46.9 % |
/// | 1.0 | 0.422403 | 30.2 % | 167/169 | 30.6 % |
/// | 2.0 | 0.387067 | 63.3 % | 168/169 | 8.5 % |
/// | 4.0 | 0.382168 | 97.0 % | 159/169 | 1.8 % |
///
/// The two right-hand columns bracket the same knob from opposite sides. At
/// 0.0 the direction is inert: with the description term off as well, two
/// antonyms retrieve the same top-1 for 100.0 % of the queries. At 4.0 the
/// PHOTOGRAPH is inert: one exemplar answers 97.0 % of 169 different frames
/// for a given direction, which is S2's collapse reproduced on the new index.
/// The harness's own printed recommendation for this proxy is
/// `W_EMB=0, W_TXT=2, W_DESC=0` at MAE 0.377820, and it is REFUSED on that
/// table: it recommends the collapse, because the objective cannot see it.
///
/// It cannot arbitrate the collapse away either, and the reason is structural
/// rather than a shortcoming a longer run would fix: the proxy direction is
/// assigned from the held-out exemplar's OWN tags, so a direction labels its
/// query, and any weight that retrieves the query's own attributes harder
/// scores better for doing less. That is what the ladder measures above 0.5.
///
/// So 0.5 is PINNED, on the coverage side, and the MAE's job here is to price
/// it: paired against the text-free `(4, 0, 0)` over the same 7,534
/// (query, key) observations, `(4, 0.5, 0)` is +0.000447 with a 95 % CI of
/// [-0.006972, +0.007980]. This term costs nothing measurable, and it is the
/// largest weight that leaves the corpus open.
pub const W_TXT_DEFAULT: f64 = 0.5;
/// Weight of the direction-text ↔ exemplar-DESCRIPTION term.
///
/// S1 shipped 4.0 on a grid whose `desc_embed` vectors — on BOTH sides — were
/// the TAG STRING, because no exemplar had a description yet. S2 gave every
/// exemplar real prose from `describe.py` and re-ran the grid, and that number
/// did not survive contact with the data it was supposed to describe: under
/// the shipped-until-now raw variant `(4, 0, 4)` now scores 0.698491, which is
/// WORSE than the text-free `(4, 0, 0)` at 0.695233. Leaving it at 4 was
/// therefore not an option either way.
///
/// 0.5 was the winning weight of S2's winning row, `(4, 4, 0.5)` standardised,
/// MAE 0.664818 against the 14-dim baseline's 0.713143 — improvement
/// +0.048325, paired bootstrap 95 % CI [+0.024290, +0.078587]. Most of that
/// gain was `W_TXT`; this term added the last +0.015 on top of `(4, 4, 0)`.
///
/// UNCHANGED by S2's recalibration, which moved `W_TXT` and not this: the
/// corrected variant's winning row is `(4, 0.5, 0.5)`, MAE 0.688864, CI
/// [+0.005837, +0.041111]. The hubness correction applies to the IMAGE-side
/// term only ([`standardise`] says why), so this term was measured exactly as
/// it had been.
///
/// AND ON TRIAL in the short-direction re-test, where it turns out to be the
/// half of the pair that pays. On the current 50-key index (see
/// [`W_TXT_DEFAULT`] for why the MAE scale moved) with the `short` proxy,
/// paired row against row rather than each against the baseline — the
/// observations are the same queries and the same keys in both, so the
/// difference is paired directly, and two overlapping baseline intervals do
/// not make two indistinguishable rows:
///
/// | against text-free `(4, 0, 0)` | mean | 95 % CI | |
/// |---|---|---|---|
/// | `(4, 0.5, 0.5)` as shipped | +0.013643 | [+0.006785, +0.020881] | worse |
/// | `(4, 0.5, 0)` no description term | +0.000447 | [-0.006972, +0.007980] | not separated |
/// | `(4, 0, 0.5)` description term alone | +0.016799 | [+0.010650, +0.022835] | worse |
///
/// Positive is MORE error. The whole of the shipped pair's cost is this term:
/// removing it recovers +0.012868, CI [+0.006925, +0.018852]. The mechanism is
/// the pairing rather than the prose — a short typed direction against a
/// paragraph of generated description is text against text through a tower
/// trained for image against text, and S2's proxy hid that by making the query
/// text a description too. Row counts run 7,534 to 7,540 because a settings
/// key with no carrier among the four retrieved neighbours yields no
/// observation.
///
/// It is KEPT at 0.5, and what buys it is measured in the same run. At
/// `W_TXT = 0.5` this is the term that separates opposite directions: antonym
/// top-1 overlap 46.9 % with it against 60.7 % without, and the corpus stays
/// open either way (top share 18.3 % against 13.6 %, 167/169 against 168/169
/// ever retrieved). Predicting the held-out settings is the reference block's
/// first job and obeying the typed Direction is its second; at zero the second
/// job keeps one term where it had two. The price is 3.2 % of a pooled
/// z-scored error, and it is a number now instead of a disclosure.
pub const W_DESC_DEFAULT: f64 = 0.5;
/// Weight of the look-library image term, and the one weight the harness never
/// evaluated: the look library carries no settings, so the leave-one-out
/// settings objective cannot see it at all.
///
/// It is NOT inert. Whenever a DIRECTION is given, the two text terms rank
/// looks against each other as well — `txt` scores that direction against each
/// look's own IMAGE vector and `desc` against its description, both per look —
/// so this weight is a real ratio against them and its SCALE can change which
/// look wins. This comment used to claim the opposite and cite a test that
/// could not have caught it: that test drove `txt: 0.0, desc: 0.0` over
/// fixtures carrying neither tags nor a description, making the look term the
/// only live one and the claim a tautology.
///
/// Both regimes are now pinned for what they are:
/// `look_weight_cannot_reorder_without_a_direction` (no text — a pure scale on
/// a single term) and `look_weight_is_a_real_ratio_against_the_direction_terms`
/// (shipped weights — the order holds from 0 through 2x the shipped value on a
/// library where direction and image disagree, and first moves at 4x). 1.0 is
/// therefore an UNMEASURED value sitting in a measured stable band, not a
/// normalisation that could not matter.
pub const W_LOOK_DEFAULT: f64 = 1.0;
/// The four retrieval weights as ONE value, resolved once at the top of a run
/// and passed down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetrievalWeights {
    pub emb: f64,
    pub txt: f64,
    pub desc: f64,
    pub look: f64,
}

impl Default for RetrievalWeights {
    fn default() -> Self {
        Self::SHIPPED
    }
}

impl RetrievalWeights {
    /// What the harness shipped.
    pub const SHIPPED: Self = RetrievalWeights {
        emb: W_EMB_DEFAULT,
        txt: W_TXT_DEFAULT,
        desc: W_DESC_DEFAULT,
        look: W_LOOK_DEFAULT,
    };
    /// Every cosine term off — the 14-dim ranking, bit for bit. The look
    /// weight keeps its normalisation because a zero there would tie every
    /// look against every other rather than removing a term.
    pub const FEATURE_ONLY: Self =
        RetrievalWeights { emb: 0.0, txt: 0.0, desc: 0.0, look: W_LOOK_DEFAULT };

    /// The shipped weights with the environment's overrides applied — the ONE
    /// place this process reads those variables.
    pub fn from_env() -> Self {
        Self::resolve(crate::config::live_env)
    }

    /// [`from_env`](Self::from_env) over an explicit environment — the seam the
    /// tests drive, so no test has to mutate the process to state a rule.
    ///
    /// Non-finite and negative values fall back to the shipped weight: a
    /// negative weight would rank the LEAST similar photo first, which is not a
    /// tuning choice.
    pub fn resolve(get: impl Fn(&str) -> Option<String>) -> Self {
        let one = |name: &str, default: f64| -> f64 {
            get(name)
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v >= 0.0)
                .unwrap_or(default)
        };
        RetrievalWeights {
            emb: one(ENV_EMBED_WEIGHT, W_EMB_DEFAULT),
            txt: one(ENV_TEXT_WEIGHT, W_TXT_DEFAULT),
            desc: one(ENV_DESC_WEIGHT, W_DESC_DEFAULT),
            look: one(ENV_LOOK_WEIGHT, W_LOOK_DEFAULT),
        }
    }
}

/// Is the embedding sidecar wanted for this run? A VALUE, resolved once from
/// flag > environment > preference and then carried as an argument.
///
/// Opt-in because the first run downloads **1.50 GB** of weights and every
/// call re-loads them (~seconds), which is not a cost an index build or a
/// develop may take without being asked. An index built with it off is a
/// perfectly good v5 index — the block is simply absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddingSwitch(bool);

impl EmbeddingSwitch {
    pub const ON: Self = EmbeddingSwitch(true);
    pub const OFF: Self = EmbeddingSwitch(false);

    pub fn on(self) -> bool {
        self.0
    }

    /// flag > environment > preference, reading the process environment once.
    ///
    /// `flag` is the CLI's `--embed` / `--no-embed` (`None` when neither was
    /// given). An environment variable that is SET wins over the preference
    /// whatever its value, including `0` — that is what makes it an override.
    pub fn resolve(flag: Option<bool>, pref: bool) -> Self {
        Self::resolve_with(flag, pref, |k| {
            crate::config::live_env_os(k).map(|v| v.to_string_lossy().into_owned())
        })
    }

    /// [`resolve`](Self::resolve) over an explicit environment — the tested
    /// seam.
    pub fn resolve_with(
        flag: Option<bool>,
        pref: bool,
        get: impl Fn(&str) -> Option<String>,
    ) -> Self {
        if let Some(f) = flag {
            return EmbeddingSwitch(f);
        }
        match get(ENV_EMBED) {
            Some(v) => EmbeddingSwitch(!matches!(v.trim(), "" | "0" | "false" | "off")),
            None => EmbeddingSwitch(pref),
        }
    }
}

/// Is the LOCAL description model wanted for this build? A VALUE, resolved
/// once from flag > environment > preference and then carried as an argument —
/// the same shape as [`EmbeddingSwitch`], and for the same reason (a switch
/// implemented by writing the process environment is a shared mutable global
/// that `cargo test`'s parallel threads can reconfigure under each other).
///
/// Opt-in because the first run downloads **4.3 GB** of Qwen3-VL weights and
/// the pass costs seconds per photograph on top of the embedding. An index
/// built with it off is a perfectly good v5 index — `desc` is simply absent
/// and `desc_embed` falls back to the tag string, which is exactly what S1
/// shipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescribeSwitch(bool);

impl DescribeSwitch {
    pub const ON: Self = DescribeSwitch(true);
    pub const OFF: Self = DescribeSwitch(false);

    pub fn on(self) -> bool {
        self.0
    }

    /// flag > environment > preference, reading the process environment once.
    ///
    /// `flag` is the CLI's `--describe` (`None` when it was not given). An
    /// environment variable that is SET wins over the preference whatever its
    /// value, including `0` — that is what makes it an override.
    pub fn resolve(flag: Option<bool>, pref: bool) -> Self {
        Self::resolve_with(flag, pref, |k| {
            crate::config::live_env_os(k).map(|v| v.to_string_lossy().into_owned())
        })
    }

    /// [`resolve`](Self::resolve) over an explicit environment — the tested
    /// seam.
    pub fn resolve_with(
        flag: Option<bool>,
        pref: bool,
        get: impl Fn(&str) -> Option<String>,
    ) -> Self {
        if let Some(f) = flag {
            return DescribeSwitch(f);
        }
        match get(ENV_DESCRIBE) {
            Some(v) => DescribeSwitch(!matches!(v.trim(), "" | "0" | "false" | "off")),
            None => DescribeSwitch(pref),
        }
    }
}

/// What a stored embedding block was produced BY, in one string.
///
/// The checkpoint alone was not enough to tell two incomparable indices apart:
/// the text tower's numbers also depend on which TOKENIZER produced the ids,
/// and this batch's own root cause (F-11) was two doors on one checkpoint
/// answering vectors at cosine 0.72-0.78 of each other. An index built through
/// the other door is not comparable with this one, and now it does not look
/// identical either.
///
/// The revision is abbreviated to 12 characters after the tokenizer class
/// because it is the same pinned checkpoint revision spelled in full earlier in
/// the same string — the field says WHICH tokenizer, not a second pin.
pub fn embed_provenance_string() -> String {
    format!(
        "{}@{} tokenizer={}@{} vocab-v{}",
        crate::embed::MODEL_REPO,
        crate::embed::MODEL_REVISION,
        crate::embed::TEXT_TOKENIZER_CLASS,
        &crate::embed::MODEL_REVISION[..12],
        LOOK_VOCAB_VERSION,
    )
}

/// The `vocab-vN` stamp out of a provenance string, when it carries one.
///
/// `None` covers both "no stamp" (an index written before the field existed)
/// and "unparseable"; the loader treats those as UNKNOWN — never as a match,
/// but not as a version it can refuse on either, so only a stamp it can read
/// and disagrees with drops anything. What keeps a stamp from going missing is
/// `save`: a build with no vectors of its own carries the merged half's stamp
/// forward instead of writing `null` over it.
pub fn vocab_version_of(provenance: &str) -> Option<u32> {
    provenance
        .split_whitespace()
        .find_map(|field| field.strip_prefix("vocab-v"))
        .and_then(|n| n.parse().ok())
}

/// The MODEL half of a provenance string — everything but the `vocab-vN`
/// field — which two populations must share before one stamp can describe
/// both ([`StyleIndex::save`]'s merge).
pub fn model_of(provenance: &str) -> String {
    provenance
        .split_whitespace()
        .filter(|field| !field.starts_with("vocab-v"))
        .collect::<Vec<_>>()
        .join(" ")
}

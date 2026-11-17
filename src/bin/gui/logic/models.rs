//! The model-catalogue landing, headless: how a fetched listing is classified
//! and what the landing says. Fetching, single-flight flags and the generation
//! guard are the runtime's.
use autoshade::text::generated::*;


use super::UiText;
use std::collections::HashMap;

/// One installed catalogue, classified for the role that asked.
pub(crate) struct Catalogue {
    pub(crate) chat: Vec<String>,
    /// Only a role that generates images keeps an image list; an analysis
    /// catalogue holding ids from an image endpoint would offer the wrong
    /// picker.
    pub(crate) image: Vec<String>,
    /// What the listing SAID about each id, kept only where it said it.
    pub(crate) efforts: HashMap<String, Vec<String>>,
    pub(crate) vision: HashMap<String, bool>,
    pub(crate) total: usize,
}

/// Classify one listing for a role.
pub(crate) fn catalogue(
    models: &[autoshade::openai_models::ModelInfo],
    role_generates_images: bool,
) -> Catalogue {
    let chat: Vec<String> =
        models.iter().filter(|m| autoshade::openai_models::is_chat_model(&m.id)).map(|m| m.id.clone()).collect();
    let image: Vec<String> = if role_generates_images {
        models.iter().filter(|m| autoshade::openai_models::is_image_model(&m.id)).map(|m| m.id.clone()).collect()
    } else {
        Vec::new()
    };
    let mut efforts = HashMap::new();
    let mut vision = HashMap::new();
    for m in models {
        if let Some(tiers) = &m.effort_options {
            efforts.insert(m.id.clone(), tiers.clone());
        }
        if let Some(v) = m.vision {
            vision.insert(m.id.clone(), v);
        }
    }
    Catalogue { chat, image, efforts, vision, total: models.len() }
}

/// The line for a landing that installed a listing, was discarded as stale,
/// or failed.
pub(crate) fn installed_words(catalogue: &Catalogue) -> UiText {
    UiText::with(
        K_LOGIC_FETCHED_MODELS_CHAT_IMAGE,
        &[
            ("n", &catalogue.total.to_string()),
            ("chat", &catalogue.chat.len().to_string()),
            ("img", &catalogue.image.len().to_string()),
        ],
    )
}

pub(crate) fn discarded_words() -> UiText {
    UiText::of(K_LOGIC_MODEL_LIST_DISCARDED_SETTINGS_CHANGED_WHILE_IT_W)
}

pub(crate) fn failed_words(err: &str) -> UiText {
    UiText::with(K_LOGIC_FETCH_FAILED, &[("err", err)])
}

#[cfg(test)]
mod tests {
    use super::*;
    use autoshade::openai_models::ModelInfo;

    fn model(id: &str) -> ModelInfo {
        ModelInfo { id: id.into(), effort_options: None, vision: None }
    }

    /// The listing is split into chat and image ids; an analysis role keeps no
    /// image list, and the per-id metadata is kept only where the listing said
    /// it.
    #[test]
    fn a_listing_is_classified_for_the_role_that_asked() {
        let mut with_meta = model("gpt-5");
        with_meta.effort_options = Some(vec!["low".into(), "high".into()]);
        let mut vision = model("gpt-5-vision");
        vision.vision = Some(true);
        let models = vec![
            with_meta,
            vision,
            model("gpt-image-1"),
            model("text-embedding-3"),
        ];

        let analysis = catalogue(&models, false);
        assert_eq!(analysis.total, 4);
        assert!(analysis.chat.contains(&"gpt-5".to_string()));
        assert!(!analysis.chat.contains(&"gpt-image-1".to_string()), "an image model is not a chat model");
        assert!(analysis.image.is_empty(), "an analysis role keeps no image list");
        assert_eq!(analysis.efforts.get("gpt-5").map(Vec::len), Some(2));
        assert_eq!(analysis.vision.get("gpt-5-vision"), Some(&true));
        assert!(!analysis.vision.contains_key("gpt-5"), "nothing invented where the listing was silent");

        let image = catalogue(&models, true);
        assert_eq!(image.image, vec!["gpt-image-1".to_string()]);
        assert_eq!(image.chat, analysis.chat);
    }

    /// The three lines: installed names its counts, a discarded completion
    /// says why, and a failure carries the error.
    #[test]
    fn the_landing_words_match_the_outcome() {
        let models = vec![model("gpt-5"), model("gpt-image-1")];
        let installed = installed_words(&catalogue(&models, true));
        assert_eq!(installed.key, "fetched {n} models ({chat} chat · {img} image)");
        assert_eq!(installed.args[0].1, super::super::Arg::Text("2".into()));
        assert_eq!(installed.args[2].1, super::super::Arg::Text("1".into()));

        assert_eq!(
            discarded_words().key,
            "model list discarded — settings changed while it was being fetched"
        );
        let failed = failed_words("401");
        assert_eq!(failed.key, "fetch failed: {err}");
        assert_eq!(failed.args[0].1, super::super::Arg::Text("401".into()));
    }
}

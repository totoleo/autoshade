//! The Settings window's form: load/save the prefs it edits, the model
//! catalogue fetch and the Python-environment install/probe rows. Same
//! `AutoShadeApp` impl as `actions.rs`, split by domain.

use crate::*;

impl Commands<'_> {
    /// Populate the Settings form from the resolved config (keys are shown only as
    /// "present", never revealed). Called when the window opens.
    pub(crate) fn load_settings_form(&mut self) {
        let cfg = autoshade::config::Config::load();
        // Keep any model lists already fetched this session so reopening Settings
        // doesn't force a re-fetch — and keep an in-flight fetch's flag alive:
        // zeroing it mid-fetch stopped the repaint pump AND re-armed the fetch
        // button (duplicate requests, stalled status). The catalogues also
        // carry the per-role auto-fetch guards and generation stamps.
        let image_models = std::mem::take(&mut self.workspace.ui_mut().settings.image_models);
        let analysis_models = std::mem::take(&mut self.workspace.ui_mut().settings.analysis_models);
        // ONE read of the settings file for the two fields below that show the
        // file's own spelling rather than the resolved value.
        let local = autoshade::config::load_local_settings();
        let comfy = autoshade::comfy::Settings::load();
        self.workspace.ui_mut().settings = SettingsForm {
            analysis_provider_api: cfg.analysis_is_api(),
            image_provider_oauth: cfg.image_is_oauth(),
            analysis_model: cfg.analysis_model.clone(),
            analysis_base_url: cfg.analysis_base_url.clone(),
            analysis_api_key: String::new(),
            analysis_key_present: cfg.analysis_api_key.is_some(),
            analysis_effort: cfg.analysis_effort.clone().unwrap_or_default(),
            image_model: cfg.openai_model.clone(),
            image_base_url: cfg.openai_base_url.clone(),
            image_gen_model: cfg.openai_image_model.clone(),
            image_api_key: String::new(),
            image_key_present: cfg.openai_api_key.is_some(),
            image_effort: cfg.image_effort.clone().unwrap_or_default(),
            image_api_family: cfg.image_api_family.clone(),
            // The FILE's own spelling, not the resolved root: this is the
            // field the user edits, and showing a resolved absolute path in it
            // would silently turn "use the default" into a pinned folder on
            // the next save. `load_local_settings` (not the raw file) so an
            // ambient working-directory copy cannot pre-fill it either.
            out_dir: local.out_dir.unwrap_or_default(),
            // Same rule for the interpreter (M1-3): showing the RESOLVED
            // program here would turn "use the default" into a pinned path on
            // the next save, and `load_local_settings` is what keeps an
            // ambient working-directory copy from pre-filling either field.
            python_bin: local.python_bin.unwrap_or_default(),
            comfy_cli: comfy.cli,
            comfy_workspace: comfy.workspace,
            comfy_base_dir: comfy.base_dir,
            comfy_reimagine_workflow: comfy.reimagine_workflow,
            comfy_fill_workflow: comfy.fill_workflow,
            status: String::new(),
            image_models,
            analysis_models,
        };
        // Availability is credential-dependent: another surface (the web
        // Settings) may have replaced a saved key since these lists were
        // fetched, and the URL-keyed self-invalidation in `settings_ui`
        // cannot see that. Compare each kept catalogue's credential stamp
        // against today's key and drop a stale list here, at the one moment
        // the resolved config is already in hand.
        for role in [ModelRole::Image, ModelRole::Analysis] {
            let fp = key_fingerprint(match role {
                ModelRole::Image => cfg.openai_api_key.as_deref().unwrap_or(""),
                ModelRole::Analysis => cfg.analysis_api_key.as_deref().unwrap_or(""),
            });
            let cat = self.workspace.catalogue_mut(role);
            if !cat.is_empty() && cat.from_key != fp {
                cat.clear();
            }
        }
        self.autofetch_models_once(&cfg);
    }

    /// First time Settings opens in a session, fill the pick-lists without
    /// making the user find the button — "which models can I use here" is the
    /// question the panel exists to answer, and a blank dropdown next to a
    /// configured key reads as "none available".
    ///
    /// Once per session PER ROLE, and only for a role that already has a key:
    /// a probe with no credential can only fail, and firing on every open
    /// would put a network call behind "I came here to switch the language".
    ///
    /// The guard is consumed at DISPATCH (`fetch_models` marks the role), not
    /// by the visit itself: one global open-consumed boolean meant a Settings
    /// visit before any key existed spent the whole session's opportunity, so
    /// the role that became eligible five minutes later — key saved, provider
    /// flipped to `api` — was never probed on any later open.
    pub(crate) fn autofetch_models_once(&mut self, cfg: &autoshade::config::Config) {
        if cfg.openai_api_key.is_some() && !self.workspace.ui().settings.image_models.autofetched {
            self.fetch_models(ModelRole::Image);
        }
        // The analysis endpoint only has a catalogue to fetch in `api` mode —
        // the OAuth verifier is the `claude` CLI, which serves no /models.
        if cfg.analysis_is_api()
            && cfg.analysis_api_key.is_some()
            && !self.workspace.ui().settings.analysis_models.autofetched
        {
            self.fetch_models(ModelRole::Analysis);
        }
    }

    /// Persist the Settings form to autoshade.local.json (gitignored). A blank key
    /// keeps the stored one. The next Analyze/Export reloads Config, so it applies.
    pub(crate) fn save_settings_form(&mut self) {
        // The whole load-merge-save runs under the cross-process settings
        // lock (`config::update_local_settings`): this panel and the serve
        // process merge onto the same file, and the old unlocked cycle here
        // let a save that landed between its load and its rename be erased.
        let form = &self.workspace.ui().settings;
        let saved = autoshade::config::update_local_settings(|cur| {
            cur.analysis_provider =
                Some(if form.analysis_provider_api { "api" } else { "oauth" }.to_string());
            cur.image_provider =
                Some(if form.image_provider_oauth { "oauth" } else { "api" }.to_string());
            cur.analysis_model = Some(form.analysis_model.trim().to_string());
            cur.analysis_base_url = Some(form.analysis_base_url.trim().to_string());
            cur.image_model = Some(form.image_model.trim().to_string());
            cur.image_base_url = Some(form.image_base_url.trim().to_string());
            cur.image_gen_model = Some(form.image_gen_model.trim().to_string());
            // Effort: an empty field is a real choice — "let the provider
            // decide" — so it is stored as empty rather than skipped, or
            // clearing the field could never take effect.
            cur.analysis_effort = Some(form.analysis_effort.trim().to_string());
            cur.image_effort = Some(form.image_effort.trim().to_string());
            cur.image_api_family = Some(form.image_api_family.trim().to_string());
            // Stored blank for OpenAI's call, so the choice can undo a template.
            // The delivery root (M8), same explicit-blank rule as the two
            // efforts: an emptied field is a real choice ("the default
            // ./out"), so it is STORED empty rather than skipped, or clearing
            // it could never take effect. `update_local_settings` drops the
            // memo, so the next export claim reads this value.
            cur.out_dir = Some(form.out_dir.trim().to_string());
            // The interpreter (M1-3), same explicit-blank rule: an emptied
            // field is the real choice "use the platform default", and only a
            // STORED blank can undo a path saved earlier.
            cur.python_bin = Some(form.python_bin.trim().to_string());
            // Secrets: only overwrite when a non-empty value was actually
            // typed — and a typed key is FOR the endpoint on screen beside
            // it, so record that home (`config::file_key_for` enforces it at
            // load: a later provider flip or base edit cannot re-route it).
            let ak = form.analysis_api_key.trim().to_string();
            let ik = form.image_api_key.trim().to_string();
            if !ak.is_empty() {
                cur.analysis_api_key = Some(ak.clone());
                cur.analysis_api_key_base =
                    cur.analysis_base_url.clone().filter(|s| !s.trim().is_empty());
            }
            if !ik.is_empty() {
                cur.image_api_key = Some(ik.clone());
                cur.image_api_key_base =
                    cur.image_base_url.clone().filter(|s| !s.trim().is_empty());
            }
            // ONE DeepSeek key for both roles. With the DeepSeek preset both
            // endpoints are api.deepseek.com, so a key typed in either API Key
            // field fills the other role when that role has no key of its own
            // (a key saved for another endpoint is refused at load by its
            // recorded home, so it does not block this).
            if let Some((key, base)) = autoshade::config::fill_deepseek_key(
                cur.analysis_base_url.as_deref(),
                cur.image_base_url.as_deref(),
                &ik,
                cur.analysis_api_key.as_deref().is_some_and(|s| !s.is_empty()),
            ) {
                cur.analysis_api_key = Some(key);
                cur.analysis_api_key_base = base;
            }
            if let Some((key, base)) = autoshade::config::fill_deepseek_key(
                cur.image_base_url.as_deref(),
                cur.analysis_base_url.as_deref(),
                &ak,
                cur.image_api_key.as_deref().is_some_and(|s| !s.is_empty()),
            ) {
                cur.image_api_key = Some(key);
                cur.image_api_key_base = base;
            }
        });
        // ComfyUI settings live in their own file (comfy.local.json): the
        // generative backend has its own surface, so a failed/absent Comfy
        // install must not block saving the AI settings above.
        {
            let mut comfy = autoshade::comfy::Settings::load();
            comfy.cli = self.workspace.ui().settings.comfy_cli.trim().to_string();
            comfy.workspace = self.workspace.ui().settings.comfy_workspace.trim().to_string();
            comfy.base_dir = self.workspace.ui().settings.comfy_base_dir.trim().to_string();
            comfy.reimagine_workflow = self.workspace.ui().settings.comfy_reimagine_workflow.trim().to_string();
            comfy.fill_workflow = self.workspace.ui().settings.comfy_fill_workflow.trim().to_string();
            if let Err(e) = comfy.save() {
                self.workspace.ui_mut().settings.status = trf(
                    self.lang,
                    K_ACTIONS_SETTINGS_SAVED_BUT_THE_COMFYUI_PATHS_FAILED_TO_S,
                    &[("err", &e.to_string())],
                );
            }
        }
        // Did the user type a key on this save? Asked BEFORE the fields are
        // cleared, so the refusal check below can tell "typed and rejected"
        // from "left blank on purpose".
        let typed_image = !self.workspace.ui().settings.image_api_key.trim().is_empty();
        let typed_analysis = !self.workspace.ui().settings.analysis_api_key.trim().is_empty();
        match saved {
            Ok(p) => {
                self.workspace.ui_mut().settings.analysis_api_key.clear();
                self.workspace.ui_mut().settings.image_api_key.clear();
                // Presence reflects the RESOLVED config (file merged with env) —
                // deriving it from the file alone told a user whose key lives in
                // OPENAI_API_KEY that no key was set right after saving.
                let cfg = autoshade::config::Config::load();
                self.workspace.ui_mut().settings.analysis_key_present = cfg.analysis_api_key.is_some();
                self.workspace.ui_mut().settings.image_key_present = cfg.openai_api_key.is_some();
                // Normal's sidebar says whether its looks use the AI.
                self.workspace.normal_mut().key_present = Some(cfg.openai_api_key.is_some());
                // A key that cannot appear in an HTTP header is REFUSED by
                // Config::load (`header_safe_key` — a newline or space from a
                // paste is the usual cause). The refusal only prints to
                // stderr, which the windowed GUI does not show, so "saved"
                // beside a still-empty key state was the whole story the user
                // got. Say it here instead.
                let refused = (typed_image && !self.workspace.ui().settings.image_key_present)
                    || (typed_analysis && !self.workspace.ui().settings.analysis_key_present);
                // The file's HOME decides what `Config::load` reads back:
                // under the shared temp root (no per-user data directory on
                // this machine) it is not trusted with a key or an endpoint,
                // and only the models survive the merge. Said HERE — the
                // loader's warning goes to stderr, which the windowed GUI
                // never shows, so "saved" was the whole story until 2026-09-24.
                let shared = autoshade::config::load_local_settings_from().1
                    == autoshade::config::SettingsOrigin::SharedRoot;
                self.workspace.ui_mut().settings.status = if refused {
                    tr(self.lang, K_ACTIONS_SAVED_BUT_THE_KEY_WAS_NOT_ACCEPTED_IT_CONTAINS_C).into()
                } else if shared {
                    trf(self.lang, K_ACTIONS_SAVED_BUT_THIS_FILE_SITS_IN_THE_SHARED_TEMP_FOLD, &[("path", &p.display().to_string())])
                } else {
                    trf(self.lang, K_ACTIONS_SAVED, &[("path", &p.display().to_string())])
                };
                self.workspace.set_status(tr(self.lang, K_ACTIONS_SETTINGS_SAVED_APPLIES_TO_THE_NEXT_AI_CALL_ANALY).into());
                // The interpreter field may have just changed: measure it now
                // and let the note ride the same line (a wrong one would fail
                // every sidecar later with a different error).
                if let Some(note) = self.refresh_python_check() {
                    self.workspace.status_mut().push_str(" — ");
                    self.workspace.status_mut().push_str(&note);
                }
            }
            Err(e) => {
                self.workspace.ui_mut().settings.status =
                    trf(self.lang, K_ACTIONS_SAVE_FAILED, &[("err", &e.to_string())])
            }
        }
    }

    /// Fetch one role's model ids (`GET /models`) on a worker thread and fill that
    /// role's pick-lists. Uses the key/base typed in the form if present, else the
    /// saved config — so it works whether or not the user has saved a key yet.
    ///
    /// The base and key resolve HERE, on the UI thread, so the catalogue's
    /// `from_base`/`from_key` stamps describe exactly what the worker sends
    /// (the worker used to re-load Config on its own thread, so the stamp and
    /// the request could disagree about which credential was used).
    pub(crate) fn fetch_models(&mut self, role: ModelRole) {
        let (form_key, form_base) = match role {
            ModelRole::Image => (
                self.workspace.ui().settings.image_api_key.trim().to_string(),
                self.workspace.ui().settings.image_base_url.trim().to_string(),
            ),
            ModelRole::Analysis => (
                self.workspace.ui().settings.analysis_api_key.trim().to_string(),
                self.workspace.ui().settings.analysis_base_url.trim().to_string(),
            ),
        };
        let cfg = autoshade::config::Config::load();
        let (cfg_base, cfg_key) = match role {
            ModelRole::Image => (cfg.openai_base_url, cfg.openai_api_key),
            ModelRole::Analysis => (cfg.analysis_base_url, cfg.analysis_api_key),
        };
        let base = if form_base.is_empty() { cfg_base.clone() } else { form_base };
        // The SAVED key is only ever sent to the endpoint it is saved beside —
        // `config::file_key_for` is this same rule at load time. Probing a
        // freshly TYPED endpoint takes a typed (or first saved) credential;
        // silently reusing the old endpoint's key against the new URL is the
        // provider-flip misroute in miniature. With no usable key the probe
        // fails immediately with "no API key — set one in Settings", which is
        // the accurate instruction.
        let key = if !form_key.is_empty() {
            form_key
        } else if autoshade::config::same_endpoint(&base, &cfg_base) {
            cfg_key.unwrap_or_default()
        } else {
            String::new()
        };
        let cat = self.workspace.catalogue_mut(role);
        if cat.fetching {
            return;
        }
        cat.fetching = true;
        // Any dispatch — auto or manual — spends this role's convenience probe.
        cat.autofetched = true;
        // Drop the previous endpoint's lists NOW (bumping `gen`, so an older
        // in-flight completion can no longer land) and stamp what this fetch
        // targets: a failed fetch then leaves empty lists (grounded fallbacks)
        // rather than ids from the old server under the new URL's name.
        cat.clear();
        cat.from_base = base.clone();
        cat.from_key = key_fingerprint(&key);
        let generation = cat.generation;
        self.workspace.ui_mut().settings.status = tr(self.lang, K_ACTIONS_FETCHING_MODELS).into();
        // spawn_worker's catch_unwind guarantees the UI's `fetching` flag
        // always clears — a panic still delivers Msg::Models(role, gen, Err)
        // (this site used to hand-roll a Drop guard for exactly that; the
        // helper now covers every worker uniformly).
        self.services.spawn_worker(
            move || Msg::Models(role, generation, autoshade::openai_models::list_models(&base, &key)),
            move |e| Msg::Models(role, generation, Err(e)),
        );
    }


    /// 「准备 Python 环境」: create the managed environment with `uv` and
    /// install the sidecar requirements into it. Explicit, because the wheel
    /// set is a multi-gigabyte download; the app must not start one on its own.
    pub(crate) fn start_python_env_install(&mut self) {
        if self.workspace.busy() {
            return;
        }
        let lang = self.lang;
        let plan = match autoshade::python_env::Plan::resolve() {
            Ok(p) => p,
            Err(e) => {
                let t = format!("{e:#}");
                self.workspace.set_status(t.clone());
                self.workspace.toast(ToastKind::Error, t);
                return;
            }
        };
        self.workspace.set_busy(true);
        self.workspace.ui_mut().python_env_line = None;
        *self.workspace.status_mut() = tr(
            lang,
            K_ACTIONS_PREPARING_THE_PYTHON_ENVIRONMENT_UV_THE_DOWNLOAD,
        )
        .into();
        let tx = self.services.tx.clone();
        self.services.spawn_worker(
            move || {
                // uv writes pip's progress to stderr; each line lands in the
                // Settings row, so a long download says what it is doing.
                let mut send = |line: &str| {
                    let _ = tx.send(Msg::PythonEnvLine(line.to_string()));
                };
                Msg::PythonEnvReady(plan.install(&mut send).map(|p| p.display().to_string()))
            },
            |e| Msg::PythonEnvReady(Err(e)),
        );
    }

    /// The install's landing: the interpreter it produced, or the failure.
    pub(crate) fn on_python_env_ready(&mut self, lang: Lang, res: anyhow::Result<String>) {
        self.workspace.set_busy(false);
        match res {
            Ok(path) => {
                self.workspace.ui_mut().python_env_line = Some(path.clone());
                self.workspace.set_status(trf(lang, K_ACTIONS_PYTHON_ENVIRONMENT_READY, &[("path", &path)]));
                // The environment is ready now: re-measure so the Settings row
                // and any later note describe the new interpreter.
                self.refresh_python_check();
            }
            Err(e) => {
                let t = format!("{e:#}");
                self.workspace.ui_mut().python_env_line = Some(t.clone());
                self.workspace.set_status(t.clone());
                self.workspace.toast(ToastKind::Error, t);
            }
        }
    }

    /// Measure what the AI sidecars would run (`python_env::inspect`) and keep
    /// the fact on the app; returns the line a launch has to say about it, when
    /// it has to say one.
    pub(crate) fn refresh_python_check(&mut self) -> Option<String> {
        let check = autoshade::python_env::inspect(&autoshade::config::Config::load());
        let words = crate::panels::settings::python_startup_words(self.lang, &check);
        self.workspace.ui_mut().python_check = Some(check);
        words
    }
}

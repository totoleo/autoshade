//! Settings panel and its form load/save.

use crate::*;


impl PanelView<'_, '_> {
    pub(crate) fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let mut do_save = false;
        let mut fetch: Option<ModelRole> = None;
        // `lang` is a Copy snapshot so `tr`/`trf` never borrow `self` — the
        // `let f = &mut self.workspace.ui().settings` block below holds a partial borrow of self.
        let lang = self.lang;
        ui.label(
            // "& Theme", not "& Reverse-fit": the reverse-fit switch this
            // sentence used to point at now lives in the AI panel (R22 #4), and
            // Theme is the other control here that applies without a save.
            Text::new(tr(
                lang,
                K_SETTINGS_LANGUAGE_THEME_APPLY_IMMEDIATELY_THE_PROVIDER_SE,
            ))
            .weak()
            .small(),
        );
        ui.rule();
        ui.heading(tr(lang, K_SETTINGS_LANGUAGE));
        // English is the skeleton; Chinese is an overlay. Switching takes effect
        // next frame (every label re-reads `self.lang`), no restart/save needed.
        // `from_id_salt` is egui 0.29's name for the old `from_id_source`.
        ui.menu("lang_picker", self.lang.label(), None, |ui| {
                ui.selectable_value(&mut self.lang, Lang::En, Lang::En.label());
                ui.selectable_value(&mut self.lang, Lang::Zh, Lang::Zh.label());
            });
        ui.rule();
        ui.heading(tr(lang, K_SETTINGS_THEME));
        // Four surface schemes and an accent picked apart from them (see
        // ThemeColors) — switching either re-installs the egui style this
        // frame and persists with the other prefs.
        let before = (self.workspace.ui().theme, self.workspace.ui().accent);
        ui.menu("theme_picker", tr(lang, self.workspace.ui().theme.label()), None, |ui| {
                for t in ThemePref::ALL {
                    ui.selectable_value(&mut self.workspace.ui_mut().theme, t, tr(lang, t.label()));
                }
            });
        ui.row(|ui| {
            ui.label(tr(lang, K_SETTINGS_ACCENT_COLOUR));
            for a in AccentPref::ALL {
                if accent_swatch(ui, a.hue(), self.workspace.ui().accent == a).on_hover_text(tr(lang, a.label())).clicked() {
                    self.workspace.ui_mut().accent = a;
                }
            }
        });
        if (self.workspace.ui().theme, self.workspace.ui().accent) != before {
            install_look(ui.ctx(), self.workspace.ui().theme, self.workspace.ui().accent);
        }
        // 「Zoned fit (sky)」 MOVED to the AI panel's reverse-fit sub-area (R22
        // #4): it is a setting for ONE button, and it sat two panels away from
        // that button — you changed it here and then went hunting for 「🎛
        // Reverse-fit」. The `zoned_fit` FIELD is untouched (same Prefs key,
        // same persistence, same `start_fit` read); only its UI row moved.
        ui.rule();
        // The develop store is otherwise invisible (hashed AppData folders) —
        // this is the one place that names it and rescues pre-store saves.
        ui.heading(tr(lang, K_SETTINGS_DEVELOP_STORE));
        // THIS photo's folder, not just the root (R22-8): the root is one line
        // above a hash-named subdirectory the user then has to identify by
        // guessing, and "where is my XMP" is the question this row exists to
        // answer. Falls back to the root when no photo is open, which is all
        // there is to say then.
        let shown = match self.editor.photo().src_path.as_deref() {
            Some(p) => autoshade::store::develop_dir(p),
            None => autoshade::store::store_root(),
        };
        ui.label(Text::new(abs_display(&shown)).small().weak()).on_hover_text(tr(
            lang,
            K_SETTINGS_WHERE_SAVED_DEVELOPS_LIVE_RECIPES_LIGHTROOM_XMP,
        ));
        // Enabled only when the folder EXISTS: an unsaved photo has no develop
        // directory yet, and a file manager pointed at a missing path silently
        // opens somewhere else entirely (Explorer lands in Documents). One stat
        // per frame, but only while this window is open — a cached answer would
        // be wrong for exactly the case that matters (the user saves, then
        // reaches for this button). `develop_dir` above costs nothing repeated:
        // its identity resolution is memoized for the process (store::
        // identity_of), and `store_root` was already read here every frame.
        let exists = shown.is_dir();
        if action(ui, exists, tr(lang, K_SETTINGS_SHOW_IN_FILE_MANAGER))
            .on_hover_text(if exists {
                tr(lang, K_SETTINGS_OPEN_THIS_FOLDER_IN_YOUR_FILE_MANAGER)
            } else {
                tr(lang, K_SETTINGS_NOTHING_SAVED_FOR_THIS_PHOTO_YET_THE_FOLDER_APPE)
            })
            .clicked()
            && let Err(e) = reveal_folder(&shown)
        {
            let t = trf(lang, K_SETTINGS_COULD_NOT_OPEN_THE_FOLDER, &[("err", &e.to_string())]);
            self.workspace.set_status(t.clone());
            self.workspace.toast(ToastKind::Error, t);
        }
        // The pixel masters (2026-10-18): the AI denoise / stack / heal /
        // clone / fill results cards are built FROM. They left the delivery
        // folder so cleaning exports cannot break a card, which also hid
        // them — this row names the folder and opens it.
        let masters = autoshade::store::masters_dir();
        ui.label(
            Text::new(trf(lang, K_SETTINGS_PIXEL_MASTERS, &[("path", &abs_display(&masters))]))
                .small()
                .weak(),
        )
        .on_hover_text(tr(
            lang,
            K_SETTINGS_THE_FULL_RESOLUTION_IMAGES_AI_DENOISE_STACKING_H,
        ));
        let has_masters = masters.is_dir();
        if action(ui, has_masters, tr(lang, K_SETTINGS_SHOW_MASTERS_FOLDER))
            .on_hover_text(if has_masters {
                tr(lang, K_SETTINGS_OPEN_THIS_FOLDER_IN_YOUR_FILE_MANAGER)
            } else {
                tr(lang, K_SETTINGS_NO_MASTER_YET_THE_FOLDER_APPEARS_WITH_THE_FIRST)
            })
            .clicked()
            && let Err(e) = reveal_folder(&masters)
        {
            let t = trf(lang, K_SETTINGS_COULD_NOT_OPEN_THE_FOLDER, &[("err", &e.to_string())]);
            self.workspace.set_status(t.clone());
            self.workspace.toast(ToastKind::Error, t);
        }
        if action(ui, true, tr(lang, K_SETTINGS_IMPORT_DEVELOPS_FROM_AN_OLD_OUT_FOLDER))
            .on_hover_text(tr(
                lang,
                K_SETTINGS_SAVES_MADE_BEFORE_V0_13_LIVED_IN_A_OUT_FOLDER_NE,
            ))
            .clicked()
        {
            self.act_all(vec![logic::Action::Settings(
                logic::settings::SettingsAction::ImportLegacyFolder,
            )]);
        }
        // --- the DELIVERY ROOT (R24-5 M8) ------------------------------------
        // The counterpart to the develop store above: that folder holds the
        // recipes, this one holds the finished files. It used to be the
        // hardcoded cwd-relative `./out` in five places at once (the CLI, the
        // batch renderer, the web download route, the GUI's destination
        // setting, the style-prompt writer), which is why "where did my export
        // go" had no answer but "wherever you launched the app from".
        ui.rule();
        ui.heading(tr(lang, K_SETTINGS_DELIVERY_FOLDER));
        let resolved = autoshade::config::delivery_root();
        let mut browse_delivery = false;
        {
            let f = &mut self.workspace.ui_mut().settings;
            ui.row(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut f.out_dir)
                        .desired_width(FIELD_W_MAX.min(ui.avail_width() - 90.0).max(80.0))
                        .hint_text(autoshade::config::DEFAULT_DELIVERY_ROOT),
                )
                .on_hover_text(tr(
                    lang,
                    K_SETTINGS_WHERE_FINISHED_FILES_LAND_EXPORTS_AND_THE_EXTRAC,
                ));
                if action(ui, true, tr(lang, K_SETTINGS_BROWSE))
                    .on_hover_text(tr(lang, K_SETTINGS_PICK_THE_DELIVERY_FOLDER))
                    .clicked()
                {
                    browse_delivery = true;
                }
            });
        }
        if browse_delivery {
            self.act_all(vec![logic::Action::Settings(
                logic::settings::SettingsAction::PickDeliveryFolder,
            )]);
        }
        // The RESOLVED root, absolute: a relative setting (the default
        // included) means nothing without saying which directory it is
        // relative to — the same reason the export summary spells its target
        // out in full. Reflects the SAVED value, so it only moves after
        // 「Save settings」.
        ui.label(Text::new(abs_display(&resolved)).small().weak());
        // --- the PYTHON INTERPRETER (M1-3) -----------------------------------
        // WRITABLE here, unlike the sidecar path below, and that is not the
        // same ruling softened. `AUTOSHADE_PYTHON` stays `Trust::Destination`
        // — no `.env`, no working-directory `autoshade.local.json` beside
        // someone's photos may supply it — and this field writes ONLY the
        // trusted per-user file, which is the authority the environment
        // variable already had. What changed is REACHABILITY: a
        // Finder-launched `.app` inherits launchd's environment, not a
        // shell's, so on macOS an env-only setting is one no user of the
        // shipped app can set at all.
        //
        // The button probes interpreter NAMES through `PATH`
        // (`config::PYTHON_CANDIDATES`) — never a fixed machine path, never a
        // file dialog — so a value the user did not type by hand is one the
        // OS itself resolved.
        ui.rule();
        // ComfyUI (S5.11): the local generative backend's paths. Blank fields
        // derive the defaults (CLI from PATH, workspace/models from the Comfy
        // Desktop registry, output under the app store) — the value shown is
        // the resolved one, and it is what the next save pins.
        ui.heading(tr(lang, K_SETTINGS_COMFYUI_LOCAL_GENERATION));
        {
            let f = &mut self.workspace.ui_mut().settings;
            let width = |ui: &egui::Ui| FIELD_W_MAX.min(ui.avail_width() - 140.0).max(80.0);
            for (label, field) in [
                ("ComfyUI CLI", &mut f.comfy_cli),
                ("ComfyUI folder", &mut f.comfy_workspace),
                ("Models folder", &mut f.comfy_base_dir),
                ("Reimagine workflow", &mut f.comfy_reimagine_workflow),
                ("Fill workflow", &mut f.comfy_fill_workflow),
            ] {
                ui.row(|ui| {
                    ui.label(tr(lang, label));
                    ui.add(egui::TextEdit::singleline(field).desired_width(width(ui)));
                });
            }
            ui.label(Text::new(tr(lang, K_SETTINGS_BLANK_DEFAULT)).small().weak());
        }
        // One resolved config for this row and the sidecar row below; the
        // panel used to load it once per row.
        let cfg_now = autoshade::config::Config::load();
        ui.heading(tr(lang, K_SETTINGS_PYTHON_INTERPRETER));
        {
            let f = &mut self.workspace.ui_mut().settings;
            ui.row(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut f.python_bin)
                        .desired_width(FIELD_W_MAX.min(ui.avail_width() - 90.0).max(80.0))
                        .hint_text(autoshade::config::default_python_bin()),
                )
                .on_hover_text(tr(
                    lang,
                    K_SETTINGS_WHICH_PYTHON_RUNS_THE_AI_SIDECARS_SEGMENTATION_D,
                ));
                if action(ui, true, tr(lang, K_SETTINGS_DETECT))
                    .on_hover_text(tr(
                        lang,
                        K_SETTINGS_LOOK_IN_THE_STANDARD_INSTALL_LOCATIONS_FOR_A_WOR,
                    ))
                    .clicked()
                {
                    // `probe_python_bin` RUNS each candidate (`--version`)
                    // before offering it: on a Mac without developer tools
                    // the system `python3` exists and does not work, so an
                    // existence check alone would fill this field with a stub
                    // whose only behaviour is an install prompt.
                    match autoshade::config::probe_python_bin() {
                        Some(bin) => {
                            f.status = trf(lang, K_SETTINGS_FOUND, &[("bin", &bin)]);
                            f.python_bin = bin;
                        }
                        // A real answer, said as one: "we looked in the
                        // standard places and found nothing" is actionable,
                        // a silently unchanged field is a button that did
                        // nothing.
                        None => {
                            f.status = tr(
                                lang,
                                K_SETTINGS_NO_PYTHON_FOUND_IN_THE_STANDARD_INSTALL_LOCATION,
                            )
                            .into();
                        }
                    }
                }
            });
        }
        // What the app will ACTUALLY run. The field above holds the FILE's
        // spelling and an `AUTOSHADE_PYTHON` in the environment outranks it,
        // so a user whose environment already answers would otherwise be
        // looking at an empty box.
        ui.label(Text::new(cfg_now.sidecar_python()).small().weak());
        // The managed environment: an app-owned interpreter carrying the
        // sidecars' wheels. Prepared only on request — the CUDA set is a
        // multi-gigabyte download, so it is never fetched in the background.
        ui.row(|ui| {
            ui.label(tr(lang, K_SETTINGS_MANAGED_PYTHON));
            match autoshade::python_env::readiness() {
                autoshade::python_env::Readiness::Ready(p) => {
                    ui.label(Text::new(p.display().to_string()).small())
                        .on_hover_text(tr(
                            lang,
                            K_SETTINGS_THE_AI_SIDECARS_RUN_IN_THIS_ENVIRONMENT_UNLESS_Y,
                        ));
                }
                autoshade::python_env::Readiness::NotPrepared => {
                    ui.label(Text::new(tr(lang, K_SETTINGS_NOT_PREPARED)).small().weak());
                }
            }
        });
        if action(ui, !self.workspace.busy(), tr(lang, K_SETTINGS_PREPARE_PYTHON_ENVIRONMENT))
            .on_hover_text(tr(
                lang,
                K_SETTINGS_RUNS_UV_TO_CREATE_A_PRIVATE_ENVIRONMENT_AND_INST,
            ))
            .clicked()
        {
            self.act_all(vec![logic::Action::Settings(
                logic::settings::SettingsAction::InstallPython,
            )]);
        }
        if let Some(line) = &self.workspace.ui().python_env_line {
            ui.label(Text::new(line).small().weak());
        }
        // What the sidecars will actually launch — the launch check's answer,
        // which is the only place a wrong interpreter becomes visible before a
        // sidecar fails.
        if let Some(check) = &self.workspace.ui().python_check {
            use autoshade::python_env::PythonState;
            let fact = match &check.state {
                PythonState::Managed(_) => None,
                PythonState::Interpreter => Some(trf(
                    lang,
                    K_SETTINGS_AI_SIDECARS_USE_THE_INTERPRETER_NAMED_ABOVE,
                    &[("bin", &check.bin)],
                )),
                PythonState::UnpreparedDefault => Some(trf(
                    lang,
                    K_SETTINGS_AI_SIDECARS_CURRENTLY_USE_THE_MANAGED_ENVIRONMEN,
                    &[("bin", &check.bin)],
                )),
                PythonState::Unusable => Some(trf(
                    lang,
                    K_SETTINGS_AI_SIDECARS_NEED_PYTHON_3_IS_NOT_ONE,
                    &[("bin", &check.bin)],
                )),
            };
            if let Some(fact) = fact {
                ui.label(Text::new(fact).small().weak());
            }
        }
        // --- the SEGMENTATION SIDECAR path (R25, closing R22-1) --------------
        // R22 left "a settings row for the sidecar path" to R23; R23 and R24
        // did not do it, and this is why. `AUTOSHADE_SEGMENT_SCRIPT` names a
        // file that goes straight into `Command::new`, so `config.rs`
        // registers it `env_only(..., Trust::Destination)`: neither a `.env`
        // nor an ambient `autoshade.local.json` beside someone's photos may
        // supply it. Adding a picker here would write it into the trusted
        // settings file and hand every later launch a program chosen in a
        // dialog — the opposite of that ruling. The interpreter row ABOVE is
        // not a counter-example to it: that one offers a fixed candidate list
        // rather than a dialog, and it exists because a Finder-launched app
        // has no environment to read it from. This path is different on the
        // fact that decides it — it already has a working default inside the
        // app's own tree, so nobody needs a picker to make the app run.
        //
        // So this is READ-ONLY on purpose, and it says why. The two facts a
        // user actually needs are which file is resolved and whether it is
        // there; the existing 「did not ship the python sidecar」 line (the one
        // the two 🤖 buttons already show) is the answer for the missing case,
        // so the missing arm is not a second wording of the same thing.
        ui.rule();
        ui.heading(tr(lang, K_SETTINGS_SEGMENTATION_HELPER));
        let seg = cfg_now.segment_script;
        let seg_path = std::path::PathBuf::from(&seg);
        ui.label(Text::new(abs_display(&seg_path)).small().weak()).on_hover_text(tr(
            lang,
            K_SETTINGS_SET_WITH_THE_AUTOSHADE_SEGMENT_SCRIPT_ENVIRONMEN,
        ));
        // `segment_helper_available` is the SAME predicate the two AI-select
        // buttons gate on (one `exists()` per process), so this row and those
        // buttons can never disagree about whether the helper is there.
        if !segment_helper_available() {
            ui.label(
                Text::new(tr(lang,
                    K_DEVELOP_THIS_FEATURE_NEEDS_AUTOSHADE_S_PYTHON_HELPER_WHI,
                ))
                .small()
                .weak(),
            );
        }
        {
            let f = &mut self.workspace.ui_mut().settings;
            // Fetched ids belong to the endpoint recorded at fetch time; once
            // that role's URL stops matching (typed edit, provider auto-swap),
            // they describe a DIFFERENT server — self-invalidate so the
            // pickers fall back to grounded defaults, not a stale menu.
            if !f.image_models.is_empty() && !same_base(&f.image_base_url, &f.image_models.from_base)
            {
                f.image_models.clear();
            }
            if !f.analysis_models.is_empty()
                && !same_base(&f.analysis_base_url, &f.analysis_models.from_base)
            {
                f.analysis_models.clear();
            }
            ui.rule();
            ui.heading(tr(lang, K_SETTINGS_ANALYSIS_MODEL_USED_BY_AI_ANALYZE_AI_ADJUST_AND));
            ui.row(|ui| {
                ui.label(tr(lang, K_SETTINGS_PROVIDER));
                let r1 = ui.radio_value(&mut f.analysis_provider_api, false, tr(lang, K_SETTINGS_OAUTH_CLAUDE_CLI));
                let r2 = ui.radio_value(&mut f.analysis_provider_api, true, tr(lang, K_SETTINGS_API_OPENAI_COMPATIBLE));
                if r1.changed() || r2.changed() {
                    // Without this the OTHER provider's model id stays in the
                    // field and the picker presents it as the current choice
                    // (a claude alias sent to an OpenAI endpoint, or vice
                    // versa). Swap to this provider's default on a flip —
                    // but ONLY what provably belongs to the other provider:
                    // see `analysis_model_on_flip` for what a name can and
                    // cannot decide here.
                    if let Some(m) =
                        analysis_model_on_flip(&f.analysis_model, f.analysis_provider_api)
                    {
                        f.analysis_model = m.into();
                    }
                }
            });
            // DeepSeek is OpenAI-compatible: one click fills the endpoint and
            // the model id, and the harness's fresh AI Analyze reads the key
            // entered below (analysis_api_key, falling back from the image
            // role's key). Without this the panel read as Claude-vs-OpenAI
            // only, which is not what this user has.
            ui.row(|ui| {
                let label = tr(lang, K_SETTINGS_USE_DEEPSEEK);
                if action(ui, true, label)
                    .on_hover_text(tr(
                        lang,
                        K_SETTINGS_SETS_BOTH_ROLES_PROPOSER_AND_VERIFIER_TO_DEEPSEE,
                    ))
                    .clicked()
                {
                    f.analysis_provider_api = true;
                    f.analysis_base_url = autoshade::config::DEEPSEEK_BASE.to_string();
                    f.analysis_model = autoshade::config::DEEPSEEK_MODEL.to_string();
                    f.analysis_models.clear();
                    f.image_provider_oauth = false;
                    f.image_base_url = autoshade::config::DEEPSEEK_BASE.to_string();
                    f.image_model = autoshade::config::DEEPSEEK_MODEL.to_string();
                    f.image_models.clear();
                    // DeepSeek refuses OpenAI's response_format.json_schema;
                    // the chat family is the plain chat-completions path the
                    // legacy proposer/verifier already implement.
                    f.image_api_family = autoshade::config::DEEPSEEK_FAMILY.to_string();
                }
            });
            ui.row(|ui| {
                ui.label(tr(lang, K_SETTINGS_MODEL));
                // OAuth uses Claude CLI aliases; API uses the ids fetched from
                // the ANALYSIS endpoint — its own probe, not the image role's.
                // Borrowing the image list meant a separate analysis endpoint
                // offered nothing but two hardcoded ids.
                let opts = if f.analysis_provider_api {
                    model_opts(&f.analysis_models.chat, &["gpt-5.5", "gpt-4o"], &f.analysis_model)
                } else {
                    model_opts(&[], &CLAUDE_ALIASES, &f.analysis_model)
                };
                model_picker(ui, "set_analysis_model", &mut f.analysis_model, &opts, lang);
            });
            ui.row(|ui| {
                ui.label(tr(lang, K_SETTINGS_REASONING_EFFORT));
                // The CLI and the API endpoints accept different tier names —
                // offer the right list, and keep the free-text field for
                // anything a given endpoint adds later.
                // …and the model's OWN list where the listing said (part D):
                // a `/models` that names the tiers a model takes is the
                // detection the fixed suggestions stand in for.
                let tiers: Vec<&str> = if f.analysis_provider_api {
                    f.analysis_models.tiers_for(&f.analysis_model, &EFFORT_TIERS_API)
                } else {
                    EFFORT_TIERS_CLI.to_vec()
                };
                effort_picker(ui, "set_analysis_effort", &mut f.analysis_effort, &tiers, lang);
                if f.analysis_provider_api && f.analysis_models.efforts.contains_key(f.analysis_model.trim()) {
                    tiers_note(ui, tiers.is_empty(), lang);
                }
            });
            if f.analysis_provider_api {
                ui.row(|ui| {
                    ui.label(tr(lang, K_SETTINGS_BASE_URL));
                    ui.text_edit_singleline(&mut f.analysis_base_url);
                });
                ui.row(|ui| {
                    ui.label(tr(lang, K_SETTINGS_API_KEY));
                    let hint = if f.analysis_key_present { tr(lang, K_SETTINGS_KEY_SET_BLANK_KEEPS_IT) } else { tr(lang, K_SETTINGS_NO_KEY_SET) };
                    if ui
                        .add(egui::TextEdit::singleline(&mut f.analysis_api_key).password(true).hint_text(hint))
                        .changed()
                    {
                        // Availability is CREDENTIAL-dependent as well as
                        // URL-dependent — see the image role's key field.
                        f.analysis_models.clear();
                    }
                });
                ui.row(|ui| {
                    let label = if f.analysis_models.fetching {
                        tr(lang, K_SETTINGS_FETCHING)
                    } else {
                        tr(lang, K_SETTINGS_FETCH_MODELS)
                    };
                    if action(ui, !f.analysis_models.fetching, label)
                        .on_hover_text(tr(
                            lang,
                            K_SETTINGS_LIST_THE_MODELS_THIS_ENDPOINT_SERVES_GET_MODELS__2,
                        ))
                        .clicked()
                    {
                        fetch = Some(ModelRole::Analysis);
                    }
                    if !f.analysis_models.chat.is_empty() {
                        ui.label(
                            Text::new(trf(
                                lang,
                                K_SETTINGS_CHAT,
                                &[("chat", &f.analysis_models.chat.len().to_string())],
                            ))
                            .weak()
                            .small(),
                        );
                    }
                });
            }
            ui.rule();
            ui.heading(tr(lang, K_SETTINGS_IMAGE_MODEL_LOOKS_AT_YOUR_PHOTO_AND_GENERATES_OR));
            ui.row(|ui| {
                ui.label(tr(lang, K_SETTINGS_PROVIDER));
                ui.radio_value(&mut f.image_provider_oauth, false, tr(lang, K_SETTINGS_API_OPENAI_COMPATIBLE));
                ui.radio_value(&mut f.image_provider_oauth, true, tr(lang, K_SETTINGS_SIGN_IN_WITH_CHATGPT_VIA_THE_LOCAL_CODEX_BRIDGE));
            });
            // Flipping into OAuth while the endpoint is still empty or the stock
            // OpenAI host means the field is wrong for a subscription bridge —
            // swap in the loopback bridge default so it works without retyping.
            // Idempotent: stops once the user sets any other (custom) value.
            if f.image_provider_oauth {
                let b = f.image_base_url.trim();
                if b.is_empty() || b.trim_end_matches('/') == OPENAI_DEFAULT_URL {
                    f.image_base_url = CODEX_BRIDGE_URL.to_string();
                }
            } else {
                // Mirror image of the swap above: flipping BACK to API mode
                // with the auto-installed loopback bridge URL still in the
                // field would send real-API calls at a local bridge that may
                // not even be running. Idempotent, stops at custom values.
                let b = f.image_base_url.trim();
                if b.is_empty() || b.trim_end_matches('/') == CODEX_BRIDGE_URL.trim_end_matches('/') {
                    f.image_base_url = OPENAI_DEFAULT_URL.to_string();
                }
            }
            ui.row(|ui| {
                let label = if f.image_models.fetching { tr(lang, K_SETTINGS_FETCHING) } else { tr(lang, K_SETTINGS_FETCH_MODELS) };
                let clicked = action(ui, !f.image_models.fetching, label)
                    .on_hover_text(tr(
                        lang,
                        K_SETTINGS_LIST_THE_MODELS_THIS_ENDPOINT_SERVES_GET_MODELS,
                    ))
                    .clicked();
                if clicked {
                    fetch = Some(ModelRole::Image);
                }
                if !f.image_models.is_empty() {
                    let cn = f.image_models.chat.len().to_string();
                    let im = f.image_models.image_gen.len().to_string();
                    ui.label(
                        Text::new(trf(lang, K_SETTINGS_CHAT_IMAGE, &[("chat", &cn), ("image", &im)]))
                            .weak()
                            .small(),
                    );
                }
            });
            ui.row(|ui| {
                ui.label(if f.image_provider_oauth { tr(lang, K_SETTINGS_BRIDGE_URL) } else { tr(lang, K_SETTINGS_BASE_URL) });
                ui.text_edit_singleline(&mut f.image_base_url);
            });
            ui.row(|ui| {
                ui.label(tr(lang, K_SETTINGS_VISION_MODEL));
                let opts = model_opts(&f.image_models.chat, &["gpt-5.5", "gpt-4o"], &f.image_model);
                model_picker(ui, "set_vision_model", &mut f.image_model, &opts, lang);
                // The one thing the listing can say that a wrong pick would
                // otherwise only say at the first paid call.
                if f.image_models.vision_of(&f.image_model) == Some(false) {
                    ui.label(
                        Text::new(tr(lang, K_SETTINGS_TAKES_NO_IMAGES_PER_MODELS))
                            .color(ui.visuals().warn_fg_color)
                            .small(),
                    );
                }
            });
            ui.row(|ui| {
                ui.label(tr(lang, K_SETTINGS_REASONING_EFFORT));
                let tiers = f.image_models.tiers_for(&f.image_model, &EFFORT_TIERS_API);
                effort_picker(ui, "set_image_effort", &mut f.image_effort, &tiers, lang);
                if f.image_models.efforts.contains_key(f.image_model.trim()) {
                    tiers_note(ui, tiers.is_empty(), lang);
                }
            });
            ui.row(|ui| {
                ui.label(tr(lang, K_SETTINGS_API_STYLE));
                api_family_picker(ui, &mut f.image_api_family, lang);
            });
            ui.row(|ui| {
                ui.label(tr(lang, K_SETTINGS_IMAGE_GEN_MODEL));
                // OAuth (subscription) exposes gpt-image-2 first; API keys often
                // still prefer gpt-image-1.5 for its input_fidelity lock.
                let fallbacks: &[&str] = if f.image_provider_oauth {
                    &["gpt-image-2", "gpt-image-1.5"]
                } else {
                    &["gpt-image-1.5", "gpt-image-2", "gpt-image-1", "gpt-image-1-mini", "chatgpt-image-latest"]
                };
                let opts = model_opts(&f.image_models.image_gen, fallbacks, &f.image_gen_model);
                model_picker(ui, "set_imagegen_model", &mut f.image_gen_model, &opts, lang);
            });
            ui.row(|ui| {
                ui.label(if f.image_provider_oauth { tr(lang, K_SETTINGS_BRIDGE_PASSWORD) } else { tr(lang, K_SETTINGS_API_KEY) });
                let hint = if f.image_key_present {
                    tr(lang, K_SETTINGS_SET_BLANK_KEEPS_IT)
                } else if f.image_provider_oauth {
                    tr(lang, K_SETTINGS_THE_PASSWORD_SET_IN_THE_BRIDGE_S_OWN_CONFIG_API)
                } else {
                    tr(lang, K_SETTINGS_NO_KEY_SET)
                };
                if ui
                    .add(egui::TextEdit::singleline(&mut f.image_api_key).password(true).hint_text(hint))
                    .changed()
                {
                    // Model availability is CREDENTIAL-dependent, not just
                    // URL-dependent: a different key at the same endpoint can
                    // serve a different catalogue, and the URL-keyed
                    // self-invalidation above can't see that. Typing a new
                    // key drops the fetched lists (the pickers fall back to
                    // grounded defaults until the next fetch).
                    f.image_models.clear();
                }
            });
            let note = if f.image_provider_oauth {
                tr(lang, K_SETTINGS_USES_YOUR_CHATGPT_SUBSCRIPTION_THROUGH_THE_CODEX)
            } else {
                tr(lang, K_SETTINGS_TIP_GPT_IMAGE_1_5_KEEPS_THE_PHOTO_MOST_FAITHFUL)
            };
            ui.label(Text::new(note).weak().small());
            ui.rule();
            ui.row(|ui| {
                if primary(ui, true, tr(lang, K_SETTINGS_SAVE_SETTINGS)).clicked() {
                    do_save = true;
                }
                if !f.status.is_empty() {
                    ui.label(Text::new(&f.status).weak().small());
                }
            });
        }
        // The icon's fine print (round-13 easter egg).
        ui.gap(SPACE_SM);
        ui.label(
            Text::new(tr(
                lang,
                K_SETTINGS_SKYMANBP_S_AS_THE_AS_STANDS_FOR_AUTOSHADE_NOT_AN,
            ))
            .weak()
            .small(),
        );
        if do_save {
            self.act_all(vec![logic::Action::Settings(
                logic::settings::SettingsAction::SaveForm,
            )]);
        }
        if let Some(role) = fetch {
            self.act_all(vec![logic::Action::Settings(
                logic::settings::SettingsAction::FetchModels(match role {
                    ModelRole::Analysis => logic::settings::ModelSide::Analysis,
                    ModelRole::Image => logic::settings::ModelSide::Image,
                }),
            )]);
        }
    }
}


/// The launch line a check with something wrong gets; `None` when the sidecars
/// have a Python 3 and nothing has to be said.
pub(crate) fn python_startup_words(
    lang: Lang,
    check: &autoshade::python_env::PythonCheck,
) -> Option<String> {
    use autoshade::python_env::PythonState;
    match &check.state {
        PythonState::Managed(_) | PythonState::Interpreter => None,
        PythonState::UnpreparedDefault => Some(trf(
            lang,
            K_SETTINGS_THE_MANAGED_PYTHON_ENVIRONMENT_IS_NOT_PREPARED_T,
            &[("bin", &check.bin)],
        )),
        PythonState::Unusable => Some(trf(
            lang,
            K_SETTINGS_THE_AI_SIDECARS_NEED_PYTHON_3_DID_NOT_REPORT_ONE,
            &[("bin", &check.bin)],
        )),
    }
}

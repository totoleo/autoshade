// One part of the sidecar's tests (src/xmp/tests.rs includes it): the mask brush tables: import order, fixed-point fields, every refusal and the stage-three ground truth.

#[test]
fn mask_brush_tables_import_independently_in_owner_and_table_order() {
    let (dir, raw) = mb_temp("happy-multi");
    let objects = [mb_object(MB_GOOD_A), mb_object(MB_GOOD_B)];
    let (acr, tokens) = mb_acr(&objects);
    std::fs::write(raw.with_extension("acr"), acr).unwrap();
    let doc = mb_doc(&[
        ("First", &tokens[0], MB_GOOD_A_LEN),
        ("Second", &tokens[1], MB_GOOD_B_LEN),
    ]);
    let (recipe, lines) = mb_parse(&raw, &doc);
    assert!(lines.is_empty(), "valid tables emitted diagnostics: {lines:?}");
    assert_eq!(recipe.masks.len(), 2);
    let MaskGeometry::Brush { strokes: first, .. } = &recipe.masks[0].mask else {
        panic!("first table did not stay with its aggregate")
    };
    let MaskGeometry::Brush { strokes: second, .. } = &recipe.masks[1].mask else {
        panic!("second table did not stay with its aggregate")
    };
    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].sync_id, "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    assert_eq!(first[1].sync_id, "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB");
    assert_eq!(second[0].sync_id, "CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn mask_brush_fixed_point_fields_and_r_f_d_tokens_map_exactly() {
    let (dir, raw) = mb_temp("fixed-point");
    let (acr, tokens) = mb_acr(&[mb_object(MB_GOOD_A)]);
    std::fs::write(raw.with_extension("acr"), acr).unwrap();
    let recipe = mb_parse(&raw, &mb_doc(&[("Mask 1", &tokens[0], MB_GOOD_A_LEN)])).0;
    let MaskGeometry::Brush { strokes, .. } = &recipe.masks[0].mask else { panic!() };
    assert_eq!((strokes[0].value * 1_000_000.0).round() as u32, 51_402);
    assert_eq!((strokes[0].radius * 1_000_000.0).round() as u32, 36_957);
    assert_eq!((strokes[0].flow * 1_000_000.0).round() as u32, 1_000_000);
    assert_eq!(strokes[0].center_weight, 0.0);
    assert_eq!(
        strokes[0].dabs,
        "r 0.123456\nf 0.0103\nd 0.404621 0.692602\nd 0.401151 0.693698"
    );
    assert!(!strokes[0].dabs.lines().any(|token| token.starts_with("h ")));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn missing_mask_brush_companion_is_mask_brush_table_unavailable() {
    mb_assert_refusal(
        "unavailable",
        None,
        "00000000000000000000000000000000",
        MB_GOOD_A_LEN,
        MaskBrushTableRefusal::MaskBrushTableUnavailable,
    );
}

#[test]
fn malformed_mask_brush_directory_is_container_invalid() {
    let (mut acr, tokens) = mb_acr(&[mb_object(MB_GOOD_A)]);
    acr[12..16].copy_from_slice(&((MAX_ACR_DIRECTORY_ENTRIES + 1) as u32).to_le_bytes());
    mb_assert_refusal(
        "container",
        Some(&acr),
        &tokens[0],
        MB_GOOD_A_LEN,
        MaskBrushTableRefusal::ContainerInvalid,
    );

    let (mut acr, tokens) = mb_acr(&[mb_object(MB_GOOD_A), mb_object(MB_GOOD_B)]);
    let len = le_u64_at(&acr, 36);
    let offset = le_u64_at(&acr, 44);
    let padding = usize::try_from(offset + len).unwrap();
    assert_eq!(acr[padding], 0, "fixture must have inter-object padding");
    acr[padding] = 1;
    mb_assert_refusal(
        "container-padding",
        Some(&acr),
        &tokens[0],
        MB_GOOD_A_LEN,
        MaskBrushTableRefusal::ContainerInvalid,
    );
}

#[test]
fn absent_mask_brush_key_is_reference_mismatch() {
    let (acr, _) = mb_acr(&[mb_object(MB_GOOD_A)]);
    mb_assert_refusal(
        "reference",
        Some(&acr),
        "00000000000000000000000000000000",
        MB_GOOD_A_LEN,
        MaskBrushTableRefusal::ReferenceMismatch,
    );
}

#[test]
fn changed_mask_brush_blob_is_digest_mismatch() {
    let (mut acr, tokens) = mb_acr(&[mb_object(MB_GOOD_A)]);
    let len = le_u64_at(&acr, 36);
    let offset = le_u64_at(&acr, 44);
    let last = usize::try_from(offset + len - 1).unwrap();
    acr[last] ^= 0x01;
    mb_assert_refusal(
        "digest",
        Some(&acr),
        &tokens[0],
        MB_GOOD_A_LEN,
        MaskBrushTableRefusal::DigestMismatch,
    );
}

#[test]
fn unknown_mask_brush_envelope_is_encoding_unsupported() {
    let mut object = mb_object(MB_GOOD_A);
    object[0..4].copy_from_slice(&5u32.to_le_bytes());
    let (acr, tokens) = mb_acr(&[object]);
    mb_assert_refusal(
        "encoding",
        Some(&acr),
        &tokens[0],
        MB_GOOD_A_LEN,
        MaskBrushTableRefusal::EncodingUnsupported,
    );
}

#[test]
fn invalid_mask_brush_brotli_is_corrupt() {
    let (acr, tokens) = mb_acr(&[mb_object(&[0xFF])]);
    mb_assert_refusal(
        "corrupt",
        Some(&acr),
        &tokens[0],
        1,
        MaskBrushTableRefusal::Corrupt,
    );
}

#[test]
fn wrong_mask_brush_advertised_size_is_length_mismatch() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_GOOD_A)]);
    mb_assert_refusal(
        "length",
        Some(&acr),
        &tokens[0],
        MB_GOOD_A_LEN + 1,
        MaskBrushTableRefusal::LengthMismatch,
    );
}

#[test]
fn binary_h_opcode_is_payload_unsupported() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_UNKNOWN_OPCODE)]);
    mb_assert_refusal(
        "payload-unsupported",
        Some(&acr),
        &tokens[0],
        MB_UNKNOWN_OPCODE_LEN,
        MaskBrushTableRefusal::PayloadUnsupported,
    );
}

#[test]
fn trailing_mask_brush_payload_is_payload_invalid() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_TRAILING)]);
    mb_assert_refusal(
        "payload-invalid",
        Some(&acr),
        &tokens[0],
        MB_TRAILING_LEN,
        MaskBrushTableRefusal::PayloadInvalid,
    );
}

#[test]
fn one_bad_mask_brush_table_does_not_take_down_an_independent_table() {
    let objects = [mb_object(MB_GOOD_B), mb_object(MB_UNKNOWN_OPCODE)];
    let (acr, tokens) = mb_acr(&objects);
    let (dir, raw) = mb_temp("independent-refusal");
    std::fs::write(raw.with_extension("acr"), acr).unwrap();
    let survivor = lr_paint(
        "1111111111111111111111111111111C",
        "1",
        "0",
        "false",
        &["d 0.25 0.75"],
    );
    let bad_group = format!(
        "<rdf:li crs:What=\"Mask/Aggregate\" crs:MaskActive=\"true\" \
             crs:MaskName=\"Bad table\" crs:MaskBlendMode=\"0\" crs:MaskInverted=\"false\" \
             crs:MaskSyncID=\"0000000000000000000000000000000E\" crs:MaskValue=\"1\" \
             crs:MaskBrushTable=\"{}\" crs:MaskBrushUncompressedBytes=\"{}\">\n{}\
             </rdf:li>\n",
        tokens[1], MB_UNKNOWN_OPCODE_LEN, survivor
    );
    let doc = lr_doc(&format!(
        "{}{}",
        lr_correction(
            "Good",
            "",
            &mb_group("Brush 1", &tokens[0], MB_GOOD_B_LEN),
        ),
        lr_correction("Bad", "", &bad_group),
    ));
    let (recipe, lines) = mb_parse(&raw, &doc);
    assert_eq!(
        recipe.masks.len(),
        2,
        "the good table and bad table's text survivor must both import: {recipe:?}"
    );
    let MaskGeometry::Brush { strokes, .. } = &recipe.masks[1].mask else { panic!() };
    assert_eq!(strokes.len(), 1, "a refused table must contribute no partial records");
    assert_eq!(strokes[0].sync_id, "1111111111111111111111111111111C");
    assert!(lines.iter().any(|line| line.text.contains("PayloadUnsupported")));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn non_one_mask_brush_table_word_is_payload_unsupported() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_TABLE_WORD)]);
    mb_assert_refusal(
        "table-word",
        Some(&acr),
        &tokens[0],
        MB_TABLE_WORD_LEN,
        MaskBrushTableRefusal::PayloadUnsupported,
    );
}

#[test]
fn mask_brush_record_count_bound_refuses_before_allocation() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_RECORD_BOUND)]);
    mb_assert_refusal(
        "record-bound",
        Some(&acr),
        &tokens[0],
        MB_RECORD_BOUND_LEN,
        MaskBrushTableRefusal::PayloadInvalid,
    );
}

#[test]
fn mask_brush_d_count_bound_refuses_before_token_walk() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_DCOUNT_BOUND)]);
    mb_assert_refusal(
        "d-count-bound",
        Some(&acr),
        &tokens[0],
        MB_DCOUNT_BOUND_LEN,
        MaskBrushTableRefusal::PayloadInvalid,
    );
}

#[test]
fn mask_brush_token_count_bound_covers_unbounded_state_tokens() {
    let (acr, tokens) = mb_acr(&[mb_object(MB_TOKEN_BOUND)]);
    mb_assert_refusal(
        "token-bound",
        Some(&acr),
        &tokens[0],
        MB_TOKEN_BOUND_LEN,
        MaskBrushTableRefusal::PayloadInvalid,
    );
}

#[test]
fn table_import_preserves_residual_aggregate_and_gesture_paints() {
    let (dir, raw) = mb_temp("survivors");
    let (acr, tokens) = mb_acr(&[mb_object(MB_GOOD_B)]);
    std::fs::write(raw.with_extension("acr"), acr).unwrap();
    let aggregate_survivor = lr_brush_group("false", "");
    let gesture = lr_paint(
        "1111111111111111111111111111111B",
        "1",
        "0",
        "false",
        &["h 1.0000", "d 0.25 0.75"],
    )
    .replace(
        "crs:CenterWeight=\"0\">",
        "crs:CenterWeight=\"0\" crs:BrushGestureInterpretation=\"0\">",
    );
    let corrections = format!(
        "{}{}",
        lr_correction(
            "Brushes",
            "",
            &format!(
                "{}{}",
                mb_group("Binary", &tokens[0], MB_GOOD_B_LEN),
                aggregate_survivor
            ),
        ),
        lr_correction("Gesture", "", &lr_ai_mask("0", "0", "1", "", &gesture)),
    );
    let doc = lr_doc(&corrections);
    let recipe = mb_parse(&raw, &doc).0;
    assert_eq!(recipe.masks.len(), 2);
    assert_eq!(recipe.masks[0].components.len(), 1, "aggregate survivor was dropped");
    let MaskGeometry::AiMask { gesture, .. } = &recipe.masks[1].mask else { panic!() };
    assert_eq!(gesture.len(), 1, "gesture survivor was dropped");
    assert!(gesture[0].dabs.starts_with("h 1.0000\n"), "text h token must survive");
    let merged = merge_recipe_into_xmp_in_frame_for_photo(&doc, &recipe, None, Some(&raw))
        .expect("the survivor document merges");
    assert!(merged.doc.contains("crs:BrushGestureInterpretation=\"0\""));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn unchanged_table_mask_round_trip_keeps_attributes_without_text_paints() {
    let (dir, raw) = mb_temp("writer-round-trip");
    let (acr, tokens) = mb_acr(&[mb_object(MB_GOOD_A)]);
    std::fs::write(raw.with_extension("acr"), acr).unwrap();
    let doc = mb_doc(&[("Mask 1", &tokens[0], MB_GOOD_A_LEN)]);
    let mut recipe = mb_parse(&raw, &doc).0;
    recipe.exposure_ev = 0.75;
    let merged = merge_recipe_into_xmp_in_frame_for_photo(&doc, &recipe, None, Some(&raw))
        .expect("table-bearing base must merge");
    assert_eq!(merged.doc.matches("crs:MaskBrushTable=").count(), 1);
    assert!(merged.doc.contains(&format!("crs:MaskBrushTable=\"{}\"", tokens[0])));
    assert!(merged.doc.contains(&format!(
        "crs:MaskBrushUncompressedBytes=\"{MB_GOOD_A_LEN}\""
    )));
    assert_eq!(
        merged.doc.matches("crs:What=\"Mask/Paint\"").count(),
        0,
        "table records must not be synthesized as duplicate text Paints"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn env_mask_brush_sample_matches_stage_three_ground_truth() {
    let Some(root) = crate::config::live_env("AUTOSHADE_MB_SAMPLE_ROOT") else {
        crate::test_skipped(
            "env_mask_brush_sample_matches_stage_three_ground_truth",
            "AUTOSHADE_MB_SAMPLE_ROOT unset",
        );
        return;
    };
    let root = std::path::Path::new(&root);
    let xmp_path = root.join("P04-rewritten-brushtable.xmp");
    let photo = root.join("P04-rewritten.arw");
    let text = std::fs::read_to_string(&xmp_path).expect("read rewritten P04 XMP");
    let collector = crate::diag::Collector::new();
    let diag = crate::diag::Diag::about(&collector, &photo);
    let recipe = xmp_to_recipe_with_diag(&text, &diag);
    assert!(collector.take().is_empty(), "the confirmed specimen must parse without refusal");
    let mut tables: Vec<&Vec<BrushStroke>> = Vec::new();
    for mask in &recipe.masks {
        for geometry in std::iter::once(&mask.mask)
            .chain(mask.components.iter().map(|component| &component.geometry))
        {
            if let MaskGeometry::Brush { strokes, .. } = geometry
                && matches!(strokes.len(), 54 | 4 | 18)
            {
                tables.push(strokes);
            }
        }
    }
    assert_eq!(
        tables.iter().map(|table| table.len()).collect::<Vec<_>>(),
        [54, 4, 18],
        "the three table record counts stay in XMP order"
    );
    let d_count: usize = tables
        .iter()
        .flat_map(|table| table.iter())
        .map(|stroke| stroke.dabs.lines().filter(|token| token.starts_with("d ")).count())
        .sum();
    assert_eq!(d_count, 3_043);
    let t2_first = &tables[1][0];
    assert_eq!((t2_first.value * 1_000_000.0).round() as u32, 51_402);
    assert_eq!(t2_first.dabs.lines().next(), Some("d 0.404621 0.692602"));
}

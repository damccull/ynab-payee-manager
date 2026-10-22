use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use dioxus::prelude::*;

use crate::{
    database::{get_active_payee_ids, get_all_actions, get_review_items, save_review_item},
    fuzzy::find_duplicate_groups,
    models::{ActionStatus, DuplicateGroup, PayeeActionKind, PayeeReviewItem},
};

async fn run_scan(
    db: Arc<idb::Database>,
    payees_cache: Signal<Vec<crate::models::Payee>>,
    mut groups: Signal<Vec<DuplicateGroup>>,
    mut is_scanning: Signal<bool>,
    mut feedback_msg: Signal<Option<String>>,
    thresh: f64,
) {
    is_scanning.set(true);
    feedback_msg.set(None);

    // Fetch already queued IDs to prevent re-clustering payees that are in flight
    let mut queued_ids = HashSet::new();
    if let Ok(review_items) = get_review_items(&db).await {
        for item in review_items {
            for id in item.source_payee_ids {
                queued_ids.insert(id);
            }
        }
    }
    if let Ok(actions) = get_all_actions(&db).await {
        for action in actions {
            if action.status != ActionStatus::Confirmed {
                match action.kind {
                    PayeeActionKind::MergePayees {
                        target_payee_id,
                        source_payee_ids,
                        ..
                    } => {
                        queued_ids.insert(target_payee_id);
                        for id in source_payee_ids {
                            queued_ids.insert(id);
                        }
                    }
                    PayeeActionKind::RenamePayee { payee_id, .. } => {
                        queued_ids.insert(payee_id);
                    }
                }
            }
        }
    }

    let active_ids = get_active_payee_ids(&db).await.unwrap_or_default();
    let filter_zero_tx = !active_ids.is_empty();

    let cache = payees_cache.read();
    let unqueued_payees: Vec<crate::models::Payee> = cache
        .iter()
        .filter(|p| {
            if queued_ids.contains(&p.id) {
                return false;
            }
            if filter_zero_tx && !active_ids.contains(&p.id) {
                return false;
            }
            true
        })
        .cloned()
        .collect();

    let found_groups = find_duplicate_groups(&unqueued_payees, thresh);
    let total_candidates: usize = found_groups.iter().map(|g| g.candidates.len()).sum();
    groups.set(found_groups);
    is_scanning.set(false);

    if groups.read().is_empty() {
        feedback_msg.set(Some("No duplicate payee groups detected with current sensitivity.".to_string()));
    } else {
        feedback_msg.set(Some(format!(
            "Found {} potential duplicate groups ({} payees).",
            groups.read().len(),
            total_candidates
        )));
    }
}

#[component]
pub fn Duplicates(
    payees_cache: Signal<Vec<crate::models::Payee>>,
    review_count: Signal<usize>,
) -> Element {
    let db = use_context::<Arc<idb::Database>>();

    let mut sensitivity = use_signal(|| 0.92f64);
    let mut search_query = use_signal(|| String::new());
    let mut groups = use_signal(Vec::<DuplicateGroup>::new);
    let is_scanning = use_signal(|| false);
    let feedback_msg = use_signal(|| Option::<String>::None);

    // Initial scan when component mounts or cache populates
    let db_init = db.clone();
    use_effect(move || {
        let cache_len = payees_cache.read().len();
        if cache_len > 0 && groups.read().is_empty() {
            let thresh = *sensitivity.read();
            spawn(run_scan(db_init.clone(), payees_cache, groups, is_scanning, feedback_msg, thresh));
        }
    });

    let groups_list = groups.read();
    let query = search_query.read().to_lowercase();
    let filtered_groups: Vec<(usize, DuplicateGroup)> = groups_list
        .iter()
        .enumerate()
        .filter(|(_, g)| {
            if query.is_empty() {
                return true;
            }
            g.suggested_target_name.to_lowercase().contains(&query)
                || g.candidates.iter().any(|c| c.payee.name.to_lowercase().contains(&query))
        })
        .map(|(i, g)| (i, g.clone()))
        .collect();

    rsx! {
        div {
            class: "duplicates-container",
            div {
                class: "duplicates-header",
                div {
                    class: "header-title-block",
                    h2 { "Fuzzy Duplicate Payees" }
                    p {
                        class: "section-subtitle",
                        "Review suspected duplicate payees. All items start pre-checked; uncheck any false positives. "
                        "Combining checked items adds the new payee with the cleanest name to the Payee Review Queue."
                    }
                }
                div {
                    class: "controls-bar",
                    div {
                        class: "sensitivity-selector",
                        span { class: "control-label", "Sensitivity: " }
                        button {
                            class: if (*sensitivity.read() - 0.92).abs() < 0.01 { "btn-chip active" } else { "btn-chip" },
                            onclick: {
                                let db = db.clone();
                                move |_| {
                                    sensitivity.set(0.92);
                                    spawn(run_scan(db.clone(), payees_cache, groups, is_scanning, feedback_msg, 0.92));
                                }
                            },
                            "Strict (92%)"
                        }
                        button {
                            class: if (*sensitivity.read() - 0.85).abs() < 0.01 { "btn-chip active" } else { "btn-chip" },
                            onclick: {
                                let db = db.clone();
                                move |_| {
                                    sensitivity.set(0.85);
                                    spawn(run_scan(db.clone(), payees_cache, groups, is_scanning, feedback_msg, 0.85));
                                }
                            },
                            "Balanced (85%)"
                        }
                        button {
                            class: if (*sensitivity.read() - 0.78).abs() < 0.01 { "btn-chip active" } else { "btn-chip" },
                            onclick: {
                                let db = db.clone();
                                move |_| {
                                    sensitivity.set(0.78);
                                    spawn(run_scan(db.clone(), payees_cache, groups, is_scanning, feedback_msg, 0.78));
                                }
                            },
                            "Loose (78%)"
                        }
                    }
                    div {
                        class: "search-box",
                        input {
                            r#type: "text",
                            class: "search-input",
                            placeholder: "Search groups...",
                            value: "{search_query.read()}",
                            oninput: move |evt| search_query.set(evt.value()),
                        }
                    }
                    button {
                        class: "btn btn-secondary",
                        disabled: *is_scanning.read(),
                        onclick: {
                            let db = db.clone();
                            move |_| {
                                let thresh = *sensitivity.read();
                                spawn(run_scan(db.clone(), payees_cache, groups, is_scanning, feedback_msg, thresh));
                            }
                        },
                        if *is_scanning.read() { "Scanning..." } else { "Re-scan" }
                    }
                }
            }

            if let Some(msg) = feedback_msg.read().as_ref() {
                div { class: "info-banner", "{msg}" }
            }

            if *is_scanning.read() {
                div { class: "loading-container", p { "Analyzing payee names and calculating fuzzy similarity..." } }
            } else if filtered_groups.is_empty() {
                div {
                    class: "empty-state-card",
                    p { class: "empty-title", "No Duplicate Groups Found" }
                    p { class: "empty-description", "All eligible payees appear unique, or all detected duplicates are already queued." }
                }
            } else {
                div {
                    class: "groups-grid",
                    for (_group_idx, group) in filtered_groups {
                        {
                            let local_group = group.clone();
                            let group_id = group.id.clone();
                            let db = db.clone();

                            let checked_count = local_group.candidates.iter().filter(|c| c.is_checked).count();
                            let total_count = local_group.candidates.len();

                            rsx! {
                                div {
                                    key: "{group_id}",
                                    class: "group-card",
                                    div {
                                        class: "group-card-header",
                                        div {
                                            class: "group-title-row",
                                            span { class: "group-name", "{local_group.suggested_target_name}" }
                                            span { class: "badge badge-reason", "{local_group.match_reason}" }
                                            span { class: "badge badge-count", "{checked_count} of {total_count} selected" }
                                        }
                                    }

                                    div {
                                        class: "candidate-list",
                                        for (cand_idx, cand) in local_group.candidates.iter().enumerate() {
                                            {
                                                let is_checked = cand.is_checked;
                                                let payee_name = cand.payee.name.clone();
                                                let payee_id = cand.payee.id.clone();
                                                let is_suggested = payee_name == local_group.suggested_target_name;
                                                let g_id_cand = group_id.clone();

                                                rsx! {
                                                    div {
                                                        key: "{payee_id}",
                                                        class: if is_checked { "candidate-row checked" } else { "candidate-row unchecked" },
                                                        label {
                                                            class: "checkbox-container",
                                                            input {
                                                                r#type: "checkbox",
                                                                checked: is_checked,
                                                                onchange: {
                                                                    let g_id = g_id_cand.clone();
                                                                    move |evt| {
                                                                        let mut cur = groups.read().clone();
                                                                        if let Some(target_g) = cur.iter_mut().find(|g| g.id == g_id) {
                                                                            if let Some(c) = target_g.candidates.get_mut(cand_idx) {
                                                                                c.is_checked = evt.checked();
                                                                            }
                                                                        }
                                                                        groups.set(cur);
                                                                    }
                                                                }
                                                            }
                                                            div {
                                                                class: "candidate-info",
                                                                span {
                                                                    class: "candidate-name",
                                                                    "{payee_name}"
                                                                }
                                                                if is_suggested {
                                                                    span { class: "badge badge-cleanest", "Cleanest Name" }
                                                                }
                                                                span {
                                                                    class: "candidate-id-subtext",
                                                                    "ID: {payee_id}"
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    div {
                                        class: "group-card-footer",
                                        div {
                                            class: "suggested-name-box",
                                            span { class: "clean-label", "Target Name to Queue: " }
                                            strong { "{local_group.suggested_target_name}" }
                                        }
                                        div {
                                            class: "group-actions",
                                            button {
                                                class: "btn btn-sm btn-outline",
                                                onclick: {
                                                    let g_id = group_id.clone();
                                                    move |_| {
                                                        let mut cur = groups.read().clone();
                                                        cur.retain(|g| g.id != g_id);
                                                        groups.set(cur);
                                                    }
                                                },
                                                "Skip Group"
                                            }
                                            button {
                                                class: "btn btn-sm btn-primary",
                                                disabled: checked_count < 2,
                                                onclick: {
                                                    let g_id = group_id.clone();
                                                    let db = db.clone();
                                                    let mut review_count = review_count;
                                                    let mut feedback_msg = feedback_msg;

                                                    move |_| {
                                                        let cur = groups.read().clone();
                                                        let Some(target_group) = cur.iter().find(|g| g.id == g_id).cloned() else {
                                                            return;
                                                        };

                                                        let checked_candidates: Vec<_> = target_group
                                                            .candidates
                                                            .iter()
                                                            .filter(|c| c.is_checked)
                                                            .collect();

                                                        if checked_candidates.len() >= 2 {
                                                            let source_ids: Vec<String> = checked_candidates
                                                                .iter()
                                                                .map(|c| c.payee.id.clone())
                                                                .collect();
                                                            let source_names: Vec<String> = checked_candidates
                                                                .iter()
                                                                .map(|c| c.payee.name.clone())
                                                                .collect();
                                                            let count = source_ids.len();

                                                            let review_item = PayeeReviewItem {
                                                                id: uuid::Uuid::new_v4().to_string(),
                                                                suggested_name: target_group.suggested_target_name.clone(),
                                                                custom_name: None,
                                                                source_payee_ids: source_ids,
                                                                source_payee_names: source_names,
                                                                created_at: Utc::now(),
                                                            };

                                                            let db = db.clone();
                                                            let g_id_to_remove = g_id.clone();

                                                            spawn(async move {
                                                                match save_review_item(&db, &review_item).await {
                                                                    Ok(()) => {
                                                                        if let Ok(items) = get_review_items(&db).await {
                                                                            *review_count.write() = items.len();
                                                                        }
                                                                        let mut cur = groups.read().clone();
                                                                        cur.retain(|g| g.id != g_id_to_remove);
                                                                        groups.set(cur);
                                                                        feedback_msg.set(Some(format!(
                                                                            "Combined {} payees into '{}' and queued for review.",
                                                                            count, review_item.suggested_name
                                                                        )));
                                                                    }
                                                                    Err(e) => {
                                                                        feedback_msg.set(Some(format!("Database error: {:?}", e)));
                                                                    }
                                                                }
                                                            });
                                                        }
                                                    }
                                                },
                                                if checked_count < 2 { "Select at least 2" } else { "Combine Checked & Queue" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

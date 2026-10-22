use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use dioxus::prelude::*;

use crate::{
    database::{
        delete_ignored_pair, get_active_payee_ids, get_all_actions, get_ignored_pair_ids,
        get_ignored_pairs, get_review_items, save_ignored_pairs, save_review_item,
    },
    fuzzy::find_duplicate_groups,
    models::{ActionStatus, DuplicateGroup, IgnoredPair, PayeeActionKind, PayeeReviewItem},
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

    let ignored_pair_ids = get_ignored_pair_ids(&db).await.unwrap_or_default();
    let found_groups = find_duplicate_groups(&unqueued_payees, thresh, &ignored_pair_ids);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DuplicateTab {
    Groups,
    IgnoredPairs,
}

#[component]
pub fn Duplicates(
    payees_cache: Signal<Vec<crate::models::Payee>>,
    review_count: Signal<usize>,
) -> Element {
    let db = use_context::<Arc<idb::Database>>();

    let mut active_tab = use_signal(|| DuplicateTab::Groups);
    let mut sensitivity = use_signal(|| 0.92f64);
    let mut search_query = use_signal(|| String::new());
    let mut groups = use_signal(Vec::<DuplicateGroup>::new);
    let is_scanning = use_signal(|| false);
    let feedback_msg = use_signal(|| Option::<String>::None);
    let mut ignored_pairs = use_signal(Vec::<IgnoredPair>::new);
    let mut ignored_search = use_signal(|| String::new());

    // Initial scan and load ignored pairs
    let db_init = db.clone();
    use_effect(move || {
        let db = db_init.clone();
        let cache_len = payees_cache.read().len();
        if cache_len > 0 && groups.read().is_empty() {
            let thresh = *sensitivity.read();
            spawn(run_scan(db.clone(), payees_cache, groups, is_scanning, feedback_msg, thresh));
        }
        let db_ign = db.clone();
        spawn(async move {
            if let Ok(pairs) = get_ignored_pairs(&db_ign).await {
                ignored_pairs.set(pairs);
            }
        });
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

    let ign_query = ignored_search.read().to_lowercase();
    let ignored_list = ignored_pairs.read();
    let filtered_ignored: Vec<IgnoredPair> = ignored_list
        .iter()
        .filter(|p| {
            if ign_query.is_empty() {
                return true;
            }
            p.payee_name_1.to_lowercase().contains(&ign_query)
                || p.payee_name_2.to_lowercase().contains(&ign_query)
        })
        .cloned()
        .collect();

    rsx! {
        div {
            class: "duplicates-container",

            div {
                class: "dup-tabs-bar",
                button {
                    class: if *active_tab.read() == DuplicateTab::Groups { "dup-tab active" } else { "dup-tab" },
                    onclick: move |_| active_tab.set(DuplicateTab::Groups),
                    "Potential Duplicates ({groups_list.len()})"
                }
                button {
                    class: if *active_tab.read() == DuplicateTab::IgnoredPairs { "dup-tab active" } else { "dup-tab" },
                    onclick: {
                        let db = db.clone();
                        move |_| {
                            active_tab.set(DuplicateTab::IgnoredPairs);
                            let db = db.clone();
                            spawn(async move {
                                if let Ok(pairs) = get_ignored_pairs(&db).await {
                                    ignored_pairs.set(pairs);
                                }
                            });
                        }
                    },
                    "Ignored Pairs ({ignored_list.len()})"
                }
            }

            if let Some(msg) = feedback_msg.read().as_ref() {
                div { class: "info-banner", "{msg}" }
            }

            if *active_tab.read() == DuplicateTab::IgnoredPairs {
                div {
                    class: "ignored-view",
                    div {
                        class: "controls-card",
                        div {
                            class: "controls-row",
                            div {
                                class: "search-box",
                                style: "flex-grow: 1; max-width: 480px;",
                                input {
                                    r#type: "text",
                                    class: "search-input",
                                    placeholder: "Search ignored pairs by payee name...",
                                    value: "{ignored_search.read()}",
                                    oninput: move |evt| ignored_search.set(evt.value()),
                                }
                            }
                            if !ignored_search.read().is_empty() {
                                button {
                                    class: "btn btn-sm btn-secondary",
                                    onclick: move |_| ignored_search.set(String::new()),
                                    "Clear"
                                }
                            }
                        }
                    }

                    if filtered_ignored.is_empty() {
                        div {
                            class: "empty-state-card",
                            p {
                                class: "empty-title",
                                if ignored_search.read().is_empty() {
                                    "No Ignored Pairs"
                                } else {
                                    "No Matching Ignored Pairs"
                                }
                            }
                            p {
                                class: "empty-description",
                                if ignored_search.read().is_empty() {
                                    "Use the 'Not Duplicates' button on any duplicate group to exclude payees from clustering together."
                                } else {
                                    "Try adjusting your search query."
                                }
                            }
                        }
                    } else {
                        div {
                            class: "table-container",
                            table {
                                thead {
                                    tr {
                                        th { "Payee 1" }
                                        th { style: "width: 40px; text-align: center;", "" }
                                        th { "Payee 2" }
                                        th { "Marked On" }
                                        th { style: "text-align: right; width: 180px;", "Action" }
                                    }
                                }
                                tbody {
                                    for pair in filtered_ignored {
                                        {
                                            let pair_id = pair.id.clone();
                                            let db = db.clone();
                                            let payees_cache = payees_cache.clone();
                                            let groups = groups;
                                            let is_scanning = is_scanning;
                                            let mut feedback_msg = feedback_msg;
                                            let mut ignored_pairs = ignored_pairs;
                                            let thresh = *sensitivity.read();

                                            let formatted_date = pair.created_at.format("%Y-%m-%d %H:%M").to_string();

                                            rsx! {
                                                tr {
                                                    key: "{pair_id}",
                                                    td {
                                                        div { style: "font-weight: 600;", "{pair.payee_name_1}" }
                                                        div { class: "payee-id-subtext", "{pair.payee_id_1}" }
                                                    }
                                                    td {
                                                        style: "text-align: center; color: #8b949e;",
                                                        "↮"
                                                    }
                                                    td {
                                                        div { style: "font-weight: 600;", "{pair.payee_name_2}" }
                                                        div { class: "payee-id-subtext", "{pair.payee_id_2}" }
                                                    }
                                                    td {
                                                        "{formatted_date}"
                                                    }
                                                    td {
                                                        style: "text-align: right;",
                                                        button {
                                                            class: "btn btn-sm btn-outline-danger",
                                                            title: "Allow these payees to be matched again as duplicates",
                                                            onclick: move |_| {
                                                                let pid = pair_id.clone();
                                                                let db = db.clone();
                                                                spawn(async move {
                                                                    if let Ok(()) = delete_ignored_pair(&db, &pid).await {
                                                                        if let Ok(updated) = get_ignored_pairs(&db).await {
                                                                            ignored_pairs.set(updated);
                                                                        }
                                                                        feedback_msg.set(Some("Restored pair. It will be considered in future duplicate scans.".to_string()));
                                                                        run_scan(db.clone(), payees_cache, groups, is_scanning, feedback_msg, thresh).await;
                                                                    }
                                                                });
                                                            },
                                                            "Undo (Allow Match)"
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
            } else {
                div {
                    class: "groups-view",
                    div {
                        class: "controls-card",
                        div {
                            class: "controls-row",
                            div {
                                class: "chip-group",
                                span { class: "chip-label", "Sensitivity: " }
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

                    if *is_scanning.read() {
                        div { class: "loading-container", p { "Analyzing payee names and calculating fuzzy similarity..." } }
                    } else if filtered_groups.is_empty() {
                        div {
                            class: "empty-state-card",
                            p { class: "empty-title", "No Duplicate Groups Found" }
                            p { class: "empty-description", "All eligible payees appear unique, or all detected duplicates are already queued or ignored." }
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
                                                        class: "btn btn-sm btn-outline-warning",
                                                        disabled: checked_count < 2,
                                                        title: "Mark all checked payees in this group as not duplicates of each other",
                                                        onclick: {
                                                            let g_id = group_id.clone();
                                                            let db = db.clone();
                                                            let payees_cache = payees_cache.clone();
                                                            let groups = groups;
                                                            let is_scanning = is_scanning;
                                                            let mut feedback_msg = feedback_msg;
                                                            let mut ignored_pairs = ignored_pairs;
                                                            let thresh = *sensitivity.read();

                                                            move |_| {
                                                                let cur = groups.read().clone();
                                                                let Some(target_group) = cur.iter().find(|g| g.id == g_id).cloned() else {
                                                                    return;
                                                                };

                                                                let checked: Vec<_> = target_group
                                                                    .candidates
                                                                    .iter()
                                                                    .filter(|c| c.is_checked)
                                                                    .map(|c| c.payee.clone())
                                                                    .collect();

                                                                if checked.len() < 2 {
                                                                    return;
                                                                }

                                                                let db = db.clone();
                                                                spawn(async move {
                                                                    let mut pairs = Vec::new();
                                                                    for i in 0..checked.len() {
                                                                        for j in (i + 1)..checked.len() {
                                                                            pairs.push(IgnoredPair::new(&checked[i], &checked[j]));
                                                                        }
                                                                    }
                                                                    let pair_count = pairs.len();
                                                                    if let Err(e) = save_ignored_pairs(&db, &pairs).await {
                                                                        feedback_msg.set(Some(format!("Database error saving ignored pairs: {:?}", e)));
                                                                        return;
                                                                    }

                                                                    if let Ok(updated) = get_ignored_pairs(&db).await {
                                                                        ignored_pairs.set(updated);
                                                                    }

                                                                    feedback_msg.set(Some(format!(
                                                                        "Marked {} pair(s) among {} payees as not duplicates.",
                                                                        pair_count,
                                                                        checked.len(),
                                                                    )));

                                                                    run_scan(db.clone(), payees_cache, groups, is_scanning, feedback_msg, thresh).await;
                                                                });
                                                            }
                                                        },
                                                        if checked_count >= 2 { "Not Duplicates ({checked_count})" } else { "Not Duplicates" }
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
    }
}

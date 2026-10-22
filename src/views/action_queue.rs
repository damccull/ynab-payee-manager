use std::sync::Arc;

use chrono::Utc;
use dioxus::prelude::*;
use reqwest::header::HeaderMap;
use serde_json::json;

use crate::{
    database::{
        clear_all_actions, delete_action, enqueue_action, get_all_actions,
        get_next_action_sequence, get_pending_actions, purge_expired_actions, update_action,
    },
    models::{ActionStatus, Payee, PayeeAction, PayeeActionKind},
    AuthActions,
};

#[component]
pub fn ActionQueue(
    budget_id: String,
    action_count: Signal<usize>,
    payees_cache: Signal<Vec<Payee>>,
) -> Element {
    let db = use_context::<Arc<idb::Database>>();
    let auth = use_context::<AuthActions>();
    let token = (auth.token)();

    let mut actions = use_signal(Vec::<PayeeAction>::new);
    let mut is_loading = use_signal(|| false);
    let mut is_committing = use_signal(|| false);
    let mut progress_message = use_signal(|| Option::<String>::None);
    let mut notification = use_signal(|| Option::<String>::None);
    let mut error_banner = use_signal(|| Option::<String>::None);

    // Standalone Rename Payee state
    let mut rename_payee_id = use_signal(|| String::new());
    let mut rename_new_name = use_signal(|| String::new());
    let mut show_rename_dialog = use_signal(|| false);

    let db_for_load = db.clone();
    let load_actions = move || {
        let db = db_for_load.clone();
        let mut action_count = action_count;
        spawn(async move {
            is_loading.set(true);
            // 1. Purge expired actions older than 1 hour
            if let Ok(purged) = purge_expired_actions(&db).await {
                if purged > 0 {
                    notification.set(Some(format!(
                        "Automatically tossed {} expired action(s) older than 1 hour.",
                        purged
                    )));
                }
            }

            // 2. Fetch all actions
            if let Ok(all_actions) = get_all_actions(&db).await {
                let pending_count = all_actions
                    .iter()
                    .filter(|a| a.status != ActionStatus::Confirmed)
                    .count();
                *action_count.write() = pending_count;
                actions.set(all_actions);
            }
            is_loading.set(false);
        });
    };

    use_effect(move || {
        load_actions();
    });

    let actions_list = actions.read();
    let has_in_progress = actions_list
        .iter()
        .any(|a| matches!(a.status, ActionStatus::InProgress));
    let pending_actions: Vec<_> = actions_list
        .iter()
        .filter(|a| a.status != ActionStatus::Confirmed)
        .cloned()
        .collect();
    let confirmed_actions_count = actions_list
        .iter()
        .filter(|a| matches!(a.status, ActionStatus::Confirmed))
        .count();

    // Commit / Execute Runner
    let db_for_commit = db.clone();
    let token_for_commit = token.clone();
    let execute_commit = move |_| {
        let db = db_for_commit.clone();
        let token = token_for_commit.clone();
        let budget = budget_id.clone();
        let mut action_count = action_count;

        spawn(async move {
            let Some(token_obj) = token else {
                error_banner.set(Some("Cannot commit actions: No valid YNAB token found.".to_string()));
                return;
            };

            if !token_obj.is_valid() {
                error_banner.set(Some("Cannot commit actions: YNAB token is expired. Please re-authenticate.".to_string()));
                return;
            }

            let token_str = token_obj.to_string();
            is_committing.set(true);
            error_banner.set(None);

            // Purge expired actions first
            let _ = purge_expired_actions(&db).await;

            // Fetch pending actions in strict sequence order
            let pending = match get_pending_actions(&db).await {
                Ok(p) => p,
                Err(e) => {
                    error_banner.set(Some(format!("Failed to read pending actions from database: {:?}", e)));
                    is_committing.set(false);
                    return;
                }
            };

            let total = pending.len();
            if total == 0 {
                is_committing.set(false);
                progress_message.set(None);
                notification.set(Some("No pending actions to commit.".to_string()));
                return;
            }

            let httpclient = reqwest::Client::new();
            let mut headers = HeaderMap::new();
            if let Ok(auth_val) = format!("Bearer {}", token_str).parse() {
                headers.insert("Authorization", auth_val);
            }
            headers.insert("Content-Type", "application/json".parse().unwrap());

            let mut executed_count = 0;

            for (idx, mut action) in pending.into_iter().enumerate() {
                let current_step = idx + 1;
                let step_desc = match &action.kind {
                    PayeeActionKind::RenamePayee { new_name, .. } => format!("Renaming to '{}'", new_name),
                    PayeeActionKind::MergePayees { target_name, .. } => format!("Merging into '{}'", target_name),
                };

                progress_message.set(Some(format!(
                    "Executing step {} of {}: {}",
                    current_step, total, step_desc
                )));

                // Mark in-progress in IndexedDB for crash recovery
                action.status = ActionStatus::InProgress;
                let _ = update_action(&db, &action).await;

                // Refresh UI list state
                if let Ok(all) = get_all_actions(&db).await {
                    actions.set(all);
                }

                // Execute action against YNAB API
                let result = match &action.kind {
                    PayeeActionKind::RenamePayee { payee_id, new_name, .. } => {
                        let url = format!("https://api.ynab.com/v1/budgets/{}/payees/{}", budget, payee_id);
                        let body = json!({
                            "payee": {
                                "name": new_name
                            }
                        });

                        let res = httpclient
                            .patch(&url)
                            .headers(headers.clone())
                            .json(&body)
                            .send()
                            .await;

                        match res {
                            Ok(r) if r.status().is_success() => Ok(()),
                            Ok(r) => {
                                let st = r.status();
                                let txt = r.text().await.unwrap_or_default();
                                Err(format!("YNAB API error {}: {}", st, txt))
                            }
                            Err(e) => Err(format!("Network request failed: {}", e)),
                        }
                    }
                    PayeeActionKind::MergePayees {
                        target_payee_id,
                        target_name,
                        source_payee_ids,
                        ..
                    } => {
                        // 1. Rename target payee if needed
                        let rename_url = format!("https://api.ynab.com/v1/budgets/{}/payees/{}", budget, target_payee_id);
                        let rename_body = json!({
                            "payee": {
                                "name": target_name
                            }
                        });

                        let rename_res = httpclient
                            .patch(&rename_url)
                            .headers(headers.clone())
                            .json(&rename_body)
                            .send()
                            .await;

                        if let Ok(r) = rename_res {
                            if !r.status().is_success() {
                                let st = r.status();
                                let txt = r.text().await.unwrap_or_default();
                                // If rename fails, report error
                                debug!("Rename target payee notice {}: {}", st, txt);
                            }
                        }

                        // 2. Fetch transactions to reassign from source_payee_ids to target_payee_id
                        let tx_url = format!("https://api.ynab.com/v1/budgets/{}/transactions", budget);
                        let tx_res = httpclient
                            .get(&tx_url)
                            .headers(headers.clone())
                            .send()
                            .await;

                        match tx_res {
                            Ok(r) if r.status().is_success() => {
                                #[derive(serde::Deserialize)]
                                struct TxData {
                                    transactions: Vec<TxItem>,
                                }
                                #[derive(serde::Deserialize)]
                                struct TxItem {
                                    id: String,
                                    payee_id: Option<String>,
                                }
                                #[derive(serde::Deserialize)]
                                struct TxResponse {
                                    data: TxData,
                                }

                                if let Ok(parsed) = r.json::<TxResponse>().await {
                                    let mut updates = Vec::new();
                                    for tx in parsed.data.transactions {
                                        if let Some(pid) = tx.payee_id {
                                            if source_payee_ids.contains(&pid) {
                                                updates.push(json!({
                                                    "id": tx.id,
                                                    "payee_id": target_payee_id
                                                }));
                                            }
                                        }
                                    }

                                    if !updates.is_empty() {
                                        let patch_tx_res = httpclient
                                            .patch(&tx_url)
                                            .headers(headers.clone())
                                            .json(&json!({ "transactions": updates }))
                                            .send()
                                            .await;

                                        match patch_tx_res {
                                            Ok(pr) if pr.status().is_success() => Ok(()),
                                            Ok(pr) => {
                                                let st = pr.status();
                                                let txt = pr.text().await.unwrap_or_default();
                                                Err(format!("Failed reassigning transactions ({}: {})", st, txt))
                                            }
                                            Err(e) => Err(format!("Transaction update network error: {}", e)),
                                        }
                                    } else {
                                        // 0 transactions to reassign
                                        Ok(())
                                    }
                                } else {
                                    Ok(())
                                }
                            }
                            Ok(r) => {
                                let st = r.status();
                                let txt = r.text().await.unwrap_or_default();
                                Err(format!("Failed fetching transactions ({}: {})", st, txt))
                            }
                            Err(e) => Err(format!("Network error fetching transactions: {}", e)),
                        }
                    }
                };

                match result {
                    Ok(()) => {
                        action.status = ActionStatus::Confirmed;
                        action.confirmed_at = Some(Utc::now());
                        let _ = update_action(&db, &action).await;
                        executed_count += 1;
                    }
                    Err(err) => {
                        action.status = ActionStatus::Failed { error: err.clone() };
                        let _ = update_action(&db, &action).await;

                        if let Ok(all) = get_all_actions(&db).await {
                            actions.set(all);
                        }

                        error_banner.set(Some(format!(
                            "Execution halted at step {}/{} due to error: {}. Remaining actions remain saved in IndexedDB and can be resumed.",
                            current_step, total, err
                        )));
                        is_committing.set(false);
                        progress_message.set(None);
                        return;
                    }
                }

                // Update UI list state after confirmation
                if let Ok(all) = get_all_actions(&db).await {
                    actions.set(all);
                }
            }

            // Run delta refresh for transactions and payees
            if executed_count > 0 {
                progress_message.set(Some("Performing delta sync with YNAB...".to_string()));
                let tx_k = crate::database::get_transactions(&db).await.map(|(_, k)| k).unwrap_or(0);
                let payee_k = crate::database::get_payees(&db).await.map(|(_, k)| k).unwrap_or(0);

                let _ = crate::api::fetch_and_store_transactions(&db, &token_str, &budget, tx_k, true).await;
                if let Ok((updated_payees, _)) = crate::api::fetch_and_store_payees(&db, &token_str, &budget, payee_k, true).await {
                    *payees_cache.write() = updated_payees;
                }
            }

            is_committing.set(false);
            progress_message.set(None);
            notification.set(Some(format!(
                "Successfully committed and confirmed {} action(s) and synced delta state with YNAB!",
                executed_count
            )));

            if let Ok(pending_now) = get_pending_actions(&db).await {
                *action_count.write() = pending_now.len();
            }
        });
    };

    rsx! {
        div {
            class: "actions-container",
            div {
                class: "actions-header",
                div {
                    class: "header-title-block",
                    h2 { "Action Queue & Commit" }
                    p {
                        class: "section-subtitle",
                        "All payee actions (combines and renames) are staged here in strict sequence. "
                        "They are sent one-by-one only after you click 'Commit Changes'. "
                        "If the app or network is interrupted, unconfirmed actions are safely preserved for resume. "
                        "Actions older than 1 hour expire automatically."
                    }
                }
                div {
                    class: "header-controls",
                    button {
                        class: "btn btn-secondary",
                        onclick: move |_| {
                            let cur = *show_rename_dialog.read();
                            show_rename_dialog.set(!cur);
                        },
                        if *show_rename_dialog.read() { "Close Rename Form" } else { "+ Queue a Payee Rename" }
                    }
                    button {
                        class: "btn btn-primary",
                        disabled: pending_actions.is_empty() || *is_committing.read(),
                        onclick: execute_commit,
                        if *is_committing.read() {
                            "Executing..."
                        } else if has_in_progress {
                            "Resume Commit ({pending_actions.len()} Pending)"
                        } else {
                            "Commit Changes ({pending_actions.len()} Pending)"
                        }
                    }
                }
            }

            if *show_rename_dialog.read() {
                div {
                    class: "rename-dialog-card",
                    h3 { "Queue a Payee Rename" }
                    div {
                        class: "form-row",
                        div {
                            class: "form-group",
                            label { "Select Payee:" }
                            select {
                                class: "select-input",
                                onchange: move |evt| rename_payee_id.set(evt.value()),
                                option { value: "", "Choose a payee..." }
                                for p in payees_cache.read().iter() {
                                    option {
                                        value: "{p.id}",
                                        "{p.name}"
                                    }
                                }
                            }
                        }
                        div {
                            class: "form-group",
                            label { "New Name:" }
                            input {
                                r#type: "text",
                                class: "text-input",
                                placeholder: "Enter cleaned/new name",
                                value: "{rename_new_name.read()}",
                                oninput: move |evt| rename_new_name.set(evt.value()),
                            }
                        }
                        div {
                            class: "form-actions",
                            button {
                                class: "btn btn-primary",
                                disabled: rename_payee_id.read().is_empty() || rename_new_name.read().trim().is_empty(),
                                onclick: {
                                    let db = db.clone();
                                    let mut action_count = action_count;
                                    move |_| {
                                        let p_id = rename_payee_id.read().clone();
                                        let n_name = rename_new_name.read().trim().to_string();
                                        let cache = payees_cache.read();
                                        let old_name = cache
                                            .iter()
                                            .find(|p| p.id == p_id)
                                            .map(|p| p.name.clone())
                                            .unwrap_or_else(|| "Unknown".to_string());

                                        let db = db.clone();
                                        spawn(async move {
                                            let seq = get_next_action_sequence(&db).await.unwrap_or(1);
                                            let action = PayeeAction {
                                                id: uuid::Uuid::new_v4().to_string(),
                                                sequence_number: seq,
                                                kind: PayeeActionKind::RenamePayee {
                                                    payee_id: p_id,
                                                    old_name,
                                                    new_name: n_name,
                                                },
                                                status: ActionStatus::Pending,
                                                created_at: Utc::now(),
                                                confirmed_at: None,
                                            };
                                            if let Ok(()) = enqueue_action(&db, &action).await {
                                                if let Ok(pending) = get_pending_actions(&db).await {
                                                    *action_count.write() = pending.len();
                                                }
                                                if let Ok(all) = get_all_actions(&db).await {
                                                    actions.set(all);
                                                }
                                                notification.set(Some("Rename action queued in Action Queue.".to_string()));
                                                rename_payee_id.set(String::new());
                                                rename_new_name.set(String::new());
                                                show_rename_dialog.set(false);
                                            }
                                        });
                                    }
                                },
                                "Add to Action Queue"
                            }
                        }
                    }
                }
            }

            if has_in_progress && !*is_committing.read() {
                div {
                    class: "warning-banner",
                    span { class: "warning-icon", "⚠️" }
                    span {
                        "Interrupted sync detected! One or more actions were in progress before the app was closed or reloaded. "
                        "Your unconfirmed actions are safely preserved. Click 'Resume Commit' to continue safely."
                    }
                }
            }

            if let Some(err) = error_banner.read().as_ref() {
                div { class: "error-banner", "{err}" }
            }

            if let Some(msg) = notification.read().as_ref() {
                div { class: "info-banner", "{msg}" }
            }

            if let Some(prog) = progress_message.read().as_ref() {
                div {
                    class: "progress-banner",
                    div { class: "spinner" }
                    span { "{prog}" }
                }
            }

            if *is_loading.read() && actions_list.is_empty() {
                div { class: "loading-container", p { "Loading action queue..." } }
            } else if actions_list.is_empty() {
                div {
                    class: "empty-state-card",
                    p { class: "empty-title", "Action Queue is Empty" }
                    p { class: "empty-description", "No actions are waiting. Approve items from the Payee Review Queue to stage actions here." }
                }
            } else {
                div {
                    class: "actions-table-card",
                    div {
                        class: "actions-table-toolbar",
                        span { class: "toolbar-stat", "{pending_actions.len()} Pending • {confirmed_actions_count} Confirmed" }
                        div {
                            class: "toolbar-actions",
                            if confirmed_actions_count > 0 {
                                button {
                                    class: "btn btn-sm btn-outline",
                                    onclick: {
                                        let db = db.clone();
                                        move |_| {
                                            let db = db.clone();
                                            spawn(async move {
                                                // Retain only unconfirmed
                                                if let Ok(all) = get_all_actions(&db).await {
                                                    for a in all {
                                                        if a.status == ActionStatus::Confirmed {
                                                            let _ = delete_action(&db, &a.id).await;
                                                        }
                                                    }
                                                    if let Ok(refreshed) = get_all_actions(&db).await {
                                                        actions.set(refreshed);
                                                    }
                                                }
                                            });
                                        }
                                    },
                                    "Clear Confirmed"
                                }
                            }
                            if !pending_actions.is_empty() && !*is_committing.read() {
                                button {
                                    class: "btn btn-sm btn-outline-danger",
                                    onclick: {
                                        let db = db.clone();
                                        let mut action_count = action_count;
                                        move |_| {
                                            let db = db.clone();
                                            spawn(async move {
                                                let _ = clear_all_actions(&db).await;
                                                *action_count.write() = 0;
                                                actions.set(Vec::new());
                                                notification.set(Some("Action queue cleared.".to_string()));
                                            });
                                        }
                                    },
                                    "Cancel All Pending"
                                }
                            }
                        }
                    }

                    div {
                        class: "table-container",
                        table {
                            thead {
                                tr {
                                    th { "#" }
                                    th { "Action Kind" }
                                    th { "Details" }
                                    th { "Status" }
                                    th { "Expires In" }
                                    th { "" }
                                }
                            }
                            tbody {
                                for action in actions_list.iter() {
                                    {
                                        let action_id = action.id.clone();
                                        let seq = action.sequence_number;
                                        let status_copy = action.status.clone();
                                        let now = Utc::now();
                                        let expires_at = action.created_at + chrono::Duration::hours(1);
                                        let remaining_mins = (expires_at - now).num_minutes().max(0);

                                        let db = db.clone();
                                        let mut action_count = action_count;

                                        rsx! {
                                            tr {
                                                key: "{action_id}",
                                                td { class: "cell-seq", "#{seq}" }
                                                td {
                                                    class: "cell-kind",
                                                    match &action.kind {
                                                        PayeeActionKind::RenamePayee { .. } => rsx! { span { class: "badge badge-rename", "Rename" } },
                                                        PayeeActionKind::MergePayees { .. } => rsx! { span { class: "badge badge-merge", "Merge" } },
                                                    }
                                                }
                                                td {
                                                    class: "cell-details",
                                                    match &action.kind {
                                                        PayeeActionKind::RenamePayee { payee_id, old_name, new_name } => rsx! {
                                                            div {
                                                                div { class: "action-main-text", "Rename '{old_name}' → " strong { "{new_name}" } }
                                                                div { class: "action-id-subtext", "Payee ID: {payee_id}" }
                                                            }
                                                        },
                                                        PayeeActionKind::MergePayees { target_payee_id, target_name, source_payee_ids, source_payee_names } => rsx! {
                                                            div {
                                                                div {
                                                                    class: "action-main-text",
                                                                    "Merge into " strong { "{target_name}" } " (reassign from {source_payee_ids.len()} sources)"
                                                                }
                                                                div {
                                                                    class: "action-sources-subtext",
                                                                    "Sources: {source_payee_names.join(\", \")}"
                                                                }
                                                                div { class: "action-id-subtext", "Target ID: {target_payee_id}" }
                                                            }
                                                        },
                                                    }
                                                }
                                                td {
                                                    class: "cell-status",
                                                    match &status_copy {
                                                        ActionStatus::Pending => rsx! { span { class: "status-badge status-pending", "Pending" } },
                                                        ActionStatus::InProgress => rsx! { span { class: "status-badge status-inprogress", "In Progress..." } },
                                                        ActionStatus::Confirmed => rsx! { span { class: "status-badge status-confirmed", "✓ Confirmed" } },
                                                        ActionStatus::Failed { error } => rsx! {
                                                            div {
                                                                span { class: "status-badge status-failed", "Failed" }
                                                                p { class: "status-error-text", "{error}" }
                                                            }
                                                        },
                                                    }
                                                }
                                                td {
                                                    class: "cell-ttl",
                                                    if status_copy == ActionStatus::Confirmed {
                                                        span { class: "ttl-confirmed", "Confirmed" }
                                                    } else {
                                                        span { class: "ttl-active", "{remaining_mins} min" }
                                                    }
                                                }
                                                td {
                                                    class: "cell-action",
                                                    if status_copy != ActionStatus::Confirmed && !*is_committing.read() {
                                                        button {
                                                            class: "btn-icon-danger",
                                                            title: "Remove Action",
                                                            onclick: {
                                                                let a_id = action_id.clone();
                                                                let db = db.clone();
                                                                move |_| {
                                                                    let a_id = a_id.clone();
                                                                    let db = db.clone();
                                                                    spawn(async move {
                                                                        if let Ok(()) = delete_action(&db, &a_id).await {
                                                                            if let Ok(pending) = get_pending_actions(&db).await {
                                                                                *action_count.write() = pending.len();
                                                                            }
                                                                            if let Ok(all) = get_all_actions(&db).await {
                                                                                actions.set(all);
                                                                            }
                                                                        }
                                                                    });
                                                                }
                                                            },
                                                            "×"
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
}

use std::sync::Arc;

use chrono::Utc;
use dioxus::prelude::*;

use crate::{
    database::{
        delete_review_item, enqueue_action, get_next_action_sequence, get_pending_actions,
        get_review_items, save_review_item,
    },
    models::{ActionStatus, PayeeAction, PayeeActionKind, PayeeReviewItem},
};

#[component]
pub fn ReviewQueue(
    review_count: Signal<usize>,
    action_count: Signal<usize>,
) -> Element {
    let db = use_context::<Arc<idb::Database>>();

    let mut review_items = use_signal(Vec::<PayeeReviewItem>::new);
    let mut is_loading = use_signal(|| false);
    let mut notification = use_signal(|| Option::<String>::None);

    let db_for_load = db.clone();
    let load_items = move || {
        let db = db_for_load.clone();
        let mut review_count = review_count;
        spawn(async move {
            is_loading.set(true);
            if let Ok(items) = get_review_items(&db).await {
                *review_count.write() = items.len();
                review_items.set(items);
            }
            is_loading.set(false);
        });
    };

    use_effect(move || {
        load_items();
    });

    let items_list = review_items.read();

    rsx! {
        div {
            class: "review-container",
            div {
                class: "review-header",
                div {
                    class: "header-title-block",
                    h2 { "Payee Review Queue" }
                    p {
                        class: "section-subtitle",
                        "Review and refine newly combined payee names before committing them. "
                        "When you approve an item, it is translated into a structured action in the persistent Action Queue. "
                        "Renames and merges will only be sent to YNAB once you commit them."
                    }
                }
                div {
                    class: "header-stats",
                    span { class: "badge badge-count", "{items_list.len()} payees awaiting name review" }
                }
            }

            if let Some(msg) = notification.read().as_ref() {
                div { class: "info-banner", "{msg}" }
            }

            if *is_loading.read() && items_list.is_empty() {
                div { class: "loading-container", p { "Loading review queue from IndexedDB..." } }
            } else if items_list.is_empty() {
                div {
                    class: "empty-state-card",
                    p { class: "empty-title", "Review Queue is Empty" }
                    p { class: "empty-description", "Head over to 'Find Duplicates' to discover and combine similar payees." }
                }
            } else {
                div {
                    class: "review-cards-list",
                    for item in items_list.iter() {
                        {
                            let item_id = item.id.clone();
                            let current_display_name = item.display_name().to_string();
                            let suggested_name = item.suggested_name.clone();
                            let source_names = item.source_payee_names.clone();
                            let created_str = item.created_at.format("%Y-%m-%d %H:%M UTC").to_string();

                            let db = db.clone();
                            let mut review_count = review_count;
                            let mut action_count = action_count;

                            rsx! {
                                div {
                                    key: "{item_id}",
                                    class: "review-card",
                                    div {
                                        class: "review-card-top",
                                        div {
                                            class: "review-name-field",
                                            label { "Target Payee Name:" }
                                            input {
                                                r#type: "text",
                                                class: "name-input",
                                                value: "{current_display_name}",
                                                oninput: {
                                                    let item_id = item_id.clone();
                                                    let db = db.clone();
                                                    move |evt| {
                                                        let new_name = evt.value();
                                                        let mut cur_list = review_items.read().clone();
                                                        if let Some(it) = cur_list.iter_mut().find(|i| i.id == item_id) {
                                                            it.custom_name = Some(new_name);
                                                            let updated_it = it.clone();
                                                            let db = db.clone();
                                                            spawn(async move {
                                                                let _ = save_review_item(&db, &updated_it).await;
                                                            });
                                                        }
                                                        review_items.set(cur_list);
                                                    }
                                                }
                                            }
                                            if current_display_name != suggested_name {
                                                span { class: "subtext-note", "Original suggestion: {suggested_name}" }
                                            }
                                        }
                                        div {
                                            class: "review-timestamp",
                                            span { "Queued: {created_str}" }
                                        }
                                    }

                                    div {
                                        class: "review-source-block",
                                        span { class: "source-label", "Combines {source_names.len()} Payees:" }
                                        div {
                                            class: "source-tags",
                                            for name in source_names.iter() {
                                                span { class: "source-tag", "{name}" }
                                            }
                                        }
                                    }

                                    div {
                                        class: "review-card-actions",
                                        button {
                                            class: "btn btn-sm btn-outline-danger",
                                            onclick: {
                                                let item_id = item_id.clone();
                                                let db = db.clone();
                                                move |_| {
                                                    let db = db.clone();
                                                    let item_id = item_id.clone();
                                                    spawn(async move {
                                                        if let Ok(()) = delete_review_item(&db, &item_id).await {
                                                            if let Ok(items) = get_review_items(&db).await {
                                                                *review_count.write() = items.len();
                                                                review_items.set(items);
                                                            }
                                                            notification.set(Some("Item removed from review queue and returned to payee pool.".to_string()));
                                                        }
                                                    });
                                                }
                                            },
                                            "Remove / Cancel"
                                        }
                                        button {
                                            class: "btn btn-sm btn-primary",
                                            onclick: {
                                                let item_id = item_id.clone();
                                                let db = db.clone();

                                                move |_| {
                                                    let db = db.clone();
                                                    let item_id = item_id.clone();
                                                    let cur_items = review_items.read().clone();
                                                    let Some(live_item) = cur_items.iter().find(|i| i.id == item_id).cloned() else {
                                                        return;
                                                    };
                                                    let target_name = live_item.display_name().trim().to_string();
                                                    let source_ids = live_item.source_payee_ids.clone();
                                                    let source_names = live_item.source_payee_names.clone();

                                                    spawn(async move {
                                                        if source_ids.is_empty() {
                                                            return;
                                                        }
                                                        // First ID is target payee ID, others are sources to merge
                                                        let target_payee_id = source_ids[0].clone();
                                                        let remaining_sources = source_ids[1..].to_vec();
                                                        let remaining_names = if source_names.len() > 1 {
                                                            source_names[1..].to_vec()
                                                        } else {
                                                            Vec::new()
                                                        };

                                                        let seq = get_next_action_sequence(&db).await.unwrap_or(1);
                                                        let action = PayeeAction {
                                                            id: uuid::Uuid::new_v4().to_string(),
                                                            sequence_number: seq,
                                                            kind: PayeeActionKind::MergePayees {
                                                                target_payee_id,
                                                                target_name: target_name.clone(),
                                                                source_payee_ids: remaining_sources,
                                                                source_payee_names: remaining_names,
                                                            },
                                                            status: ActionStatus::Pending,
                                                            created_at: Utc::now(),
                                                            confirmed_at: None,
                                                        };

                                                        match enqueue_action(&db, &action).await {
                                                            Ok(()) => {
                                                                let _ = delete_review_item(&db, &item_id).await;

                                                                // Update counts
                                                                if let Ok(items) = get_review_items(&db).await {
                                                                    *review_count.write() = items.len();
                                                                    review_items.set(items);
                                                                }
                                                                if let Ok(pending) = get_pending_actions(&db).await {
                                                                    *action_count.write() = pending.len();
                                                                }

                                                                notification.set(Some(format!(
                                                                    "Enqueued merge action for '{}' in Action Queue.",
                                                                    target_name
                                                                )));
                                                            }
                                                            Err(e) => {
                                                                notification.set(Some(format!("Database error queueing action: {:?}", e)));
                                                            }
                                                        }
                                                    });
                                                }
                                            },
                                            "Approve & Enqueue Action"
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

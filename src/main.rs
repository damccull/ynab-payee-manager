use std::sync::Arc;

use dioxus::prelude::*;
use wasm_bindgen::JsValue;

use ynab_payee_rs::{
    api::{fetch_and_store_payees, fetch_and_store_transactions},
    database::{create_database, get_payees, get_pending_actions, get_review_items, get_transactions},
    models::{Payee, Transaction},
    views::{ActionQueue, Duplicates, ReviewQueue},
    AuthActions, AuthActionStatus,
};

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
        #[route("/")]
        Payees {},
        #[route("/duplicates")]
        DuplicatesPage {},
        #[route("/review")]
        ReviewPage {},
        #[route("/actions")]
        ActionsPage {},
        #[route("/transactions")]
        Transactions {},
}

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/main.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

/// Local in-memory cache of payees pulled from the API; don't update this too often
static PAYEES_CACHE: GlobalSignal<Vec<Payee>> = GlobalSignal::new(|| Vec::new());
static PAYEES_KNOWLEDGE: GlobalSignal<u64> = GlobalSignal::new(|| 0);
static TRANSACTIONS_CACHE: GlobalSignal<Vec<Transaction>> = GlobalSignal::new(|| Vec::new());
static TRANSACTIONS_KNOWLEDGE: GlobalSignal<u64> = GlobalSignal::new(|| 0);
/// The budget ID to use when talking to the API.
/// "default" routes to the budget selected by the user on the YNAB OAuth authorization page.
static BUDGET: GlobalSignal<&str> = GlobalSignal::new(|| "default");
static REVIEW_QUEUE_COUNT: GlobalSignal<usize> = GlobalSignal::new(|| 0);
static ACTION_QUEUE_COUNT: GlobalSignal<usize> = GlobalSignal::new(|| 0);

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    info!("ATTENTION: Some of the debug code may display your YNAB oauth2 token.");
    info!("This is only stored in your browser and never sent to any server");
    info!("other than YNAB's API. Nobody but you can see your logs, or your");
    info!("YNAB oauth2 token.");

    let callback_access_token = use_hook(get_token_from_hash);

    let db_resource = use_resource(move || async move {
        let db = create_database().await.expect("unable to open database");
        Arc::new(db)
    });

    let db_handle = db_resource.read().as_ref().cloned();

    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        document::Link { rel: "stylesheet", href: MAIN_CSS }

        match db_handle {
            Some(db) => rsx! {
                AuthenticatedApp {
                    db: DbHandle(db),
                    callback_access_token: callback_access_token.clone(),
                }
            },
            None => rsx! {
                div {
                    class: "login-container",
                    p { "Initializing database..." }
                }
            },
        }
    }
}

#[derive(Clone)]
struct DbHandle(Arc<idb::Database>);

impl PartialEq for DbHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[component]
fn AuthenticatedApp(db: DbHandle, callback_access_token: Option<String>) -> Element {
    let db = db.0;
    use_context_provider(|| db.clone());
    debug!("added database handle to context");

    let initial_token = callback_access_token.clone().map(Into::into);
    let db_for_auth = db.clone();
    let auth_actions = use_hook(move || {
        let aa = AuthActions::new(db_for_auth.clone(), initial_token);
        aa
    });
    use_context_provider(|| auth_actions.clone());
    debug!("added auth_actions handle to context");

    let db_for_counts = db.clone();
    use_effect(move || {
        let db = db_for_counts.clone();
        spawn(async move {
            if let Ok(items) = get_review_items(&db).await {
                *REVIEW_QUEUE_COUNT.write() = items.len();
            }
            if let Ok(pending) = get_pending_actions(&db).await {
                *ACTION_QUEUE_COUNT.write() = pending.len();
            }
        });
    });

    let do_login = move |_| {
        let auth_url = "https://app.ynab.com/oauth/authorize?client_id=Dszs-ubBjiUIP9pEHY7znF9LI_GrFdWSErx8zj84JWA&redirect_uri=http%3A%2F%2Flocalhost%3A8080&response_type=token&scope=";

        // Send user to ynab login page
        if let Some(window) = web_sys::window() {
            let _ = window.location().set_href(auth_url);
        }
    };

    let token = (auth_actions.token)();
    debug!("token in main: {:#?}", &token);
    match token {
        Some(token) if token.is_valid() => {
            rsx! {
                Router::<Route> {}
            }
        }
        _ => {
            match &*auth_actions.status.read() {
                AuthActionStatus::Loading => rsx! {
                    div {
                        class: "login-container",
                        p { "Loading authentication..." }
                    }
                },
                _ => rsx! {
                    div {
                        class: "login-container",
                        h2 { "YNAB Payee Manager" }
                        p { "Please authenticate with YNAB to manage payees in your selected budget." }
                        button {
                            id: "login",
                            onclick: do_login,
                            "Login with YNAB"
                        }
                    }
                },
            }
        }
    }
}

/// Payees Page
#[component]
fn Payees() -> Element {
    let db = use_context::<Arc<idb::Database>>();
    let auth = use_context::<AuthActions>();
    let token = (auth.token)();

    let mut is_loading = use_signal(|| false);
    let mut error_msg = use_signal(|| Option::<String>::None);

    let db_for_load = db.clone();
    let token_for_load = token.clone();

    // Load initial data: load cached from IndexedDB, and if cache is empty, fetch from API
    use_resource(move || {
        let db = db_for_load.clone();
        let token = token_for_load.clone();
        async move {
            // First load payees from IndexedDB
            if let Ok((payees, knowledge)) = get_payees(&db).await {
                if !payees.is_empty() {
                    *PAYEES_CACHE.write() = payees;
                    *PAYEES_KNOWLEDGE.write() = knowledge;
                    debug!("Loaded payees from IndexedDB");
                }
            }
            // Also load transactions from IndexedDB
            if let Ok((txs, knowledge)) = get_transactions(&db).await {
                if !txs.is_empty() {
                    *TRANSACTIONS_CACHE.write() = txs;
                    *TRANSACTIONS_KNOWLEDGE.write() = knowledge;
                    debug!("Loaded transactions from IndexedDB");
                }
            }

            // If caches are empty and we have a valid token, fetch automatically
            if let Some(ref t) = token {
                if t.is_valid() {
                    let budget = *BUDGET.read();
                    if PAYEES_CACHE.read().is_empty() {
                        is_loading.set(true);
                        error_msg.set(None);
                        if let Ok((payees, k)) = fetch_and_store_payees(&db, &t.to_string(), budget).await {
                            *PAYEES_CACHE.write() = payees;
                            *PAYEES_KNOWLEDGE.write() = k;
                        }
                        is_loading.set(false);
                    }
                    if TRANSACTIONS_CACHE.read().is_empty() {
                        let _ = fetch_and_store_transactions(&db, &t.to_string(), budget).await;
                    }
                }
            }
        }
    });

    let db_for_refresh = db.clone();
    let token_for_refresh = token.clone();
    let refresh_payees = move |_| {
        let db = db_for_refresh.clone();
        let token = token_for_refresh.clone();
        spawn(async move {
            if let Some(ref t) = token {
                if t.is_valid() {
                    is_loading.set(true);
                    error_msg.set(None);
                    let budget = *BUDGET.read();

                    match fetch_and_store_payees(&db, &t.to_string(), budget).await {
                        Ok((payees, k)) => {
                            *PAYEES_CACHE.write() = payees;
                            *PAYEES_KNOWLEDGE.write() = k;
                        }
                        Err(e) => error_msg.set(Some(e)),
                    }

                    if let Ok((txs, k, _)) = fetch_and_store_transactions(&db, &t.to_string(), budget).await {
                        *TRANSACTIONS_CACHE.write() = txs;
                        *TRANSACTIONS_KNOWLEDGE.write() = k;
                    }

                    is_loading.set(false);
                }
            }
        });
    };

    let payees_cache = PAYEES_CACHE.read();
    let mut display_payees: Vec<_> = payees_cache
        .iter()
        .filter(|payee| {
            payee
                .transfer_account_id
                .as_deref()
                .is_none_or(|id| id.trim().is_empty())
        })
        .collect();
    display_payees.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    let total_payees = payees_cache.len();
    let hidden_transfer_payees = total_payees - display_payees.len();
    let hidden_note = match hidden_transfer_payees {
        0 => String::new(),
        1 => " (1 transfer account payee hidden)".to_string(),
        n => format!(" ({n} transfer account payees hidden)"),
    };

    rsx! {
        div {
            class: "payees-header",
            button {
                id: "get_payees",
                class: "btn btn-primary",
                onclick: refresh_payees,
                disabled: *is_loading.read(),
                if *is_loading.read() { "Syncing Payees..." } else { "Sync Payees" }
            }
            if let Some(err) = error_msg.read().as_ref() {
                p { class: "error-banner", "Error: {err}" }
            }
        }
        div {
            class: "stats-bar",
            span { "Total Payees: " strong { "{total_payees}" } }
            if hidden_transfer_payees > 0 {
                span { class: "stats-hidden-badge", "{hidden_note}" }
            }
            span { class: "stats-divider", "|" }
            span { "Payee Knowledge: " code { "{PAYEES_KNOWLEDGE.read()}" } }
            span { class: "stats-divider", "|" }
            span { "Transactions Cached: " strong { "{TRANSACTIONS_CACHE.read().len()}" } }
        }

        if *is_loading.read() && PAYEES_CACHE.read().is_empty() {
            p { "Loading payees from YNAB..." }
        } else {
            div {
                class: "table-container",
                table {
                    thead {
                        tr {
                            th { "Name" }
                            th { "Transfer Account ID" }
                            th { "Deleted" }
                        }
                    }
                    tbody {
                        for payee in display_payees {
                            tr {
                                td {
                                    "data-fieldname": "name",
                                    div {
                                        class: "payee-name-container",
                                        style: "display: flex; flex-direction: column; gap: 2px;",
                                        div {
                                            class: "payee-name",
                                            style: "font-weight: 600;",
                                            "{payee.name}"
                                        }
                                        div {
                                            class: "payee-id-subtext",
                                            "data-fieldname": "id",
                                            style: "font-style: italic; font-size: 11px; color: #8b949e; font-family: ui-monospace, SFMono-Regular, monospace; margin-top: 2px;",
                                            "{payee.id}"
                                        }
                                    }
                                },
                                td {"data-fieldname": "transfer_acct_id", "{payee.transfer_account_id.as_deref().unwrap_or(\"None\")}"},
                                td {
                                    "data-fieldname": "deleted",
                                    span {
                                        class: if payee.deleted { "status-badge status-deleted" } else { "status-badge status-active" },
                                        "{payee.deleted}"
                                    }
                                },
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Duplicates Page
#[component]
fn DuplicatesPage() -> Element {
    rsx! {
        Duplicates {
            payees_cache: PAYEES_CACHE.signal(),
            review_count: REVIEW_QUEUE_COUNT.signal(),
        }
    }
}

/// Review Page
#[component]
fn ReviewPage() -> Element {
    rsx! {
        ReviewQueue {
            review_count: REVIEW_QUEUE_COUNT.signal(),
            action_count: ACTION_QUEUE_COUNT.signal(),
        }
    }
}

/// Actions Page
#[component]
fn ActionsPage() -> Element {
    rsx! {
        ActionQueue {
            budget_id: (*BUDGET.read()).to_string(),
            action_count: ACTION_QUEUE_COUNT.signal(),
            payees_cache: PAYEES_CACHE.signal(),
        }
    }
}

/// Transaction Page
#[component]
fn Transactions() -> Element {
    let db = use_context::<Arc<idb::Database>>();
    let auth = use_context::<AuthActions>();
    let token = (auth.token)();

    let mut is_syncing = use_signal(|| false);
    let mut error_msg = use_signal(|| Option::<String>::None);

    let db_for_sync = db.clone();
    let token_for_sync = token.clone();
    let sync_transactions = move |_| {
        let db = db_for_sync.clone();
        let token = token_for_sync.clone();
        spawn(async move {
            if let Some(ref t) = token {
                if t.is_valid() {
                    is_syncing.set(true);
                    error_msg.set(None);
                    let budget = *BUDGET.read();
                    match fetch_and_store_transactions(&db, &t.to_string(), budget).await {
                        Ok((txs, k, _)) => {
                            *TRANSACTIONS_CACHE.write() = txs;
                            *TRANSACTIONS_KNOWLEDGE.write() = k;
                        }
                        Err(e) => error_msg.set(Some(e)),
                    }
                    is_syncing.set(false);
                }
            }
        });
    };

    let txs = TRANSACTIONS_CACHE.read();

    rsx! {
        div {
            class: "payees-header",
            button {
                class: "btn btn-primary",
                onclick: sync_transactions,
                disabled: *is_syncing.read(),
                if *is_syncing.read() { "Syncing Transactions..." } else { "Sync Transactions" }
            }
            if let Some(err) = error_msg.read().as_ref() {
                p { class: "error-banner", "Error: {err}" }
            }
        }
        div {
            class: "stats-bar",
            span { "Total Transactions: " strong { "{txs.len()}" } }
            span { class: "stats-divider", "|" }
            span { "Transaction Knowledge: " code { "{TRANSACTIONS_KNOWLEDGE.read()}" } }
        }

        if txs.is_empty() {
            div {
                class: "empty-state-card",
                p { class: "empty-title", "No Transactions Cached" }
                p { class: "empty-description", "Transactions will load automatically from YNAB." }
            }
        } else {
            div {
                class: "table-container",
                table {
                    thead {
                        tr {
                            th { "Date" }
                            th { "Amount" }
                            th { "Payee Name" }
                            th { "Category" }
                            th { "Memo" }
                            th { "Cleared" }
                            th { "Approved" }
                        }
                    }
                    tbody {
                        for tx in txs.iter() {
                            {
                                let amount_dollars = (tx.amount as f64) / 1000.0;
                                let amount_str = format!("${:.2}", amount_dollars);
                                let payee_name = tx.payee_name.as_deref().unwrap_or("None");
                                let category_name = tx.category_name.as_deref().unwrap_or("Uncategorized");
                                let memo = tx.memo.as_deref().unwrap_or("");

                                rsx! {
                                    tr {
                                        key: "{tx.id}",
                                        td { "data-fieldname": "date", "{tx.date}" }
                                        td { "data-fieldname": "amount", "{amount_str}" }
                                        td { "data-fieldname": "payee_name", "{payee_name}" }
                                        td { "data-fieldname": "category_name", "{category_name}" }
                                        td { "data-fieldname": "memo", "{memo}" }
                                        td { "data-fieldname": "cleared", "{tx.cleared}" }
                                        td {
                                            "data-fieldname": "approved",
                                            span {
                                                class: if tx.approved { "status-badge status-active" } else { "status-badge status-pending" },
                                                if tx.approved { "Yes" } else { "No" }
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

/// Shared navbar component.
#[component]
fn Navbar() -> Element {
    let review_cnt = *REVIEW_QUEUE_COUNT.read();
    let action_cnt = *ACTION_QUEUE_COUNT.read();

    rsx! {
        header {
            class: "app-header",
            div {
                class: "header-brand",
                h1 { "YNAB Payee Manager" }
            }
            nav {
                id: "navbar",
                Link {
                    to: Route::Payees {},
                    class: "nav-link",
                    "Payees"
                }
                Link {
                    to: Route::DuplicatesPage {},
                    class: "nav-link",
                    "Find Duplicates"
                }
                Link {
                    to: Route::ReviewPage {},
                    class: "nav-link nav-link-badge",
                    span { "Review Names" }
                    if review_cnt > 0 {
                        span { class: "nav-badge", "{review_cnt}" }
                    }
                }
                Link {
                    to: Route::ActionsPage {},
                    class: "nav-link nav-link-badge",
                    span { "Action Queue" }
                    if action_cnt > 0 {
                        span { class: "nav-badge nav-badge-action", "{action_cnt}" }
                    }
                }
                Link {
                    to: Route::Transactions {},
                    class: "nav-link",
                    "Transactions"
                }
            }
        }
        div {
            class: "warning-banner",
            span { class: "warning-icon", "⚠️" }
            span {
                "Warning: This app will directly affect your YNAB budget. "
                "It will not make a backup for you. "
                "Please make a backup yourself before using this."
            }
        }

        Outlet::<Route> {}
    }
}

fn get_token_from_hash() -> Option<String> {
    let window = web_sys::window()?;
    let hash = window.location().hash().ok()?;

    debug!("hash string: {:#?}", &hash);

    if hash.is_empty() {
        return None;
    }

    let fragment = hash.trim_start_matches('#');
    let params = web_sys::UrlSearchParams::new_with_str(fragment).ok()?;
    let token = params.get("access_token");

    if token.is_some() {
        if let Ok(history) = window.history() {
            let _ = history.replace_state_with_url(&JsValue::NULL, "", Some("/"));
        }
    }

    token
}

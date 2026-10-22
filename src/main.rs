use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use dioxus::prelude::*;
use wasm_bindgen::JsValue;

use ynab_payee_rs::{
    api::{fetch_and_store_payees, fetch_and_store_transactions},
    database::{
        create_database, get_payees, get_pending_actions, get_review_items, get_transactions,
        nuke_database, search_transactions_paged, ProcessedTransaction, TransferPairRole,
    },
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
static IS_SYNCING: GlobalSignal<bool> = GlobalSignal::new(|| false);
static SYNC_ERROR: GlobalSignal<Option<String>> = GlobalSignal::new(|| None);

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
    let mut show_zero_only = use_signal(|| false);
    let mut struck_payees = use_signal(|| HashSet::<String>::new());

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

    let tx_cache = TRANSACTIONS_CACHE.read();
    let mut tx_counts: HashMap<&str, usize> = HashMap::new();
    for tx in tx_cache.iter() {
        if !tx.deleted {
            if let Some(ref pid) = tx.payee_id {
                *tx_counts.entry(pid.as_str()).or_insert(0) += 1;
            }
        }
    }

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

    let zero_tx_count = display_payees
        .iter()
        .filter(|p| tx_counts.get(p.id.as_str()).copied().unwrap_or(0) == 0)
        .count();

    if *show_zero_only.read() {
        display_payees.retain(|payee| {
            tx_counts.get(payee.id.as_str()).copied().unwrap_or(0) == 0
        });
    }

    rsx! {
        if let Some(err) = error_msg.read().as_ref() {
            div {
                class: "error-banner",
                style: "margin-bottom: 14px;",
                "Error: {err}"
            }
        }
        div {
            class: "stats-bar",
            span { "Total Payees: " strong { "{total_payees}" } }
            if hidden_transfer_payees > 0 {
                span { class: "stats-hidden-badge", "{hidden_note}" }
            }
            span { class: "stats-divider", "|" }
            span { "Zero Tx Payees: " strong { "{zero_tx_count}" } }
            span { class: "stats-divider", "|" }
            span { "Payee Knowledge: " code { "{PAYEES_KNOWLEDGE.read()}" } }
            span { class: "stats-divider", "|" }
            span { "Transactions Cached: " strong { "{tx_cache.len()}" } }
            div {
                style: "margin-left: auto;",
                button {
                    class: if *show_zero_only.read() { "btn-toggle active" } else { "btn-toggle" },
                    onclick: move |_| {
                        let current = *show_zero_only.read();
                        show_zero_only.set(!current);
                    },
                    if *show_zero_only.read() {
                        "✓ Showing Zero Tx Only ({display_payees.len()})"
                    } else {
                        "Show Zero Tx Only ({zero_tx_count})"
                    }
                }
            }
        }

        if *is_loading.read() && PAYEES_CACHE.read().is_empty() {
            p { "Loading payees from YNAB..." }
        } else if display_payees.is_empty() {
            div {
                class: "empty-state-card",
                p {
                    class: "empty-title",
                    if *show_zero_only.read() {
                        "No Payees with Zero Transactions"
                    } else {
                        "No Payees Found"
                    }
                }
                if *show_zero_only.read() {
                    p {
                        class: "empty-description",
                        "All active payees currently have at least one transaction attributed to them."
                    }
                }
            }
        } else {
            div {
                class: "table-container",
                table {
                    thead {
                        tr {
                            th { style: "width: 44px; text-align: center;", "" }
                            th { "Name" }
                            th { style: "text-align: right; width: 120px;", "Transactions" }
                            th { "Transfer Account ID" }
                            th { "Deleted" }
                        }
                    }
                    tbody {
                        for payee in display_payees {
                            {
                                let is_struck = struck_payees.read().contains(&payee.id);
                                let count = tx_counts.get(payee.id.as_str()).copied().unwrap_or(0);
                                rsx! {
                                    tr {
                                        key: "{payee.id}",
                                        class: if is_struck { "payee-row-struck" } else { "" },
                                        td {
                                            style: "width: 44px; text-align: center;",
                                            input {
                                                r#type: "checkbox",
                                                class: "payee-strike-checkbox",
                                                title: "Strike out / gray out payee",
                                                checked: is_struck,
                                                onchange: {
                                                    let id = payee.id.clone();
                                                    move |_| {
                                                        let mut set = struck_payees.write();
                                                        if set.contains(&id) {
                                                            set.remove(&id);
                                                        } else {
                                                            set.insert(id.clone());
                                                        }
                                                    }
                                                }
                                            }
                                        }
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
                                        }
                                        td {
                                            "data-fieldname": "tx_count",
                                            style: "text-align: right; font-family: ui-monospace, SFMono-Regular, monospace; font-weight: 600;",
                                            span {
                                                class: if count == 0 { "tx-count-zero" } else { "tx-count-badge" },
                                                "{count}"
                                            }
                                        }
                                        td {
                                            "data-fieldname": "transfer_acct_id",
                                            "{payee.transfer_account_id.as_deref().unwrap_or(\"None\")}"
                                        }
                                        td {
                                            "data-fieldname": "deleted",
                                            span {
                                                class: if payee.deleted { "status-badge status-deleted" } else { "status-badge status-active" },
                                                "{payee.deleted}"
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

const TX_PAGE_SIZE: usize = 50;

#[derive(Clone, PartialEq, Default, Debug)]
struct TransactionSlot {
    visible: bool,
    date: String,
    outflow_str: String,
    has_outflow: bool,
    inflow_str: String,
    has_inflow: bool,
    is_transfer: bool,
    is_pair_first: bool,
    is_pair_second: bool,
    payee_name: String,
    category_name: String,
    memo: String,
    cleared: String,
    approved: bool,
}

fn update_slot_pool(pool: &mut Signal<Vec<TransactionSlot>>, items: &[ProcessedTransaction]) {
    let mut guard = pool.write();
    for i in 0..TX_PAGE_SIZE {
        let slot = &mut guard[i];
        if i < items.len() {
            let tx = &items[i];
            slot.visible = true;
            slot.date.clear();
            slot.date.push_str(&tx.date);

            use std::fmt::Write;

            slot.outflow_str.clear();
            if let Some(outflow) = tx.outflow_milliunits {
                slot.has_outflow = true;
                let _ = write!(&mut slot.outflow_str, "${:.2}", (outflow as f64) / 1000.0);
            } else {
                slot.has_outflow = false;
                slot.outflow_str.push('-');
            }

            slot.inflow_str.clear();
            if let Some(inflow) = tx.inflow_milliunits {
                slot.has_inflow = true;
                let _ = write!(&mut slot.inflow_str, "${:.2}", (inflow as f64) / 1000.0);
            } else {
                slot.has_inflow = false;
                slot.inflow_str.push('-');
            }

            slot.is_transfer = tx.is_transfer;
            slot.is_pair_first = tx.pair_role == TransferPairRole::First;
            slot.is_pair_second = tx.pair_role == TransferPairRole::Second;

            slot.payee_name.clear();
            if let Some(ref p) = tx.payee_name {
                slot.payee_name.push_str(p);
            }

            slot.category_name.clear();
            if let Some(ref c) = tx.category_name {
                slot.category_name.push_str(c);
            }

            slot.memo.clear();
            if let Some(ref m) = tx.memo {
                slot.memo.push_str(m);
            }

            slot.cleared.clear();
            slot.cleared.push_str(&tx.cleared);

            slot.approved = tx.approved;
        } else {
            slot.visible = false;
        }
    }
}

#[component]
fn PagerBar(
    page: usize,
    pages: usize,
    count: usize,
    is_loading: bool,
    start_item: usize,
    end_item: usize,
    on_page_change: EventHandler<usize>,
) -> Element {
    rsx! {
        div {
            class: "pagination-bar bottom-pagination-bar sticky-bottom",
            div {
                class: "pagination-info",
                "Showing {start_item}–{end_item} of {count} transactions"
                if is_loading {
                    span { style: "margin-left: 8px; color: #58a6ff;", "(Loading...)" }
                }
            }
            div {
                class: "pagination-controls",
                button {
                    class: "btn-page",
                    disabled: page == 0 || is_loading,
                    title: "First page",
                    onclick: move |_| on_page_change.call(0),
                    "«"
                }
                button {
                    class: "btn-page",
                    disabled: page == 0 || is_loading,
                    title: "Previous page",
                    onclick: move |_| {
                        if page > 0 {
                            on_page_change.call(page - 1);
                        }
                    },
                    "‹"
                }
                span {
                    class: "pagination-page-indicator",
                    "Page {page + 1} of {pages}"
                }
                button {
                    class: "btn-page",
                    disabled: page + 1 >= pages || is_loading,
                    title: "Next page",
                    onclick: move |_| {
                        if page + 1 < pages {
                            on_page_change.call(page + 1);
                        }
                    },
                    "›"
                }
                button {
                    class: "btn-page",
                    disabled: page + 1 >= pages || is_loading,
                    title: "Last page",
                    onclick: move |_| {
                        if pages > 0 {
                            on_page_change.call(pages - 1);
                        }
                    },
                    "»"
                }
            }
        }
    }
}

/// Transaction Page
#[component]
fn Transactions() -> Element {
    let db = use_context::<Arc<idb::Database>>();

    let mut row_pool = use_signal(|| vec![TransactionSlot::default(); TX_PAGE_SIZE]);
    let mut current_page = use_signal(|| 0usize);
    let mut total_count = use_signal(|| 0usize);
    let mut total_pages = use_signal(|| 1usize);
    let mut search_query = use_signal(|| String::new());
    let mut is_loading = use_signal(|| false);
    let mut has_loaded = use_signal(|| false);

    let db_for_load = db.clone();
    let load_page = move |page: usize, query: String| {
        let db = db_for_load.clone();
        spawn(async move {
            is_loading.set(true);
            match search_transactions_paged(&db, &query, page, TX_PAGE_SIZE).await {
                Ok(paged) => {
                    current_page.set(paged.current_page);
                    total_count.set(paged.total_count);
                    total_pages.set(paged.total_pages);
                    update_slot_pool(&mut row_pool, &paged.items);
                    has_loaded.set(true);
                }
                Err(e) => {
                    error!("Failed to load paged transactions from db: {:?}", e);
                }
            }
            is_loading.set(false);
        });
    };

    // Initial load and auto-refresh on transaction knowledge update
    use_effect({
        let load_page = load_page.clone();
        move || {
            let _ = *TRANSACTIONS_KNOWLEDGE.read();
            let q = search_query.peek().clone();
            let p = *current_page.peek();
            load_page(p, q);
        }
    });

    let pool = row_pool.read();
    let count = *total_count.read();
    let page = *current_page.read();
    let pages = *total_pages.read();
    let query_str = search_query.read().clone();

    let start_item = if count == 0 { 0 } else { page * TX_PAGE_SIZE + 1 };
    let end_item = (start_item + TX_PAGE_SIZE - 1).min(count);

    rsx! {
        div {
            class: "tx-controls-bar",
            div {
                class: "tx-search-box",
                input {
                    r#type: "text",
                    class: "search-input",
                    placeholder: "Search transactions (payee, memo, category, amount, date)...",
                    value: "{query_str}",
                    oninput: {
                        let load_page = load_page.clone();
                        move |evt| {
                            let val = evt.value();
                            search_query.set(val.clone());
                            load_page(0, val);
                        }
                    },
                }
                if !query_str.is_empty() {
                    button {
                        class: "btn-clear-search",
                        title: "Clear search",
                        onclick: {
                            let load_page = load_page.clone();
                            move |_| {
                                search_query.set(String::new());
                                load_page(0, String::new());
                            }
                        },
                        "✕ Clear"
                    }
                }
            }
            div {
                class: "stats-bar",
                style: "margin: 0; padding: 6px 12px;",
                span { "Total Transactions: " strong { "{count}" } }
                span { class: "stats-divider", "|" }
                span { "Knowledge: " code { "{TRANSACTIONS_KNOWLEDGE.read()}" } }
            }
        }

        if !*has_loaded.read() && *is_loading.read() {
            div {
                class: "empty-state-card",
                p { class: "empty-title", "Loading transactions from database..." }
            }
        } else if count == 0 {
            div {
                class: "empty-state-card",
                p {
                    class: "empty-title",
                    if query_str.is_empty() { "No Transactions Cached" } else { "No Matching Transactions" }
                }
                p {
                    class: "empty-description",
                    if query_str.is_empty() {
                        "Transactions will load automatically from YNAB on sync."
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
                            th { "Date" }
                            th { class: "th-amount", "Outflow" }
                            th { class: "th-amount", "Inflow" }
                            th { "Payee Name" }
                            th { "Category" }
                            th { "Memo" }
                            th { "Cleared" }
                            th { "Approved" }
                        }
                    }
                    tbody {
                        for (idx, slot) in pool.iter().enumerate() {
                            tr {
                                key: "{idx}",
                                class: if slot.is_pair_first {
                                    "transfer-pair-row transfer-pair-first"
                                } else if slot.is_pair_second {
                                    "transfer-pair-row transfer-pair-second"
                                } else {
                                    ""
                                },
                                style: if !slot.visible { "display: none;" } else { "" },
                                td {
                                    "data-fieldname": "date",
                                    if slot.is_pair_first {
                                        div {
                                            class: "transfer-pair-icon",
                                            title: "account-to-account transfer",
                                            "⇄"
                                        }
                                    }
                                    "{slot.date}"
                                }
                                td {
                                    "data-fieldname": "outflow",
                                    class: if slot.has_outflow { "amount-outflow" } else { "amount-empty" },
                                    "{slot.outflow_str}"
                                }
                                td {
                                    "data-fieldname": "inflow",
                                    class: if slot.has_inflow { "amount-inflow" } else { "amount-empty" },
                                    "{slot.inflow_str}"
                                }
                                td {
                                    "data-fieldname": "payee_name",
                                    if slot.is_transfer {
                                        span {
                                            class: "transfer-label-text",
                                            title: "account-to-account transfer",
                                            "Transfer"
                                        }
                                    } else if slot.payee_name.is_empty() {
                                        span { class: "text-muted", "None" }
                                    } else {
                                        "{slot.payee_name}"
                                    }
                                }
                                td {
                                    "data-fieldname": "category_name",
                                    if slot.is_transfer {
                                        span { class: "text-muted", "-" }
                                    } else if slot.category_name.is_empty() {
                                        span { class: "text-muted", "Uncategorized" }
                                    } else {
                                        "{slot.category_name}"
                                    }
                                }
                                td { "data-fieldname": "memo", "{slot.memo}" }
                                td { "data-fieldname": "cleared", "{slot.cleared}" }
                                td {
                                    "data-fieldname": "approved",
                                    span {
                                        class: if slot.approved { "status-badge status-active" } else { "status-badge status-pending" },
                                        if slot.approved { "Yes" } else { "No" }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Bottom Pager Bar
            PagerBar {
                page,
                pages,
                count,
                is_loading: *is_loading.read(),
                start_item,
                end_item,
                on_page_change: {
                    let load_page = load_page.clone();
                    let q = query_str.clone();
                    move |new_page| load_page(new_page, q.clone())
                },
            }
        }
    }
}

/// Shared navbar component.
#[component]
fn Navbar() -> Element {
    let db = use_context::<Arc<idb::Database>>();
    let mut auth = use_context::<AuthActions>();
    let review_cnt = *REVIEW_QUEUE_COUNT.read();
    let action_cnt = *ACTION_QUEUE_COUNT.read();

    let do_sync = {
        let db = db.clone();
        let auth = auth.clone();
        move |_| {
            let db = db.clone();
            let token = (auth.token)();
            spawn(async move {
                if let Some(ref t) = token {
                    if t.is_valid() {
                        *IS_SYNCING.write() = true;
                        *SYNC_ERROR.write() = None;
                        let budget = *BUDGET.read();
                        let token_str = t.to_string();

                        let payee_res = fetch_and_store_payees(&db, &token_str, budget).await;
                        let tx_res = fetch_and_store_transactions(&db, &token_str, budget).await;

                        match (payee_res, tx_res) {
                            (Ok((payees, pk)), Ok((txs, tk, _))) => {
                                *PAYEES_CACHE.write() = payees;
                                *PAYEES_KNOWLEDGE.write() = pk;
                                *TRANSACTIONS_CACHE.write() = txs;
                                *TRANSACTIONS_KNOWLEDGE.write() = tk;
                            }
                            (Err(e), _) => {
                                *SYNC_ERROR.write() = Some(format!("Payee sync failed: {}", e));
                            }
                            (_, Err(e)) => {
                                *SYNC_ERROR.write() = Some(format!("Transaction sync failed: {}", e));
                            }
                        }

                        *IS_SYNCING.write() = false;
                    }
                }
            });
        }
    };

    let sign_out = {
        let db = db.clone();
        move |_| {
            let db = db.clone();
            spawn(async move {
                // Clear in-memory token and caches
                auth.token.set(None);
                *PAYEES_CACHE.write() = Vec::new();
                *PAYEES_KNOWLEDGE.write() = 0;
                *TRANSACTIONS_CACHE.write() = Vec::new();
                *TRANSACTIONS_KNOWLEDGE.write() = 0;
                *REVIEW_QUEUE_COUNT.write() = 0;
                *ACTION_QUEUE_COUNT.write() = 0;

                // Nuke database in IndexedDB
                let _ = nuke_database(&db).await;

                // Force a complete browser refresh at root
                if let Some(window) = web_sys::window() {
                    let _ = window.location().replace("/");
                    let _ = window.location().reload();
                }
            });
        }
    };

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
                button {
                    id: "global_sync_btn",
                    class: "btn-sync",
                    disabled: *IS_SYNCING.read(),
                    onclick: do_sync,
                    if *IS_SYNCING.read() { "Syncing..." } else { "Sync" }
                }
                button {
                    class: "btn-signout btn-outline-danger",
                    title: "Nuke database and sign out",
                    onclick: sign_out,
                    "Sign Out"
                }
            }
        }
        if let Some(err) = SYNC_ERROR.read().as_ref() {
            div {
                class: "error-banner",
                style: "margin-bottom: 14px;",
                span { "Sync error: {err}" }
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

use std::sync::Arc;

use dioxus::prelude::*;
use reqwest::header::HeaderMap;
use wasm_bindgen::JsValue;

use ynab_payee_rs::{
    database::{create_database, get_payees, replace_payees},
    models::{Payee, YnabResponse},
    AuthActions, AuthActionStatus,
};

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
        #[route("/")]
        Payees {},
        #[route("/transactions")]
        Transactions {},
}

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/main.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

/// Local in-memory cache of payees pulled from the API; don't update this too often
static PAYEES_CACHE: GlobalSignal<Vec<Payee>> = GlobalSignal::new(|| Vec::new());
static PAYEES_KNOWLEDGE: GlobalSignal<u64> = GlobalSignal::new(|| 0);
/// The budget ID to use when talking to the API.
/// "default" routes to the budget selected by the user on the YNAB OAuth authorization page.
static BUDGET: GlobalSignal<&str> = GlobalSignal::new(|| "default");

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
    let auth_actions = use_hook(move || {
        let aa = AuthActions::new(db.clone(), initial_token);
        aa
    });
    use_context_provider(|| auth_actions.clone());
    debug!("added auth_actions handle to context");

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

async fn fetch_and_store_payees(db: &idb::Database, token_str: &str) -> Result<(), String> {
    let httpclient = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    let auth_header = format!("Bearer {}", token_str)
        .parse()
        .map_err(|e| format!("Invalid auth header: {}", e))?;
    headers.insert("Authorization", auth_header);

    let budget = BUDGET.read();
    let url = format!("https://api.ynab.com/v1/budgets/{}/payees", *budget);
    let response = httpclient
        .get(&url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| format!("Network request failed: {}", e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("YNAB API error {}: {}", status, body));
    }

    let ynabresponse: YnabResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let _ = replace_payees(db, &ynabresponse.data)
        .await
        .map_err(|e| format!("Failed to store payees in IndexedDB: {:?}", e))?;

    *PAYEES_CACHE.write() = ynabresponse.data.payees;
    *PAYEES_KNOWLEDGE.write() = ynabresponse.data.server_knowledge;
    Ok(())
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
            // First load from IndexedDB
            if let Ok((payees, knowledge)) = get_payees(&db).await {
                if !payees.is_empty() {
                    *PAYEES_CACHE.write() = payees;
                    *PAYEES_KNOWLEDGE.write() = knowledge;
                    debug!("Loaded payees from IndexedDB");
                }
            }

            // If PAYEES_CACHE is still empty and we have a valid token, fetch automatically
            if PAYEES_CACHE.read().is_empty() {
                if let Some(ref t) = token {
                    if t.is_valid() {
                        is_loading.set(true);
                        error_msg.set(None);
                        if let Err(e) = fetch_and_store_payees(&db, &t.to_string()).await {
                            error_msg.set(Some(e));
                        }
                        is_loading.set(false);
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
                    if let Err(e) = fetch_and_store_payees(&db, &t.to_string()).await {
                        error_msg.set(Some(e));
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
            span { "Server Knowledge: " code { "{PAYEES_KNOWLEDGE.read()}" } }
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
                                        span { class: "payee-name", "{payee.name}" }
                                        span {
                                            class: "payee-id-subtext",
                                            "data-fieldname": "id",
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

/// Transaction Page
#[component]
fn Transactions() -> Element {
    rsx! {
        TransactionList {}
    }
}

/// Transaction List
#[component]
fn TransactionList() -> Element {
    rsx! {
        div {
            class: "table-container",
            table {
                thead {
                    tr {
                        th { "Date" }
                        th { "Amount" }
                        th { "Memo" }
                        th { "Cleared" }
                        th { "Approved" }
                        th { "Payee Name" }
                        th { "Category" }
                    }
                }
                tbody {
                    Transaction {}
                }
            }
        }
    }
}

/// Transaction
#[component]
fn Transaction() -> Element {
    rsx! {
        tr {
            td {"data-fieldname": "date", "2026-01-01"},
            td {"data-fieldname": "amount", "-100.00"},
            td {"data-fieldname": "memo", "A test transaction"},
            td {"data-fieldname": "cleared", "reconciled"},
            td {"data-fieldname": "approved", "true"},
            td {"data-fieldname": "payee_name", "ACME"},
            td {"data-fieldname": "category_name", "Monthly"}
        }
    }
}

/// Shared navbar component.
#[component]
fn Navbar() -> Element {
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

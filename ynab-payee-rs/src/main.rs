use std::sync::Arc;

use dioxus::{html::nav, prelude::*, stores::index};
use idb::{Database, DatabaseEvent, Factory, IndexParams, KeyPath, ObjectStoreParams};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::{from_value, Serializer};
use wasm_bindgen::JsValue;

use anyhow::Context;
use ynab_payee_rs::{
    database::{create_database, get_payees, get_ynab_token, replace_payees, store_ynab_token},
    models::{Payee, YnabResponse},
    AuthActions,
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
const HEADER_SVG: Asset = asset!("/assets/header.svg");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

/// Local in-memory cache of payees pulled from the API; don't update this too often
static PAYEES_CACHE: GlobalSignal<Vec<Payee>> = GlobalSignal::new(|| Vec::new());
static PAYEES_KNOWLEDGE: GlobalSignal<u64> = GlobalSignal::new(|| 0);
/// The budget ID to use when talking to the API
// static BUDGET: GlobalSignal<&str> = GlobalSignal::new(|| "e71410e0-306f-42bb-8e79-ac905a392a9a");
static BUDGET: GlobalSignal<&str> = GlobalSignal::new(|| "594b4cb4-028e-41a9-a908-d00ee0647611");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    info!("ATTENTION: Some of the debug code may display your YNAB oauth2 token.");
    info!("This is only stored in your browser and never sent to any server");
    info!("other than YNAB's API. Nobody but you can see your logs, or your");
    info!("YNAB oauth2 token.");

    let callback_access_token = get_token_from_hash();

    let db_resource = use_resource(move || async move {
        let db = create_database().await.expect("unable to open database");
        Arc::new(db)
    });

    let db_handle = db_resource.read().as_ref().cloned();

    match db_handle {
        Some(db) => {
            use_context_provider(|| db.clone());
            debug!("added database handle to context");
            let auth_actions = use_hook(move || {
                let aa = AuthActions::new(db.clone());
                aa
            });
            use_context_provider(|| auth_actions.clone());
            debug!("added auth_actions handle to context");

            let aa = auth_actions.clone();
            use_hook(move || {
                if let Some(token) = callback_access_token {
                    aa.store_token(token.into());
                }
            });
            let do_login = move |_| {
                let auth_url = "https://app.ynab.com/oauth/authorize?client_id=Dszs-ubBjiUIP9pEHY7znF9LI_GrFdWSErx8zj84JWA&redirect_uri=http%3A%2F%2Flocalhost%3A8080&response_type=token&scope=";

                // Send user to ynab login page
                if let Some(window) = web_sys::window() {
                    let _ = window.location().set_href(auth_url);
                }
                // let aa = use_context::<AuthActions>();
                // async move {
                //     aa.store_token("test_token_from_login_button".into());
                // }
            };

            let token = (auth_actions.token)();
            debug!("token in main: {:#?}", &token);
            match token {
                Some(token) if token.is_valid() => {
                    rsx! {
                        document::Link { rel: "icon", href: FAVICON }
                        document::Link { rel: "stylesheet", href: MAIN_CSS } document::Link { rel: "stylesheet", href: TAILWIND_CSS }

                        p {"Code is {token}"}
                        Router::<Route> {}
                    }
                }
                _ => {
                    rsx! {
                        p{"Need to login"},
                        button {
                            id: "login",
                            onclick: do_login,
                            "Login"
                        }
                    }
                }
            }
        }
        None => {
            rsx! {
                "Initializing database..."
            }
        }
    }
}

/// Payees Page
#[component]
fn Payees() -> Element {
    use_resource(move || async move {
        let db = use_context::<Arc<Database>>();
        match get_payees(&db).await {
            Ok((payees, knowledge)) => {
                *PAYEES_CACHE.write() = payees;
                *PAYEES_KNOWLEDGE.write() = knowledge;
                debug!("set PAYEES_CACHE from indexdb");
            }
            Err(e) => {
                error!("unable to get payees from indexdb: {:#?}", e);
            }
        }
    });

    let auth = use_context::<AuthActions>();
    let token = (auth.token)();

    let get_payees = move |_| {
        let token = token.clone();

        async move {
            match token {
                Some(t) if t.is_valid() => {
                    let db = use_context::<Arc<Database>>();
                    let httpclient = reqwest::Client::new();
                    let mut headers = HeaderMap::new();
                    headers.insert("Authorization", format!("Bearer {}", t).parse().unwrap());
                    let response = httpclient
                        .get(format!("https://api.ynab.com/v1/budgets/{}/payees", BUDGET))
                        .headers(headers)
                        .send()
                        .await
                        .unwrap();
                    // debug!("{:?}", response.text().await);
                    let ynabresponse: YnabResponse = response.json().await.unwrap();
                    // debug!("{:#?}", ynabresponse);
                    debug!("got payees from ynab api and stored in local cache");
                    debug!("adding payees to indexdb");
                    replace_payees(&db, &ynabresponse.data).await.map_err(|e| {
                        error!("Failed in replace_payees: {:#?}", e);
                    });
                    *PAYEES_CACHE.write() = ynabresponse.data.payees;
                    *PAYEES_KNOWLEDGE.write() = ynabresponse.data.server_knowledge;
                    debug!("added payees to indexdb")
                }
                _ => error!("No valid token. Please sign in again."),
            }
        }
    };

    rsx! {
        div {
            button { onclick: get_payees, id: "get_payees", "Get Payees" }
        }
        p { "Server Knowledge: {PAYEES_KNOWLEDGE.read()}"}
        table {
            tr {
                th { "ID" }
                th { "Name" }
                th { "Transfer Account ID" }
                th { "Deleted" }
            }
            for payee in PAYEES_CACHE.iter() {
                tr {
                    td {"data-fieldname": "id", "{payee.id}"},
                    td {"data-fieldname": "name", "{payee.name}"},
                    td {"data-fieldname": "transfer_acct_id", "{payee.transfer_account_id.as_deref().unwrap_or(\"None\")}"},
                    td {"data-fieldname": "deleted", "{payee.deleted}"},
                }
            }
        }
    }
}

/// Transaction Page
#[component]
fn Transactions() -> Element {
    rsx! {
        TransactionList {  }
    }
}

/// Transaction List
#[component]
fn TransactionList() -> Element {
    rsx! {
        table {
            tr {
                th { "Date" }
                th { "Amount" }
                th { "Memo" }
                th { "Cleared" }
                th { "Approved" }
                th { "Payee Name" }
                th { "Category" }
            }
            Transaction {}
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
        div {
            id: "navbar",
            Link {
                to: Route::Payees {},
                "Payees"
            }
            Link {
                to: Route::Transactions {},
                "Transactions"
            }
        }
        // NOTE: Header component reference can go here
        p {
            "Warning: This app will directly affect your YNAB budget. "
            "It will not make a backup for you. "
            "Please make a backup yourself before using this."
        }

        Outlet::<Route> {}
    }
}

fn get_token_from_hash() -> Option<String> {
    let window = web_sys::window()?;
    let hash = window.location().hash().ok()?;

    debug!("hash string: {:#?}", &hash);

    if hash.is_empty() {
        // Empty string, not valid
        return None;
    }

    let fragment = hash.trim_start_matches('#');
    let params = web_sys::UrlSearchParams::new_with_str(fragment).ok()?;
    params.get("access_token")
}

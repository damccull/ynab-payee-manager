use std::sync::Arc;

use dioxus::{prelude::*, stores::index};
use idb::{Database, DatabaseEvent, Factory, IndexParams, KeyPath, ObjectStoreParams};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_wasm_bindgen::Serializer;
use wasm_bindgen::JsValue;

use anyhow::Context;

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[layout(Navbar)]
    // #[route("/")]
    // Home {},
    // #[route("/blog/:id")]
    // Blog { id: i32 },
    #[route("/")]
    // #[route("/payees")]
    Payees {},
    #[route("/transactions")]
    Transactions {},
}

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/main.css");
const HEADER_SVG: Asset = asset!("/assets/header.svg");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

const DATABASE_NAME: &str = "ynab-payee-manager";

/// Personal Access Token for YNAB account
static PAT: GlobalSignal<Option<&str>> = GlobalSignal::new(|| None);
/// Local in-memory cache of payees pulled from the API; don't update this too often
static PAYEES_CACHE: GlobalSignal<Vec<Payee>> = GlobalSignal::new(|| Vec::new());
/// The budget ID to use when talking to the API
static BUDGET: GlobalSignal<&str> = GlobalSignal::new(|| "e71410e0-306f-42bb-8e79-ac905a392a9a");

fn main() {
    dioxus::launch(App);
}

async fn create_database() -> Result<Database, idb::Error> {
    let factory = Factory::new()?;
    let mut open_request = factory
        .open(DATABASE_NAME, Some(2))
        .expect("unable to open database");

    // Add an upgrade handler for the database
    open_request.on_upgrade_needed(|event| {
        // Get database instance from event
        let database = event.database().expect("unable to get database from event");

        database.delete_object_store("payees");

        // Prepare object store parameters
        let mut store_parameters = ObjectStoreParams::new();
        store_parameters.auto_increment(true);
        store_parameters.key_path(Some(KeyPath::new_single("id")));

        // Create 'payees' object store
        let payees_store = database
            .create_object_store("payees", store_parameters)
            .expect("unable to create object store");

        // Prepare index parameters
        // let mut index_parameters = IndexParams::new();
        // index_parameters.unique(true);

        // Create an index for 'name' on the payees_store
        // payees_store
        //     .create_index("name", KeyPath::new_single("name"), Some(index_parameters))
        //     .expect("unable to create index");
    });

    open_request.await
}

async fn add_payees(database: &Database, payees: &Vec<Payee>) -> anyhow::Result<Vec<JsValue>> {
    debug!("adding {} payees", &payees.len());
    let transaction = database
        .transaction(&["payees"], idb::TransactionMode::ReadWrite)
        .map_err(|e| anyhow::anyhow!("unable to start transaction: {:#?}", e))?;

    let store = transaction
        .object_store("payees")
        .map_err(|e| anyhow::anyhow!("unable to get objectstore: {:#?}", e))?;

    let mut ids = Vec::new();
    for payee in payees.iter() {
        trace!("adding payee: {:#?}", &payee);
        let id = store
            .add(
                &payee
                    .serialize(&Serializer::json_compatible())
                    .map_err(|e| anyhow::anyhow!("unable to serialize payee: {:#?}", e))?,
                None,
            )
            .map_err(|e| anyhow::anyhow!("unable to store payee: {:#?}", e))?
            .await
            .map_err(|e| anyhow::anyhow!("unable to store payee: {:#?}", e))?;

        trace!("add payee: {:#?}", &payee);
        ids.push(id);
    }
    // let payee = serde_json::json!({
    //     "name": "BP"
    // });

    transaction
        .commit()
        .map_err(|e| anyhow::anyhow!("unable to commit transaction: {:#?}", e))?
        .await
        .map_err(|e| anyhow::anyhow!("unable to commit transaction: {:#?}", e))?;

    debug!("added {} payees", &ids.len());

    Ok(ids)
}

#[component]
fn App() -> Element {
    let env_pat = env!("YNAB_PAT"); // TODO this bakes it into the binary, security risk
    *PAT.write() = Some(env_pat);

    // use_future(move || async move {
    //     let database = create_database().await?;
    //     *DATABASE.write() = Some(database);
    //
    //     //Allows for ? syntax sugar
    //     Ok::<(), Box<dyn std::error::Error>>(())
    // });

    let db_resource = use_resource(move || async move {
        let db = create_database().await.expect("unable to open database");
        Arc::new(db)
    });

    let db_handle = db_resource.read().as_ref().cloned();

    match db_handle {
        Some(db) => {
            use_context_provider(|| db.clone());
            debug!("added database handle to context");

            rsx! {
                document::Link { rel: "icon", href: FAVICON }
                document::Link { rel: "stylesheet", href: MAIN_CSS } document::Link { rel: "stylesheet", href: TAILWIND_CSS }
                Router::<Route> {}
            }
        }
        None => {
            rsx! {
                "Initializing database..."
            }
        }
    }
}

/// Home page
#[component]
fn Home() -> Element {
    rsx! {}
}

/// Payees Page
#[component]
fn Payees() -> Element {
    let get_payees = move |_| async move {
        let db = use_context::<Arc<Database>>();
        let httpclient = reqwest::Client::new();
        let mut headers = HeaderMap::new();
        headers.insert(
            "Authorization",
            format!("Bearer {}", PAT.unwrap()).parse().unwrap(),
        );
        let response = httpclient
            .get(format!("https://api.ynab.com/v1/budgets/{}/payees", BUDGET))
            .headers(headers)
            .send()
            .await
            .unwrap();
        // debug!("{:?}", response.text().await);
        let ynabresponse: YnabResponse = response.json().await.unwrap();
        // debug!("{:#?}", ynabresponse);
        *PAYEES_CACHE.write() = ynabresponse.data.payees;
        debug!("got payees from ynab api and stored in local cache");
        debug!("adding payees to indexdb");
        add_payees(&db, &*PAYEES_CACHE.read()).await;
        debug!("added payees to indexdb")
    };

    rsx! {
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
        div {
            button { onclick: get_payees, id: "get_payees", "Get Payees" }
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

// /// Blog page
// #[component]
// pub fn Blog(id: i32) -> Element {
//     rsx! {
//         div {
//             id: "blog",
//
//             // Content
//             h1 { "This is blog #{id}!" }
//             p { "In blog #{id}, we show how the Dioxus router works and how URL parameters can be passed as props to our route components." }
//
//             // Navigation links
//             Link {
//                 to: Route::Blog { id: id - 1 },
//                 "Previous"
//             }
//             span { " <---> " }
//             Link {
//                 to: Route::Blog { id: id + 1 },
//                 "Next"
//             }
//         }
//     }
// }

/// Shared navbar component.
#[component]
fn Navbar() -> Element {
    rsx! {
        div {
            id: "navbar",
            // Link {
            //     to: Route::Home {},
            //     "Home"
            // }
            // Link {
            //     to: Route::Blog { id: 1 },
            //     "Blog"
            // }
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

#[derive(Debug, Deserialize)]
struct YnabResponse {
    data: ResponseData,
}
#[derive(Debug, Deserialize)]
struct ResponseData {
    payees: Vec<Payee>,
    server_knowledge: i32,
}

#[derive(Debug, Deserialize, Serialize)]
struct Payee {
    id: String,
    name: String,
    transfer_account_id: Option<String>,
    deleted: bool,
}

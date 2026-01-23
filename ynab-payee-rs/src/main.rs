use dioxus::prelude::*;
use reqwest::header::HeaderMap;
use serde::Deserialize;

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

/// Personal Access Token for YNAB account
static PAT: GlobalSignal<Option<&str>> = GlobalSignal::new(|| None);
/// Local in-memory cache of payees pulled from the API; don't update this too often
static PAYEES_CACHE: GlobalSignal<Vec<Payee>> = GlobalSignal::new(|| Vec::new());
/// The budget ID to use when talking to the API
static BUDGET: GlobalSignal<&str> = GlobalSignal::new(|| "e71410e0-306f-42bb-8e79-ac905a392a9a");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    let env_pat = env!("YNAB_PAT"); // TODO this bakes it into the binary, security risk
    *PAT.write() = Some(env_pat);
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS } document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        Router::<Route> {}
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

#[derive(Debug, Deserialize)]
struct Payee {
    id: String,
    name: String,
    transfer_account_id: Option<String>,
    deleted: bool,
}

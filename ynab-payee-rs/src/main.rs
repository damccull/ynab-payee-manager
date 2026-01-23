use dioxus::prelude::*;

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

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
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
    rsx! {
        PayeesList {  }
    }
}

/// Payees List
#[component]
fn PayeesList() -> Element {
    rsx! {
        table {
            tr {
                th { "ID" }
                th { "Name" }
                th { "Transfer Account ID" }
                th { "Deleted" }
            }
            Payee {}
        }

    }
}
/// Payee
#[component]
fn Payee() -> Element {
    rsx! {
        tr {
            td {"data-fieldname": "id", "12e6994f-db47-4141-8b02-a26fe367cee6"},
            td {"data-fieldname": "name", "ACME"},
            td {"data-fieldname": "transfer_acct_id", "null"},
            td {"data-fieldname": "deleted", "false"},
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

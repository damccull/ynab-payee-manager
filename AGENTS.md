# YNAB Payee Manager (`ynab-payee-rs`)

Client-side Single Page Application (SPA) built with Rust and [Dioxus 0.7](https://dioxuslabs.com/learn/0.7) compiled to WebAssembly (WASM). The app provides a fast interface for viewing and managing payees in YNAB (You Need A Budget).

---

## 1. Architecture & Current State

- **Client-Side Only**: Runs entirely in the browser using WASM. There is no custom backend server; all requests go directly to the YNAB REST API from the browser.
- **YNAB Authentication**:
  - Implements OAuth 2.0 **Implicit Grant Flow** (`response_type=token`).
  - Authorization endpoint: `https://app.ynab.com/oauth/authorize?client_id=Dszs-ubBjiUIP9pEHY7znF9LI_GrFdWSErx8zj84JWA&redirect_uri=http%3A%2F%2Flocalhost%3A8080&response_type=token&scope=`.
  - The access token is delivered in the URL fragment hash (`#access_token=...`).
  - On application mount, `get_token_from_hash()` parses the token, immediately strips the fragment from browser history via `window.history().replace_state_with_url`, and provides it for persistence.
  - Token validity is tracked with `YnabToken`, currently defaulting to a 2-hour lifespan from time of creation.
- **Budget Selection**:
  - Configured with `"default"` as the budget identifier (`BUDGET` global signal).
  - In YNAB OAuth apps with Default Budget Selection enabled, `/v1/budgets/default/...` routes to whichever budget the user selected during OAuth consent.
- **Local Persistence (IndexedDB)**:
  - Database name: `ynab-payee-manager` (Version `13`).
  - Managed via the `idb` crate with stores:
    - `settings`: Stores the serialized `YnabToken` under key `"ynab_user_token"`.
    - `payees`: Stores `Payee` records with inline key `id` and an index on `name`.
    - `transactions`: Stores `Transaction` records with inline key `id` and index on `payee_id`.
    - `server_knowledge`: Stores the latest server knowledge integer (`u64`) for payees and transactions.
    - `payee_review_queue`: Stores pending `PayeeReviewItem` records awaiting user name confirmation.
    - `action_queue`: Stores queued `PayeeAction` records waiting to be committed to YNAB.
- **API Communication**:
  - Uses `reqwest` with JSON and WASM support to query `https://api.ynab.com/v1/budgets/default/payees` and `/transactions`.
  - Full refreshes replace local records in IndexedDB and in-memory global signals.

---

## 2. Current Routes & UI Components

The application is structured into the following components and routes in `src/main.rs`:

- **`App`**: Root entry point. Sets document links (`favicon.ico`, `main.css`, `tailwind.css`), extracts the token from URL hash, initializes IndexedDB via `use_resource`, and passes the database handle to `AuthenticatedApp`.
- **`AuthenticatedApp`**:
  - Provides the database handle (`Arc<idb::Database>`) and `AuthActions` instance into Dioxus context.
  - If no valid token is found, displays the login view with a "Login with YNAB" button.
  - When authenticated with a valid token, renders `Router::<Route>`.
- **`Navbar`**: Layout component wrapping child routes.
  - Provides navigation links between Payees (`/`) and Transactions (`/transactions`).
  - Displays a warning banner advising users to make a budget backup.
  - Mounts child pages via `Outlet::<Route>`.
- **`Payees` (`/`)**:
  - Loads cached payee records and server knowledge from IndexedDB on initial mount.
  - If IndexedDB is empty, automatically triggers an initial fetch from the YNAB API.
  - Includes a manual "Sync Payees" button to fetch current payee data from the YNAB API, persist it to IndexedDB via `replace_payees`, and update in-memory cache signals.
  - Renders a table displaying Payee ID, Name, Transfer Account ID, and Deleted status.
- **`Transactions` (`/transactions`)**:
  - Displays cached transactions stored in IndexedDB with date, payee, memo, amount, cleared, and approved status.
  - Includes a manual "Sync Transactions" button.
- **`Duplicates` (`/duplicates`)**:
  - Scans cached payees (excluding payees with 0 transactions and those already queued) using fuzzy string similarity clustering (defaulting to strict matching).
  - Allows selecting duplicate payees to combine and queuing them into the Payee Review Queue.
- **`ReviewQueue` (`/review`)**:
  - Displays combined payees where the user can confirm or edit the suggested target name before pushing to the Action Queue.
- **`ActionQueue` (`/queue`)**:
  - Displays queued transactions updates and renames, with committing, clearing, and automatic full refresh upon completion.

---

## 3. State Management & Data Flow

- **Global Signals**:
  - `PAYEES_CACHE: GlobalSignal<Vec<Payee>>`: In-memory cache of payees displayed in the UI.
  - `PAYEES_KNOWLEDGE: GlobalSignal<u64>`: Server knowledge version returned by YNAB.
  - `BUDGET: GlobalSignal<&str>`: Target budget ID string (`"default"`).
- **Context Signals**:
  - `Arc<idb::Database>`: Provided to the component tree.
  - `AuthActions`: Exposes `token: Signal<Option<YnabToken>>` and `status: Signal<AuthActionStatus>` (`Idle`, `Loading`, `Saving`, `Error`).
- **Dioxus 0.7 API Conventions**:
  - No `cx` or `Scope`; state uses `Signal`, `use_signal`, `use_resource`, and `GlobalSignal`.
  - Signal reading via `.read()` or `sig()`; signal mutation via `.write()` or `.set()`.
  - Async operations spawned via `dioxus::core::spawn` or tracked via `use_resource`.
  - Routes defined via `#[derive(Routable)]` with layout annotations.

---

## 4. Local Development & Tooling

### Dev Server
Run the application using the Dioxus CLI:
```sh
dx serve --platform web --port 8080
```

> [!WARNING]
> Port Conflict Notice: A local Docker compose stack (e.g., `odysseus-searxng-1`) may bind to `127.0.0.1:8080`.
> If SearXNG appears instead of this app, stop the container (`docker stop odysseus-searxng-1`) or change SearXNG's port.
> The YNAB OAuth application `redirect_uri` is registered to `http://localhost:8080`.

### Nix Flake Environment
The repository includes a `flake.nix` providing:
- Rust toolchains (stable default, nightly, msrv) with `wasm32-unknown-unknown` target support.
- `dioxus-cli` v0.7.10 (`dx`).
- LLVM/Clang unwrapped compiler variables (`CC_wasm32_unknown_unknown`, `CXX_wasm32_unknown_unknown`, `AR_wasm32_unknown_unknown`) for WASM builds.
- Devshell hook unsetting `NIX_LDFLAGS` and `NIX_LDFLAGS_FOR_BUILD` to avoid linker argument overflow during host build script compilation.

---

## 5. Repository File Structure

- `Cargo.toml`: Package manifest with features (`web`, `desktop`, `mobile`) and dependencies (`dioxus 0.7.1`, `idb`, `reqwest`, `serde`, `chrono`, `wasm-bindgen`, `web-sys`).
- `Dioxus.toml`: Dioxus web application configuration.
- `flake.nix`: Nix development environments and package definitions.
- `src/lib.rs`: Library module declarations and re-exports (`AuthActions`, `YnabToken`).
- `src/main.rs`: Entry point, routing definitions, UI components (`App`, `AuthenticatedApp`, `Navbar`, `Payees`, `Transactions`), global signals, OAuth hash parsing, and API synchronization.
- `src/auth_actions.rs`: `AuthActions` controller managing token retrieval, storage, and asynchronous status signals.
- `src/database.rs`: IndexedDB creation, schema upgrades, token storage/retrieval, and payee CRUD functions (`create_database`, `store_ynab_token`, `get_ynab_token`, `replace_payees`, `get_payees`).
- `src/models.rs`: YNAB data structures (`Payee`, `ResponseData`, `YnabResponse`).
- `src/settings.rs`: Storage keys, database constants, and OAuth redirect URL.
- `src/token.rs`: `YnabToken` struct with validity checking and timestamp calculations.
- `assets/`: Static web assets (`favicon.ico`, `main.css`, `tailwind.css`, `header.svg`).

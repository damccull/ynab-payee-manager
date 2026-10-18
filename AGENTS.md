# YNAB Payee Manager (`ynab-payee-rs`)

This project is a client-side Single Page Application (SPA) built with Rust and [Dioxus 0.7](https://dioxuslabs.com/learn/0.7) compiled to WebAssembly (WASM). Its purpose is to provide a fast and streamlined tool for managing payees in YNAB (You Need A Budget), overcoming the limitations of the official web app's "Manage Payees" interface.

---

## 1. Architecture & Core Concepts

- **Client-Side Only**: Runs entirely in the browser using WASM. No backend server is required.
- **YNAB Authentication**:
  - Uses OAuth 2.0 **Implicit Grant Flow** (`response_type=token`).
  - Authorization endpoint: `https://app.ynab.com/oauth/authorize?client_id=...&redirect_uri=...&response_type=token&scope=`
  - Redirect returns token in URL hash fragment (`#access_token=...&token_type=Bearer&expires_in=7200`).
  - Hash is parsed on initial load, token is stored into IndexedDB, and the URL fragment is cleared via `window.history().replace_state_with_url` to avoid leaking the token.
- **Budget Selection**:
  - In YNAB OAuth apps with "Default Budget Selection" enabled, the user chooses which budget they want to authorize.
  - The API path `/v1/budgets/default/payees` automatically routes to this user-selected budget without needing a hardcoded budget UUID.
- **Local Persistence**:
  - Uses browser **IndexedDB** via the `idb` crate.
  - Object stores:
    - `settings`: Stores the current `YnabToken` (token string and expiration).
    - `payees`: Stores cached `Payee` records.
    - `server_knowledge`: Stores the latest server delta knowledge integer for payees.
- **API Communication**:
  - `reqwest` (with `json` and `rustls`/WASM support) fetches from `https://api.ynab.com/v1/budgets/default/payees`.

---

## 2. Local Environment & Port Configuration

> [!WARNING]
> Port Conflict Notice: A local Docker compose stack (e.g., `odysseus-searxng-1`) may bind to `127.0.0.1:8080`.
> If `SearXNG` appears instead of this app when navigating to `http://localhost:8080`, stop the container (`docker stop odysseus-searxng-1`) or rebind SearXNG's port.
> The YNAB OAuth application `redirect_uri` is registered to `http://localhost:8080`. If you change the dev server port, you must also update the Redirect URI in the YNAB Developer portal.

### Serving the App
Run the Dioxus development server:
```sh
dx serve --platform web --port 8080
```

---

## 3. Dioxus 0.7 Rules & Guidelines

Dioxus 0.7 removes `cx`, `Scope`, and `use_state`. All state is managed through `Signal`, `use_signal`, `use_resource`, `use_memo`, and `GlobalSignal`.

### Dependency Configuration
```toml
[dependencies]
dioxus = { version = "0.7.1", features = ["router"] }

[features]
default = ["web"]
web = ["dioxus/web"]
desktop = ["dioxus/desktop"]
```

### Components & Props
- Components are functions annotated with `#[component]`.
- Props must be owned values (`String`, `Vec<T>`), implementing `PartialEq` and `Clone`.
- Wrap reactive props in `ReadOnlySignal` where appropriate.

### State Management
- `use_signal(|| initial_val)` for local state.
- Calling `sig()` clones the value; `.read()` borrows; `.write()` or `.with_mut(...)` mutates.
- `GlobalSignal::new(|| ...)` for application-wide signals.
- Context is provided with `use_context_provider(|| value)` and retrieved with `use_context::<T>()`.

### Async / Resources
- Use `use_resource` to manage async data loading.
- Spawning asynchronous tasks: `spawn(async move { ... })`.

### Routing
- Route enum derives `Routable, Clone, PartialEq`.
- Child layouts use `#[layout(LayoutComponent)]` with `Outlet::<Route> {}`.

---

## 4. Key Files

- `src/main.rs`: Root `App` component, router, OAuth callback parser, and `Payees` view.
- `src/auth_actions.rs`: Manages auth state signals and IndexedDB token persistence.
- `src/token.rs`: `YnabToken` struct with expiration tracking and validation.
- `src/database.rs`: IndexedDB initialization and transaction helpers via `idb`.
- `src/models.rs`: YNAB API models (`Payee`, `ResponseData`, `YnabResponse`).
- `src/settings.rs`: IndexedDB store names and constant keys.

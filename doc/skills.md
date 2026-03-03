# ClardRs Development Skills & Guidelines

This document outlines the core patterns and guidelines for developing and extending the `clard` TUI client.

## Architecture Overview

Clard uses a standard `ratatui` + `crossterm` + `tokio` stack.
- **TUI Framework**: `ratatui` handles rendering, `crossterm` handles terminal backend and input events.
- **Async Runtime**: `tokio` is used to manage asynchronous tasks (e.g., API requests via `reqwest`).
- **State Management**: The central state is held in `APP` (`src/app.rs`). Different pages have their own state modules (e.g., `MenuState`, `ProxyState`).
- **IPC Backend**: `Backend` in `src/ipc/backend.rs` implements the Clash REST API and WebSocket events.

## Core Development Patterns

### 1. State Management
Whenever you add a new page (e.g., `Connections` or `Rules`):
1. Add a state struct in a new module (e.g., `src/app/connections.rs`).
2. Add the state to the `WindowState` enum in `src/app.rs`.
3. Implement `on_up_key`, `on_down_key`, `on_left_key`, `on_right_key`, and `on_enter_key` for that state to handle user input.

### 2. Async API Calls and Events
The TUI event loop in `src/lib.rs` is synchronous, but API calls are asynchronous. **Never block the event loop**.
1. When a user action requires an API call (e.g., pressing `Enter` to select a node), clone the `Backend` (`Arc<Backend>`) and the `ClardEvent` sender.
2. Spawn a `tokio` task:
   ```rust
   tokio::spawn(async move {
       match backend.do_something().await {
           Ok(data) => {
               let _ = sender.send(ClardEvent::UpdateData(data));
           }
           Err(e) => {
               let _ = sender.send(ClardEvent::Error(e.to_string()));
           }
       }
   });
   ```
3. Add a new variant to `ClardEvent` in `src/event.rs`.
4. Handle the new event in the `receiver.recv().await` loop in `src/lib.rs` by updating the `APP` state.

### 3. Rendering
Rendering is handled in `src/canvas.rs`.
- The `Painter::draw` method matches on `app.current_page`.
- Create a new `*Layout` struct (e.g., `ConnectionsLayout`) and implement a `draw_connections` method.
- Pass the specific page state (`ConnectionsState`) to the drawing method.
- Use `ratatui` widgets. For interactive lists, clone the `ListState` and use `f.render_stateful_widget()`.

## Future Work
- **Connections Page**: Subscribe to the `traffic` and `connections` WebSockets, maintain a list of active connections, and allow users to close connections via `Delete` key.
- **Rules Page**: Fetch from `get_rules()` and render the routing rules.
- **Config Handling**: Parse `~/.config/clard/config.toml` (using `src/config.rs`) to dynamically determine the Backend URL (`unix socket` vs `tcp`) instead of hardcoding.

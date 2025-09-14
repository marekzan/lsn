  # Architecture

[main.rs]
   │
   │ 1. Parse CLI Args
   ▼
[cli.rs]
   │
   │ 2. Create App instance
   ▼
[app.rs: App::new]
   │
   │ 3. Initialize Terminal, Components, and Action channel
   ▼
[app.rs: App::run]
   │
   │ 4. Enter terminal raw mode and start event loop
   ▼
[terminal/mod.rs & events.rs]
   │
   │ 5. Listen for terminal events (Tick, Render, Inputs, Resize)
   │    in a separate thread.
   │
   │ 6. Send TermEvent to App
   ▼
[app.rs: handle_terminal_events]
   │
   │ 7. Receive TermEvent
   │
   ├───► 8a. Map TermEvent to GlobalAction (e.g., Tick, Quit)
   │ │
   ├───► 8b. Pass TermEvent to Components
   │ │
   ├───► 8c. Map key events to actions
   │ │
   │ │ 9. Components can return a new AppAction
   │ ▼
   │ 10. Send AppAction (from 8a, 8c or 9)
   │     to the action channel
   ▼
[Action Channel (tokio::mpsc)]
   │
   │ 11. App::handle_actions receives
   │     AppAction from channel
   ▼
[app.rs: handle_actions]
   │
   ├───► 12a. Handle GlobalAction (e.g., Quit, Resize, ClearScreen)
   │ │      If Render action:
   │ │         └─► [app.rs: render] ───► [ratatui] draws to terminal
   │ │               │
   │ │               └─► For each component: [component.draw]
   │ │
   │ └─► 12b. Pass AppAction to all components: [component.update]
   │
   │ 13. A component's update can return a new AppAction,
   │     which is sent back to the action channel.
   │
   └─────────────────────────────────── Loop to step 11 ───┘

## Explanation of the Data Flow:

1. Initialization: main.rs starts the application. It parses command-line arguments (cli.rs) and creates an App instance.
2. App Setup: App::new initializes the main components: the Terminal, the UI components, and an mpsc channel for communication via AppAction.
3. Event Loop: App::run starts the main loop. The Terminal enters raw mode and starts its own thread (terminal/events.rs) to listen for terminal events (key presses, ticks, rendering requests).
4. Event to Action: When an event occurs in the terminal thread, it is sent to the App as a TermEvent.
5. Action Dispatch:
   * app.rs receives the TermEvent.
   * Simple events like Tick or Quit are directly converted into a GlobalAction.
   * Keyboard events (KeyEvent) are checked against the configuration in config/mod.rs to find a corresponding GlobalAction.
   * All events are also passed to the individual UI components, which can in turn generate actions.
6. Action Processing:
   * All generated AppActions are sent to the central mpsc channel.
   * The handle_actions method in app.rs receives the actions from the channel in a loop.
   * GlobalActions are processed directly by the App (e.g., should_quit = true on Quit).
   * All actions are forwarded to the update method of each component to update their internal state.
7. Rendering: When a GlobalAction::Render is received, the App calls the draw method of each component, which then draws to the screen using ratatui.
8. Loop: This process repeats continuously until the Quit action terminates the application.

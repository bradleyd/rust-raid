mod compiler;
mod puzzle;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;
use tui_textarea::TextArea;

use compiler::{validate_solution, ValidationResult};
use puzzle::{load_floor, CodexEntry, Room};

enum GameState {
    TitleScreen,
    Playing,
    RoomComplete,
    RoomTransition, // Shows entry narrative when moving to next room
    LevelComplete,
    ViewingCodex,
    GameOver,
}

enum MenuOption {
    NewGame,
    Quit,
}

impl MenuOption {
    fn next(&self) -> Self {
        match self {
            MenuOption::NewGame => MenuOption::Quit,
            MenuOption::Quit => MenuOption::NewGame,
        }
    }
}

struct App<'a> {
    rooms: Vec<Room>,
    current_room: usize,
    current_level: usize,
    editor: TextArea<'a>,
    locked_lines: Vec<usize>,
    yank_buffer: String,
    message: String,
    message_style: Style,
    message_scroll: u16,
    state: GameState,
    menu_selection: MenuOption,
    hp: u32,
    gold: u32,
    inventory: Vec<String>,
    codex: Vec<CodexEntry>,
    codex_scroll: usize,
    hints_used_room: usize,
    hints_used_total: usize,
    compile_errors_total: u32,
    command_mode: bool,
    command_buffer: String,
}

impl<'a> App<'a> {
    fn new(rooms: Vec<Room>) -> Self {
        let room = &rooms[0];
        let code = room.challenge.code.trim();
        let locked_lines = room.challenge.locked_lines.clone();

        let mut editor = TextArea::from(code.lines());
        editor.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Code Editor [F5: Run | F1: Hint | :q Quit] "),
        );
        editor.set_line_number_style(Style::default().fg(Color::DarkGray));

        App {
            rooms,
            current_room: 0,
            current_level: 1,
            editor,
            locked_lines,
            yank_buffer: String::new(),
            message: String::from("Fix the code. The compiler will guide you..."),
            message_style: Style::default().fg(Color::Yellow),
            message_scroll: 0,
            state: GameState::TitleScreen,
            menu_selection: MenuOption::NewGame,
            hp: 100,
            gold: 0,
            inventory: Vec::new(),
            codex: Vec::new(),
            codex_scroll: 0,
            hints_used_room: 0,
            hints_used_total: 0,
            compile_errors_total: 0,
            command_mode: false,
            command_buffer: String::new(),
        }
    }

    fn room(&self) -> &Room {
        &self.rooms[self.current_room]
    }

    fn start_game(&mut self) {
        self.state = GameState::Playing;
        self.hp = 100;
        self.gold = 0;
        self.inventory.clear();
        self.current_level = 1;
        self.hints_used_room = 0;
        self.hints_used_total = 0;
        self.compile_errors_total = 0;
        self.load_room(0);
    }

    fn load_level(&mut self, level: usize) -> Result<(), String> {
        let floor_name = match level {
            1 => "floor_01_ownership",
            2 => "floor_02_borrowing",
            3 => "floor_03_patterns",
            _ => return Err(format!("Level {} not implemented yet", level)),
        };
        let floor_path = std::path::Path::new("puzzles").join(floor_name);
        match load_floor(&floor_path) {
            Ok(rooms) if !rooms.is_empty() => {
                self.rooms = rooms;
                self.current_level = level;
                self.current_room = 0;
                self.hints_used_total = 0;
                self.compile_errors_total = 0;
                self.load_room(0);
                Ok(())
            }
            Ok(_) => Err(format!("No rooms found in level {}", level)),
            Err(e) => Err(format!("Failed to load level {}: {}", level, e)),
        }
    }

    fn is_line_locked(&self, line: usize) -> bool {
        self.locked_lines.contains(&(line + 1))
    }

    fn load_room(&mut self, index: usize) {
        self.current_room = index;
        let room = &self.rooms[index];
        let code = room.challenge.code.trim();
        self.locked_lines = room.challenge.locked_lines.clone();

        self.editor = TextArea::from(code.lines());
        self.editor.set_block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Code Editor [F5: Run | F1: Hint | F2: Keys | :q] "),
        );
        self.editor
            .set_line_number_style(Style::default().fg(Color::DarkGray));

        self.message = String::from("Fix the code. The compiler will guide you...");
        self.message_style = Style::default().fg(Color::Yellow);
        self.state = GameState::Playing;
        self.hints_used_room = 0;
    }

    fn advance_room(&mut self) {
        if self.current_room + 1 < self.rooms.len() {
            let next_room = &self.rooms[self.current_room + 1];
            // Check if next room has entry narrative for transition
            if let Some(entry) = &next_room.narrative.entry {
                self.state = GameState::RoomTransition;
                self.message = format!(
                    "{}\n\n\
                    ─────────────────────────────────\n\
                    Press ENTER to continue...",
                    entry.trim()
                );
                self.message_style = Style::default().fg(Color::Cyan);
                self.message_scroll = 0;
            } else {
                self.load_room(self.current_room + 1);
            }
        } else {
            // Check for required items to proceed to next level
            if self.current_level == 1 {
                let has_scroll = self.inventory.iter().any(|i| i == "Sacred Scroll");
                if !has_scroll {
                    self.message =
                        "The twin doors swing open, but an invisible barrier blocks your path.\n\n\
                        \"You cannot pass without the Sacred Scroll. There is knowledge\n\
                        inscribed upon it that you will need in the depths below.\"\n\n\
                        Perhaps you missed something in an earlier chamber..."
                            .to_string();
                    self.message_style = Style::default().fg(Color::Magenta);
                    return;
                }
            }

            self.state = GameState::LevelComplete;
            let perfect = self.hints_used_total == 0 && self.compile_errors_total == 0;
            let inventory_display = if self.inventory.is_empty() {
                "  (empty)".to_string()
            } else {
                self.inventory
                    .iter()
                    .map(|i| format!("  - {}", i))
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            let level_name = match self.current_level {
                1 => "Ownership",
                2 => "Borrowing",
                3 => "Patterns",
                _ => "Unknown",
            };
            let next_action = match self.current_level {
                1 => "Press ENTER to descend to Level 2: Borrowing...",
                2 => "Press ENTER to descend to Level 3: Patterns...",
                _ => "Press ENTER to continue...",
            };

            self.message = format!(
                "=== LEVEL {} COMPLETE! ===\n\n\
                You've mastered the art of {}.{}\n\n\
                ╔══════════════════════════╗\n\
                ║  LEVEL STATS             ║\n\
                ╠══════════════════════════╣\n\
                ║  Rooms cleared:    {:>4}  ║\n\
                ║  Compile errors:   {:>4}  ║\n\
                ║  Hints used:       {:>4}  ║\n\
                ║  Gold earned:      {:>4}  ║\n\
                ║  HP remaining:     {:>4}  ║\n\
                ╚══════════════════════════╝\n\n\
                INVENTORY:\n{}\n\n\
                {}",
                self.current_level,
                level_name,
                if perfect { " PERFECT RUN!" } else { "" },
                self.rooms.len(),
                self.compile_errors_total,
                self.hints_used_total,
                self.gold,
                self.hp,
                inventory_display,
                next_action
            );
            self.message_style = Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD);
        }
    }

    fn run_solution(&mut self) {
        self.message_scroll = 0;
        let code = self.editor.lines().join("\n");
        let expected = &self.room().challenge.expected_output;

        match validate_solution(&code, expected) {
            Ok(ValidationResult::Success) => {
                self.state = GameState::RoomComplete;
                // Award gold based on hints used (fewer hints = more gold)
                let base_gold: u32 = 50;
                let hint_penalty = self.hints_used_room as u32 * 10;
                let earned = base_gold.saturating_sub(hint_penalty).max(10);
                self.gold += earned;

                // Collect item if room grants one
                let item_info = self.room().rewards.as_ref().and_then(|r| {
                    r.grants_item.as_ref().map(|item| {
                        let desc = r
                            .item_description
                            .as_deref()
                            .unwrap_or("A mysterious artifact");
                        (item.clone(), desc.to_string())
                    })
                });
                let item_msg = if let Some((item, desc)) = item_info {
                    self.inventory.push(item.clone());
                    format!("\n\n** ITEM ACQUIRED: {} **\n{}", item, desc)
                } else {
                    String::new()
                };

                // Collect codex entry if room has one
                let codex_msg = if let Some(entry) = self.room().codex.clone() {
                    // Only add if not already in codex (avoid duplicates on replay)
                    if !self.codex.iter().any(|e| e.title == entry.title) {
                        let title = entry.title.clone();
                        self.codex.push(entry);
                        format!(
                            "\n\n** CODEX UPDATED: {} **\nType :codex to review your knowledge.",
                            title
                        )
                    } else {
                        String::new()
                    }
                } else {
                    String::new()
                };

                let alt = self
                    .room()
                    .narrative
                    .alternative_solution
                    .as_ref()
                    .map(|s| format!("\n\nALTERNATIVE APPROACH: {}", s))
                    .unwrap_or_default();

                self.message = format!(
                    "*** ROOM CLEARED! ***  +{} gold{}  [ Press ENTER ]\n\n{}{}{}{}",
                    earned,
                    if self.hints_used_room == 0 {
                        " (perfect!)"
                    } else {
                        ""
                    },
                    self.room().narrative.success,
                    item_msg,
                    codex_msg,
                    alt
                );
                self.message_style = Style::default().fg(Color::Yellow);
            }
            Ok(ValidationResult::CompileError(err)) => {
                self.compile_errors_total += 1;
                self.hp = self.hp.saturating_sub(
                    self.room()
                        .scoring
                        .as_ref()
                        .and_then(|s| s.wrong_answer_penalty_hp)
                        .unwrap_or(2),
                );
                self.message = format!("{}\n\n{}", self.room().narrative.failure_compile, err);
                self.message_style = Style::default().fg(Color::Red);
            }
            Ok(ValidationResult::WrongOutput { expected, got }) => {
                self.hp = self.hp.saturating_sub(
                    self.room()
                        .scoring
                        .as_ref()
                        .and_then(|s| s.wrong_answer_penalty_hp)
                        .unwrap_or(2),
                );
                let expected_lines = expected.lines().count();
                let got_lines = got.lines().count();
                let line_hint = if got_lines > expected_lines {
                    format!(
                        "\n\n(Your output has {} lines, expected {}—are you printing too much?)",
                        got_lines, expected_lines
                    )
                } else if got_lines < expected_lines {
                    format!(
                        "\n\n(Your output has {} lines, expected {}—are you missing something?)",
                        got_lines, expected_lines
                    )
                } else {
                    String::new()
                };
                self.message = format!(
                    "{}\n\nExpected:\n{}\n\nGot:\n{}{}",
                    self.room().narrative.failure_output,
                    expected,
                    got,
                    line_hint
                );
                self.message_style = Style::default().fg(Color::Red);
            }
            Err(e) => {
                self.message = format!("System error: {}", e);
                self.message_style = Style::default().fg(Color::Magenta);
            }
        }
    }

    fn show_inventory(&mut self) {
        self.message_scroll = 0;
        if self.inventory.is_empty() {
            self.message = "🎒 INVENTORY\n\n  (empty)\n\n  Your bag is light. Solve puzzles to collect artifacts!".to_string();
        } else {
            let items: Vec<String> = self
                .inventory
                .iter()
                .map(|item| {
                    let emoji = match item.as_str() {
                        "Sacred Scroll" => "📜",
                        "Twin Keys" => "🗝️",
                        _ => "✨",
                    };
                    format!("  {} {}", emoji, item)
                })
                .collect();
            self.message = format!(
                "🎒 INVENTORY\n\n{}\n\n  {} item(s) collected",
                items.join("\n"),
                self.inventory.len()
            );
        }
        self.message_style = Style::default().fg(Color::Cyan);
    }

    fn show_keys(&mut self) {
        self.message_scroll = 0;
        let scroll_key = if cfg!(target_os = "macos") {
            "Fn+↑/↓"
        } else {
            "PgUp/Dn"
        };
        self.message = format!(
            "KEYBOARD SHORTCUTS

 GAME
  F5 / Ctrl+R   Run code
  F1            Show hint (-5 HP)
  {}       Scroll messages
  :             Enter command mode

 NAVIGATION
  ←↑↓→          Move cursor
  Home/End      Start/end of line
  Ctrl+←/→      Jump by word
  Ctrl+Home/End Start/end of file

 EDITING
  Ctrl+Z        Undo
  Ctrl+Shift+Z  Redo
  Ctrl+Y        Yank (copy) line
  Ctrl+P        Paste line below
  Ctrl+D        Delete entire line
  Ctrl+K        Delete to end of line
  Ctrl+U        Delete to start of line
  Ctrl+W        Delete word before cursor

 COMMANDS (:)
  :q            Quit game
  :keys         This help screen
  :inv          Show inventory
  :codex        Open Codex
  :5            Jump to line 5
  :top :bot     Jump to start/end",
            scroll_key
        );
        self.message_style = Style::default().fg(Color::Cyan);
    }

    fn delete_line(&mut self) {
        let (row, _) = self.editor.cursor();
        if self.is_line_locked(row) {
            self.message =
                "That line is sealed by ancient magic. It cannot be changed.".to_string();
            self.message_style = Style::default().fg(Color::Magenta);
            return;
        }
        // Move to start of line, select to end, delete
        self.editor.move_cursor(tui_textarea::CursorMove::Head);
        self.editor.move_cursor(tui_textarea::CursorMove::End);
        self.editor.start_selection();
        self.editor.move_cursor(tui_textarea::CursorMove::Head);
        self.editor.cut();
        // Remove the now-empty line if not the only line
        if self.editor.lines().len() > 1 {
            self.editor.delete_newline();
        }
    }

    fn goto_line(&mut self, line: usize) {
        let max_line = self.editor.lines().len();
        let target = line.min(max_line).saturating_sub(1);
        // Move to top first, then down to target
        self.editor.move_cursor(tui_textarea::CursorMove::Top);
        for _ in 0..target {
            self.editor.move_cursor(tui_textarea::CursorMove::Down);
        }
        self.editor.move_cursor(tui_textarea::CursorMove::Head);
        self.message = format!("Line {}/{}", target + 1, max_line);
        self.message_style = Style::default().fg(Color::DarkGray);
    }

    fn goto_top(&mut self) {
        self.editor.move_cursor(tui_textarea::CursorMove::Top);
        self.editor.move_cursor(tui_textarea::CursorMove::Head);
    }

    fn goto_bottom(&mut self) {
        self.editor.move_cursor(tui_textarea::CursorMove::Bottom);
        self.editor.move_cursor(tui_textarea::CursorMove::Head);
    }

    fn yank_line(&mut self) {
        let (row, _) = self.editor.cursor();
        if let Some(line) = self.editor.lines().get(row) {
            self.yank_buffer = line.clone();
            self.message = format!(
                "Yanked: {}",
                if self.yank_buffer.len() > 40 {
                    format!("{}...", &self.yank_buffer[..40])
                } else {
                    self.yank_buffer.clone()
                }
            );
            self.message_style = Style::default().fg(Color::DarkGray);
        }
    }

    fn paste_line(&mut self) {
        if self.yank_buffer.is_empty() {
            self.message = "Nothing to paste. Use Ctrl+Y to yank a line first.".to_string();
            self.message_style = Style::default().fg(Color::DarkGray);
            return;
        }
        let (row, _) = self.editor.cursor();
        if self.is_line_locked(row) {
            self.message = "Cannot paste on a locked line.".to_string();
            self.message_style = Style::default().fg(Color::Magenta);
            return;
        }
        // Go to end of current line, insert newline, then insert yanked content
        self.editor.move_cursor(tui_textarea::CursorMove::End);
        self.editor.insert_newline();
        self.editor.insert_str(&self.yank_buffer);
        self.message = "Pasted line below.".to_string();
        self.message_style = Style::default().fg(Color::DarkGray);
    }

    fn show_hint(&mut self) {
        self.message_scroll = 0;
        let hint_count = self.room().narrative.hints.len();
        if self.hints_used_room < hint_count {
            let penalty = self
                .room()
                .scoring
                .as_ref()
                .and_then(|s| s.hint_penalty_hp)
                .unwrap_or(5);
            let hint = self.room().narrative.hints[self.hints_used_room].clone();
            self.hp = self.hp.saturating_sub(penalty);
            self.message = format!("HINT: {}", hint);
            self.message_style = Style::default().fg(Color::Cyan);
            self.hints_used_room += 1;
            self.hints_used_total += 1;
        } else {
            self.message = "No more hints available. You're on your own...".to_string();
            self.message_style = Style::default().fg(Color::DarkGray);
        }
    }
}

fn main() -> Result<()> {
    let floor_path = std::path::Path::new("puzzles/floor_01_ownership");
    let rooms = load_floor(floor_path)?;

    if rooms.is_empty() {
        eprintln!("No rooms found in {:?}", floor_path);
        return Ok(());
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(rooms);

    loop {
        terminal.draw(|f| draw_ui(f, &app))?;

        let event = event::read()?;

        // Ignore mouse events
        if matches!(event, Event::Mouse(_)) {
            continue;
        }

        if let Event::Key(key) = event {
            // Global Ctrl+C handler - always quit
            if key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL {
                break;
            }

            match app.state {
                GameState::TitleScreen => {
                    match key.code {
                        KeyCode::Up | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('k') => {
                            app.menu_selection = app.menu_selection.next();
                        }
                        KeyCode::Enter => match app.menu_selection {
                            MenuOption::NewGame => app.start_game(),
                            MenuOption::Quit => break,
                        },
                        KeyCode::Char('q') => break,
                        _ => {}
                    }
                    continue;
                }
                GameState::RoomComplete => {
                    match key.code {
                        KeyCode::Enter => app.advance_room(),
                        KeyCode::Esc => {
                            // Return to playing state (escape from stuck states)
                            app.state = GameState::Playing;
                            app.message = "Press F5 to run your solution.".to_string();
                            app.message_style = Style::default().fg(Color::Yellow);
                        }
                        KeyCode::PageDown => {
                            let lines = app.message.lines().count() as u16;
                            if app.message_scroll < lines.saturating_sub(5) {
                                app.message_scroll += 3;
                            }
                        }
                        KeyCode::PageUp => {
                            app.message_scroll = app.message_scroll.saturating_sub(3);
                        }
                        _ => {}
                    }
                    continue;
                }
                GameState::RoomTransition => {
                    match key.code {
                        KeyCode::Enter => {
                            // Load the next room after showing transition
                            app.load_room(app.current_room + 1);
                        }
                        KeyCode::PageDown => {
                            let lines = app.message.lines().count() as u16;
                            if app.message_scroll < lines.saturating_sub(5) {
                                app.message_scroll += 3;
                            }
                        }
                        KeyCode::PageUp => {
                            app.message_scroll = app.message_scroll.saturating_sub(3);
                        }
                        _ => {}
                    }
                    continue;
                }
                GameState::LevelComplete => {
                    match key.code {
                        KeyCode::Enter => {
                            if app.current_level < 3 {
                                match app.load_level(app.current_level + 1) {
                                    Ok(()) => {}
                                    Err(e) => {
                                        app.message = format!("Cannot proceed: {}", e);
                                        app.message_style = Style::default().fg(Color::Red);
                                    }
                                }
                            } else {
                                // Game complete!
                                break;
                            }
                        }
                        KeyCode::PageDown => {
                            let lines = app.message.lines().count() as u16;
                            if app.message_scroll < lines.saturating_sub(5) {
                                app.message_scroll += 3;
                            }
                        }
                        KeyCode::PageUp => {
                            app.message_scroll = app.message_scroll.saturating_sub(3);
                        }
                        _ => {}
                    }
                    continue;
                }
                GameState::GameOver => {
                    break;
                }
                GameState::ViewingCodex => {
                    match key.code {
                        KeyCode::Esc | KeyCode::Enter => {
                            app.state = GameState::Playing;
                        }
                        KeyCode::Up => {
                            app.codex_scroll = app.codex_scroll.saturating_sub(1);
                        }
                        KeyCode::Down => {
                            if app.codex_scroll < app.codex.len().saturating_sub(1) {
                                app.codex_scroll += 1;
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
                GameState::Playing => {}
            }

            // Command mode handling (vim-style :q)
            if app.command_mode {
                match key.code {
                    KeyCode::Esc => {
                        app.command_mode = false;
                        app.command_buffer.clear();
                    }
                    KeyCode::Enter => {
                        if app.command_buffer == "q" || app.command_buffer == "quit" {
                            break;
                        } else if app.command_buffer == "w" {
                            app.message = "There is no save... only survival.".to_string();
                            app.message_style = Style::default().fg(Color::Yellow);
                        } else if app.command_buffer == "help" {
                            app.message =
                                "Commands: :q :keys :inv :codex :hint | Type :? for all shortcuts"
                                    .to_string();
                            app.message_style = Style::default().fg(Color::Cyan);
                        } else if app.command_buffer == "hint" {
                            app.show_hint();
                        } else if app.command_buffer == "inv" || app.command_buffer == "inventory" {
                            app.show_inventory();
                        } else if app.command_buffer == "keys"
                            || app.command_buffer == "shortcuts"
                            || app.command_buffer == "?"
                        {
                            app.show_keys();
                        } else if app.command_buffer == "codex" || app.command_buffer == "j" {
                            if app.codex.is_empty() {
                                app.message =
                                    "Your codex is empty. Solve puzzles to learn!".to_string();
                                app.message_style = Style::default().fg(Color::DarkGray);
                            } else {
                                app.command_mode = false;
                                app.command_buffer.clear();
                                app.state = GameState::ViewingCodex;
                                app.codex_scroll = 0;
                                continue;
                            }
                        } else if app.command_buffer == "xyzzy" {
                            if app.room().meta.id == "torch" {
                                app.message = concat!(
                                    "*** SECRET ROOM ***\n\n",
                                    "You stand in a room with walls of pure code.\n",
                                    "Flickering runes on the floor read:\n\n",
                                    "   'Made by Bradleyd Smith'   "
                                )
                                .to_string();
                                app.message_style = Style::default()
                                    .fg(Color::Magenta)
                                    .add_modifier(Modifier::BOLD);
                            } else {
                                app.message = "A hollow voice whispers... 'Nothing happens here.'"
                                    .to_string();
                                app.message_style = Style::default().fg(Color::DarkGray);
                            }
                        } else if app.command_buffer == "restart" {
                            app.start_game();
                        } else if app.command_buffer == "top" || app.command_buffer == "0" {
                            app.goto_top();
                        } else if app.command_buffer == "bot" || app.command_buffer == "$" {
                            app.goto_bottom();
                        } else if let Some(line_str) = app.command_buffer.strip_prefix("goto ") {
                            if let Ok(line) = line_str.trim().parse::<usize>() {
                                app.goto_line(line);
                            } else {
                                app.message = format!("Invalid line number: {}", line_str);
                                app.message_style = Style::default().fg(Color::Red);
                            }
                        } else if let Ok(line) = app.command_buffer.parse::<usize>() {
                            // Bare number = goto line
                            app.goto_line(line);
                        } else if !app.command_buffer.is_empty() {
                            app.message = format!("Unknown command: {}", app.command_buffer);
                            app.message_style = Style::default().fg(Color::Red);
                        }
                        app.command_mode = false;
                        app.command_buffer.clear();
                    }
                    KeyCode::Backspace => {
                        app.command_buffer.pop();
                        if app.command_buffer.is_empty() {
                            app.command_mode = false;
                        }
                    }
                    KeyCode::Char(c) => {
                        app.command_buffer.push(c);
                    }
                    _ => {}
                }
                continue;
            }

            match (key.code, key.modifiers) {
                (KeyCode::Char(':'), KeyModifiers::NONE) => {
                    app.command_mode = true;
                    app.command_buffer.clear();
                    app.message_scroll = 0; // Reset scroll so command is visible
                }
                (KeyCode::Esc, _) => {
                    app.message = "Type :q to quit".to_string();
                    app.message_style = Style::default().fg(Color::DarkGray);
                    app.message_scroll = 0;
                }
                (KeyCode::PageDown, _) => {
                    let lines = app.message.lines().count() as u16;
                    if app.message_scroll < lines.saturating_sub(5) {
                        app.message_scroll += 3;
                    }
                }
                (KeyCode::PageUp, _) => {
                    app.message_scroll = app.message_scroll.saturating_sub(3);
                }
                (KeyCode::F(5), _) | (KeyCode::Char('r'), KeyModifiers::CONTROL) => {
                    app.run_solution();
                }
                (KeyCode::F(1), _) => {
                    app.show_hint();
                }
                (KeyCode::F(2), _) => {
                    app.show_keys();
                }
                (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                    app.delete_line();
                }
                (KeyCode::Home, KeyModifiers::CONTROL) => {
                    app.goto_top();
                }
                (KeyCode::End, KeyModifiers::CONTROL) => {
                    app.goto_bottom();
                }
                (KeyCode::Char('g'), KeyModifiers::CONTROL) => {
                    // Show current position
                    let (row, col) = app.editor.cursor();
                    let max = app.editor.lines().len();
                    app.message = format!("Line {}/{}, Col {}", row + 1, max, col + 1);
                    app.message_style = Style::default().fg(Color::DarkGray);
                }
                (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                    app.yank_line();
                }
                (KeyCode::Char('p'), KeyModifiers::CONTROL) => {
                    app.paste_line();
                }
                (KeyCode::Char('z'), KeyModifiers::CONTROL) => {
                    app.editor.undo();
                }
                (KeyCode::Char('Z'), KeyModifiers::CONTROL | KeyModifiers::SHIFT) => {
                    app.editor.redo();
                }
                _ => {
                    let (cursor_row, _) = app.editor.cursor();
                    let is_destructive = matches!(
                        key.code,
                        KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete | KeyCode::Enter
                    );

                    if is_destructive && app.is_line_locked(cursor_row) {
                        app.message = "That line is sealed by ancient magic. It cannot be changed."
                            .to_string();
                        app.message_style = Style::default().fg(Color::Magenta);
                    } else {
                        app.editor.input(key);
                    }
                }
            }
        }

        if app.hp == 0 {
            app.state = GameState::GameOver;
            app.message = "OWNED\n\nThe borrow checker wins. Your HP has reached zero.".to_string();
            app.message_style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
            terminal.draw(|f| draw_ui(f, &app))?;
            std::thread::sleep(std::time::Duration::from_secs(3));
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    match app.state {
        GameState::LevelComplete => {
            if app.current_level >= 3 {
                println!();
                println!("    ╔═══════════════════════════════════════════════════╗");
                println!("    ║                                                   ║");
                println!("    ║         R U S T   R A I D   C O M P L E T E       ║");
                println!("    ║                                                   ║");
                println!("    ║       You have conquered the Borrow Dungeon!      ║");
                println!("    ║                                                   ║");
                println!("    ╚═══════════════════════════════════════════════════╝");
                println!();
                println!("    The borrow checker bows before your mastery.");
                println!();
                println!("    ┌─────────────────────────────────────┐");
                println!("    │  FINAL STATS                        │");
                println!("    ├─────────────────────────────────────┤");
                println!("    │  Gold Collected:    {:>15}  │", app.gold);
                println!("    │  HP Remaining:      {:>15}  │", app.hp);
                println!(
                    "    │  Codex Entries:     {:>15}  │",
                    format!("{}/9", app.codex.len())
                );
                println!("    │  Items:             {:>15}  │", app.inventory.len());
                println!("    └─────────────────────────────────────┘");
                println!();
                println!("    Now go forth and write Rust without fear!");
                println!();
            } else {
                println!(
                    "\nCongratulations! You've completed Level {}: {}.\n",
                    app.current_level,
                    match app.current_level {
                        1 => "Ownership",
                        2 => "Borrowing",
                        3 => "Patterns",
                        _ => "Unknown",
                    }
                );
            }
        }
        GameState::GameOver => {
            println!("\nGame Over. The borrow checker claimed another victim.\n");
        }
        _ => {}
    }

    Ok(())
}

fn draw_ui(f: &mut Frame, app: &App) {
    if matches!(app.state, GameState::TitleScreen) {
        draw_title_screen(f, app);
        return;
    }

    if matches!(app.state, GameState::ViewingCodex) {
        draw_codex(f, app);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(10),
        ])
        .split(f.area());

    // Status bar
    let room_progress = format!(
        " L{} Room {}/{} ",
        app.current_level,
        app.current_room + 1,
        app.rooms.len()
    );
    let status = Line::from(vec![
        Span::styled(
            " RUST RAID ",
            Style::default().fg(Color::Black).bg(Color::Yellow),
        ),
        Span::raw("  "),
        Span::styled(
            format!(" {} ", app.room().meta.title),
            Style::default().fg(Color::White).bg(Color::DarkGray),
        ),
        Span::raw("  "),
        Span::styled(room_progress, Style::default().fg(Color::Cyan)),
        Span::raw("  "),
        Span::styled(
            format!(" Gold: {} ", app.gold),
            Style::default().fg(Color::Yellow),
        ),
        Span::raw("  "),
        Span::styled(
            format!(" HP: {} ", app.hp),
            Style::default().fg(if app.hp > 50 {
                Color::Green
            } else if app.hp > 20 {
                Color::Yellow
            } else {
                Color::Red
            }),
        ),
    ]);
    let status_block = Paragraph::new(status).block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(status_block, chunks[0]);

    // Main content: narrative + editor
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(chunks[1]);

    let narrative = Paragraph::new(app.room().narrative.intro.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" The Chamber "),
        )
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(Color::White));
    f.render_widget(narrative, main_chunks[0]);

    render_editor(f, app, main_chunks[1]);

    // Message area
    let message_content = if app.command_mode {
        format!(":{}", app.command_buffer)
    } else {
        app.message.clone()
    };
    let (message_style, message_title) = if app.command_mode {
        (
            Style::default().fg(Color::White).bg(Color::DarkGray),
            " Command ",
        )
    } else {
        match app.state {
            GameState::RoomComplete => (
                Style::default().fg(Color::Black).bg(Color::Green),
                " VICTORY! ",
            ),
            GameState::RoomTransition => (Style::default().fg(Color::Cyan), " Descending... "),
            GameState::LevelComplete => (
                Style::default().fg(Color::Black).bg(Color::Yellow),
                " LEVEL COMPLETE! ",
            ),
            GameState::GameOver => (
                Style::default().fg(Color::White).bg(Color::Red),
                " GAME OVER ",
            ),
            GameState::Playing | GameState::TitleScreen | GameState::ViewingCodex => {
                (app.message_style, " Compiler Whispers ")
            }
        }
    };
    let scroll_indicator = if app.message.lines().count() > 8 {
        let scroll_keys = if cfg!(target_os = "macos") {
            "Fn+Up/Down"
        } else {
            "PgUp/PgDn"
        };
        format!("{} [{} to scroll]", message_title, scroll_keys)
    } else {
        message_title.to_string()
    };
    let message = Paragraph::new(message_content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(scroll_indicator),
        )
        .wrap(Wrap { trim: false })
        .style(message_style)
        .scroll((app.message_scroll, 0));
    f.render_widget(message, chunks[2]);
}

fn render_editor(f: &mut Frame, app: &App, area: Rect) {
    f.render_widget(&app.editor, area);
}

fn draw_codex(f: &mut Frame, app: &App) {
    let area = f.area();

    // Build codex content
    let mut lines: Vec<Line> = vec![
        Line::from(vec![Span::styled(
            "══════════════════════════════════════════════════",
            Style::default().fg(Color::Yellow),
        )]),
        Line::from(vec![Span::styled(
            "              ADVENTURER'S CODEX",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            "══════════════════════════════════════════════════",
            Style::default().fg(Color::Yellow),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Knowledge gained from the depths of the dungeon.",
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(vec![Span::styled(
            "  Press Esc to close. ↑/↓ to scroll.",
            Style::default().fg(Color::DarkGray),
        )]),
        Line::from(""),
    ];

    // Add each codex entry with its description
    for entry in app.codex.iter() {
        lines.push(Line::from(vec![
            Span::styled("  ◆ ", Style::default().fg(Color::Green)),
            Span::styled(
                &entry.title,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));

        for desc_line in entry.description.lines() {
            lines.push(Line::from(vec![Span::styled(
                format!("      {}", desc_line),
                Style::default().fg(Color::White),
            )]));
        }
        lines.push(Line::from(""));
    }

    // Show locked entries hint
    let total_possible = 9; // 3 rooms × 3 levels
    let unlocked = app.codex.len();
    if unlocked < total_possible {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            format!(
                "  ○ {} more entries to discover...",
                total_possible - unlocked
            ),
            Style::default().fg(Color::DarkGray),
        )]));
    }

    let codex = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(" Codex [Esc to close] "),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(codex, area);
}

fn draw_title_screen(f: &mut Frame, app: &App) {
    let area = f.area();

    let title_art = r#"
    ╔═══════════════════════════════════════════════════════════╗
    ║                                                           ║
    ║              ██████╗ ██╗   ██╗███████╗████████╗           ║
    ║              ██╔══██╗██║   ██║██╔════╝╚══██╔══╝           ║
    ║              ██████╔╝██║   ██║███████╗   ██║              ║
    ║              ██╔══██╗██║   ██║╚════██║   ██║              ║
    ║              ██║  ██║╚██████╔╝███████║   ██║              ║
    ║              ╚═╝  ╚═╝ ╚═════╝ ╚══════╝   ╚═╝              ║
    ║                                                           ║
    ║              ██████╗  █████╗ ██╗██████╗                   ║
    ║              ██╔══██╗██╔══██╗██║██╔══██╗                  ║
    ║              ██████╔╝███████║██║██║  ██║                  ║
    ║              ██╔══██╗██╔══██║██║██║  ██║                  ║
    ║              ██║  ██║██║  ██║██║██████╔╝                  ║
    ║              ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝╚═════╝                   ║
    ║                                                           ║
    ║              "Raid the Borrow Dungeon"                    ║
    ║                                                           ║
    ╚═══════════════════════════════════════════════════════════╝
"#;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(22),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);

    let title = Paragraph::new(title_art)
        .style(Style::default().fg(Color::Yellow))
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(title, chunks[0]);

    let new_game_style = if matches!(app.menu_selection, MenuOption::NewGame) {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let new_game = Paragraph::new("  NEW GAME  ")
        .style(new_game_style)
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(new_game, chunks[1]);

    let quit_style = if matches!(app.menu_selection, MenuOption::Quit) {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let quit = Paragraph::new("  QUIT  ")
        .style(quit_style)
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(quit, chunks[2]);

    let help = Paragraph::new("↑/↓ to select  •  ENTER to confirm  •  q to quit")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(help, chunks[3]);
}

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
use puzzle::{load_floor, Room};

enum GameState {
    TitleScreen,
    Playing,
    RoomComplete,
    LevelComplete,
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
    editor: TextArea<'a>,
    locked_lines: Vec<usize>,
    message: String,
    message_style: Style,
    message_scroll: u16,
    state: GameState,
    menu_selection: MenuOption,
    hp: u32,
    gold: u32,
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
            editor,
            locked_lines,
            message: String::from("Fix the code. The compiler will guide you..."),
            message_style: Style::default().fg(Color::Yellow),
            message_scroll: 0,
            state: GameState::TitleScreen,
            menu_selection: MenuOption::NewGame,
            hp: 100,
            gold: 0,
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
        self.hints_used_room = 0;
        self.hints_used_total = 0;
        self.compile_errors_total = 0;
        self.load_room(0);
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
                .title(" Code Editor [F5: Run | F1: Hint | :q Quit] "),
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
            self.load_room(self.current_room + 1);
        } else {
            self.state = GameState::LevelComplete;
            let perfect = self.hints_used_total == 0 && self.compile_errors_total == 0;
            self.message = format!(
                "=== LEVEL 1 COMPLETE! ===\n\n\
                You've mastered the art of Ownership.{}\n\n\
                ╔══════════════════════════╗\n\
                ║  LEVEL STATS             ║\n\
                ╠══════════════════════════╣\n\
                ║  Rooms cleared:    {:>4}  ║\n\
                ║  Compile errors:   {:>4}  ║\n\
                ║  Hints used:       {:>4}  ║\n\
                ║  Gold earned:      {:>4}  ║\n\
                ║  HP remaining:     {:>4}  ║\n\
                ╚══════════════════════════╝\n\n\
                Press any key to exit...",
                if perfect { " PERFECT RUN!" } else { "" },
                self.rooms.len(),
                self.compile_errors_total,
                self.hints_used_total,
                self.gold,
                self.hp
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

                let alt = self
                    .room()
                    .narrative
                    .alternative_solution
                    .as_ref()
                    .map(|s| format!("\n\nALTERNATIVE APPROACH: {}", s))
                    .unwrap_or_default();

                self.message = format!(
                    "*** ROOM CLEARED! ***  +{} gold{}  [ Press ENTER ]\n\n{}{}",
                    earned,
                    if self.hints_used_room == 0 { " (perfect!)" } else { "" },
                    self.room().narrative.success,
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
                    format!("\n\n(Your output has {} lines, expected {}—are you printing too much?)", got_lines, expected_lines)
                } else if got_lines < expected_lines {
                    format!("\n\n(Your output has {} lines, expected {}—are you missing something?)", got_lines, expected_lines)
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

        if let Event::Key(key) = event::read()? {
            match app.state {
                GameState::TitleScreen => {
                    match key.code {
                        KeyCode::Up | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('k') => {
                            app.menu_selection = app.menu_selection.next();
                        }
                        KeyCode::Enter => {
                            match app.menu_selection {
                                MenuOption::NewGame => app.start_game(),
                                MenuOption::Quit => break,
                            }
                        }
                        KeyCode::Char('q') => break,
                        _ => {}
                    }
                    continue;
                }
                GameState::RoomComplete => {
                    if key.code == KeyCode::Enter {
                        app.advance_room();
                    }
                    continue;
                }
                GameState::LevelComplete | GameState::GameOver => {
                    break;
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
                            app.message = "Commands: :q (quit), :hint (show hint)".to_string();
                            app.message_style = Style::default().fg(Color::Cyan);
                        } else if app.command_buffer == "hint" {
                            app.show_hint();
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
                _ => {
                    let (cursor_row, _) = app.editor.cursor();
                    let is_destructive = matches!(
                        key.code,
                        KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete | KeyCode::Enter
                    );

                    if is_destructive && app.is_line_locked(cursor_row) {
                        app.message =
                            "That line is sealed by ancient magic. It cannot be changed."
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
            app.message_style = Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD);
            terminal.draw(|f| draw_ui(f, &app))?;
            std::thread::sleep(std::time::Duration::from_secs(3));
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    match app.state {
        GameState::LevelComplete => {
            println!("\nCongratulations! You've completed Level 1: Ownership.\n");
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
        " Room {}/{} ",
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
        Span::styled(
            room_progress,
            Style::default().fg(Color::Cyan),
        ),
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
        (Style::default().fg(Color::White).bg(Color::DarkGray), " Command ")
    } else {
        match app.state {
            GameState::RoomComplete => (
                Style::default().fg(Color::Black).bg(Color::Green),
                " VICTORY! "
            ),
            GameState::LevelComplete => (
                Style::default().fg(Color::Black).bg(Color::Yellow),
                " LEVEL COMPLETE! "
            ),
            GameState::GameOver => (
                Style::default().fg(Color::White).bg(Color::Red),
                " GAME OVER "
            ),
            GameState::Playing | GameState::TitleScreen => (app.message_style, " Compiler Whispers "),
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
        Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let new_game = Paragraph::new("  NEW GAME  ")
        .style(new_game_style)
        .alignment(ratatui::layout::Alignment::Center);
    f.render_widget(new_game, chunks[1]);

    let quit_style = if matches!(app.menu_selection, MenuOption::Quit) {
        Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
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

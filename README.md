# Rust Raid

**A terminal-based adventure game to master Rust's ownership and borrowing rules.**

![Title Screen](images/rust-raid-intro.png)

---

## Concept

*Rust Raid* turns the notoriously tricky concepts of Rust's borrow checker into an interactive dungeon crawl. Instead of fighting monsters with swords, you fight the compiler with code. Each room presents a small Rust program with a compile error related to ownership or borrowing. Your goal is to fix the code and appease the borrow checker to unlock the next room.

The game is designed to build a strong mental model for how Rust's memory management rules work in a fun, practical, and engaging way.

## How to Play

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (which includes `cargo`)

### Running the Game

1. Clone this repository.
2. Navigate to the project directory.
3. Run the game with `cargo`:

   ```sh
   cargo run --release
   ```

   *(Using the `--release` flag is recommended for better performance.)*

## Gameplay

The screen is divided into three main parts:

1. **The Chamber:** On the left, you'll find the narrative intro for the current puzzle, setting the scene for your task.
2. **Code Editor:** On the right is the code you need to fix. Some lines may be "sealed by ancient magic" (i.e., locked and uneditable).
3. **Compiler Whispers:** At the bottom, the compiler will give you messages. It will show you the errors in your code, hints, or success messages.

Your goal is to edit the code so that it compiles and produces the exact `expected_output` for the puzzle.

## Keybindings

### Game Controls

| Key | Action |
|---|---|
| `F5` / `Ctrl+R` | Run your solution |
| `F1` | Request a hint (-5 HP) |
| `PgUp`/`PgDn` | Scroll message panel |
| `:` | Enter Command Mode |

### Editing

| Key | Action |
|---|---|
| `←↑↓→` | Move cursor |
| `Home`/`End` | Go to start/end of line |
| `Ctrl` + `←`/`→` | Jump by word |
| `Ctrl` + `Home`/`End` | Go to start/end of file |
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo / Redo |
| `Ctrl+Y` | Yank (copy) current line |
| `Ctrl+P` | Paste yanked line below |
| `Ctrl+D` | Delete entire line |

### Command Mode (enter with `:`)

| Command | Action |
|---|---|
| `:q` / `:quit` | Quit the game |
| `:keys` | Show the keybindings screen |
| `:inv` | Show your inventory |
| `:codex` | Open your codex of knowledge |
| `:5` | Jump to line 5 in the editor |
| `:top` / `:bot` | Jump to start/end of the file |

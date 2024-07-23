use std::fs::read_to_string;
use std::time::Duration;

use rusqlite::{params, Connection, Result};
use chrono::{NaiveDate, Local, Days};
use crossterm::event::{read, poll, Event, KeyCode};
use rand::seq::SliceRandom;
use rand::thread_rng;

struct Deck {
    name: String,
}

struct Flashcard {
    id: i32,
    deck_id: i32,
    front: String,
    back: String,
    added: NaiveDate,
    next: NaiveDate,
    level: i32,
}

struct FlashcardLog {
    question_id: i32,
    answer: bool,
}

fn create_table(conn: &Connection, sql: &str) -> Result<()> {
    conn.execute(sql, [])?;
    Ok(())
}

fn create_tables(conn: &Connection) -> Result<()> {
    create_table(
        conn,
        "CREATE TABLE IF NOT EXISTS decks (
            id INTEGER PRIMARY KEY,
            name TEXT
        )"
    )?;

    create_table(
        conn,
        "CREATE TABLE IF NOT EXISTS flashcards (
            id INTEGER PRIMARY KEY,
            deck_id INTEGER,
            front TEXT,
            back TEXT,
            added DATE,
            next DATE,
            level INTEGER
        )"
    )?;

    create_table(
        conn,
        "CREATE TABLE IF NOT EXISTS flashcard_log (
            question_id INTEGER,
            answer BOOLEAN
        )"
    )?;

    Ok(())
}

fn insert_deck(conn: &Connection, deck: &Deck) -> Result<()> {
    conn.execute(
        "INSERT INTO decks (name) VALUES (?1)",
        params![deck.name],
    )?;
    Ok(())
}

fn insert_flashcard(conn: &Connection, card: &Flashcard) -> Result<()> {
    conn.execute(
        "INSERT INTO flashcards (deck_id, front, back, added, next, level) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![card.deck_id, card.front, card.back, card.added, card.next, card.level],
    )?;
    Ok(())
}

fn update_flashcard_level(conn: &Connection, id: i32, level: i32, next: &NaiveDate) -> Result<()> {
    conn.execute(
        "UPDATE flashcards SET level = ?1, next = ?2 WHERE id = ?3",
        params![level, next, id],
    )?;
    Ok(())
}

fn insert_flashcard_log(conn: &Connection, log: &FlashcardLog) -> Result<()> {
    conn.execute(
        "INSERT INTO flashcard_log (question_id, answer) VALUES (?1, ?2)",
        params![log.question_id, log.answer],
    )?;
    Ok(())
}

fn init_db(conn: &Connection) {
    create_tables(&conn).unwrap();
}

fn add(conn: &Connection, args: &Vec<String>) {
    if args.len() < 3 {
        println!("Missing <subcommand>");
        return;
    }

    let command = &args[2];
    match command.as_str() {
        "deck" => {
            if args.len() < 4 {
                println!("Missing <deck_name>");
                return;
            }

            let deck = Deck {
                name: args[3].to_string(),
            };
            match insert_deck(&conn, &deck) {
                Ok(_) => {
                    println!("Deck added: {}", deck.name);
                },
                Err(e) => {
                    println!("Error adding deck: {}", e);
                }
            }
        },
        "cards" => {
            let path = args[3].to_string();
            let file = match read_to_string(path) {
                Ok(file) => file,
                Err(e) => {
                    println!("Error reading file: {}", e);
                    return;
                }
            };

            let cards: Vec<&str> = file.split("----").collect();

            let name: Vec<&str> = cards[0].split(":").collect();
            let name = name[1].trim();

            let deck_id = match get_deck_id_from_name(&conn, name) {
                Ok(id) => id,
                Err(e) => {
                    println!("Error getting deck id: {}", e);
                    return;
                }
            };

            let added_date = Local::now().naive_utc().date();

            let mut cards = parse_cards(deck_id, &cards[1..], &added_date);

            for card in &mut cards {
                match insert_flashcard(&conn, &card) {
                    Ok(_) => {
                        println!("Flashcard added: {}", card.front);
                        let row_id = conn.last_insert_rowid();
                        card.id = row_id as i32;
                    },
                    Err(e) => {
                        println!("Error adding flashcard: {}", e);
                    }
                }
            }
        },
        _ => {
            println!("Unknown add command: {}", command);
        }
    }
}

fn get_deck_id_from_name(conn: &Connection, name: &str) -> Result<i32> {
    let mut stmt = conn.prepare("SELECT id FROM decks WHERE name = ?1")?;
    let id: i32 = stmt.query_row(params![name], |row| row.get(0))?;
    Ok(id)
}

fn parse_cards(deck_id: i32, cards: &[&str], added_date: &NaiveDate) -> Vec<Flashcard> {
    let mut result: Vec<Flashcard> = Vec::new();

    for card in cards {
        let sides: Vec<&str> = card.split("<>").collect();
        
        if sides.len() != 2 {
            continue;
        }

        let c = Flashcard {
            id: -1, // dummy value
            deck_id,
            front: sides[0].to_string(),
            back: sides[1].to_string(),
            added: *added_date,
            next: *added_date,
            level: 1,
        };

        result.push(c);
    }

    result
}

fn clear_key_buffer() {
    // Continuously read events until there are no more pending events
    while poll(Duration::from_millis(0)).unwrap() {
        if let Event::Key(_) = read().unwrap() {
            // Simply discard the event
        }
    }
}

fn level_to_date(level: i32) -> i32 {
    match level {
        1 => 1,
        2 => 4,
        3 => 10,
        4 => 25,
        5 => 50,
        _ => 1000,
    }
}

fn quiz(conn: &Connection, args: &Vec<String>) {
    if args.len() < 3 {
        println!("Missing <deck_name>");
        return;
    }

    let deck_name = &args[2];

    let deck_id = match deck_name.parse() {
        Ok(id) => id,
        Err(_) => {
            match get_deck_id_from_name(conn, deck_name) {
                Ok(id) => id,
                Err(e) => {
                    println!("Error getting deck id: {}", e);
                    return;
                }
            }
        }
    };

    let mut stmt = conn.prepare("SELECT id, level, front, back FROM flashcards WHERE deck_id = ?1 and next <= ?2").unwrap();
    let mut rows: Vec<Result<(i32, i32, String, String)>> = stmt.query_map(params![deck_id, Local::now().naive_utc().date()], |row| {
        Ok((
            row.get::<_, i32>(0)?,
            row.get::<_, i32>(1)?,
            row.get::<_, String>(2)?, 
            row.get::<_, String>(3)?
        ))
    }).unwrap().collect();

    let mut rng = thread_rng();
    rows.shuffle(&mut rng);

    clear_key_buffer();

    for row in rows {
        let (id, level, front, back) = row.unwrap();
        println!("{}", front);
        println!("press enter to flip");

        loop {
            // Wait for an event
            if let Event::Key(key_event) = read().unwrap() {
                // Check if it's a key press event
                match key_event.code {
                    KeyCode::Enter => {
                        break;
                    }
                    _ => (),
                }
            }
        }

        println!("{}", back);
        println!("Press - O: 1, X: 2");

        loop {
            // Wait for an event
            if let Event::Key(key_event) = read().unwrap() {
                // Check if it's a key press event
                match key_event.code {
                    KeyCode::Char('1') => {
                        let next_date = Local::now().naive_local().date()
                            .checked_add_days(
                                Days::new(level_to_date(level + 1) as u64)
                            ).unwrap();
                        update_flashcard_level(conn, id, level + 1, &next_date).unwrap();
                        insert_flashcard_log(conn, &FlashcardLog { question_id: id, answer: true }).unwrap();
                        break;
                    }
                    KeyCode::Char('2') => {
                        let next_date = Local::now().naive_local().date()
                            .checked_add_days(
                                Days::new(level_to_date(level) as u64)
                            ).unwrap();
                        let next_level = if level > 1 { level - 1 } else { 1 };

                        update_flashcard_level(conn, id, next_level, &next_date).unwrap();
                        insert_flashcard_log(conn, &FlashcardLog { question_id: id, answer: false }).unwrap();
                        break;
                    }
                    _ => (),
                }
            }
        }
    }
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();

    match args.len() {
        0 | 1 => {
            println!("Missing <command>");
            return;
        },
        _ => {}
    }

    let command = &args[1];
    let conn = Connection::open("flashcards.db").unwrap();

    match command.as_str() {
        "init" => init_db(&conn),
        "add" => add(&conn, &args),
        "quiz" => quiz(&conn, &args),
        _ => {
            println!("Unknown command: {}", command);
        }
    }
}

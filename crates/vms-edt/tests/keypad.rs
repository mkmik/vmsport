//! EDT's keypad mode, driven by keys against its screen model; and what
//! PRINT writes (VMS's DUMP of it: fixtures/edt/recorded/run3.log).

use libvms::term::Key;
use vms_edt::keypad::{After, Keypad};
use vms_edt::{Edt, Files, Flow};

#[derive(Default)]
struct Mem {
    written: Vec<(String, Vec<String>)>,
}

impl Files for Mem {
    fn read(&mut self, spec: &str) -> Result<Vec<String>, String> {
        Err(format!("no {spec}"))
    }

    fn write(&mut self, spec: &str, lines: &[String]) -> Result<String, String> {
        self.written.push((spec.into(), lines.to_vec()));
        Ok(format!("DISK:[T]{spec};2"))
    }
}

fn edt(lines: &[&str]) -> Edt {
    let mut e = Edt::new("A.TXT", Some(lines.iter().map(|s| s.to_string()).collect()));
    e.terminal = true;
    e
}

fn text(e: &Edt) -> Vec<String> {
    e.buffers[0].texts()
}

/// Keys: a string is typed, a Key pressed.
fn press(k: &mut Keypad, e: &mut Edt, f: &mut Mem, keys: &[Key]) -> After {
    let mut last = After::Stay;
    for key in keys {
        last = k.key(e, key.clone(), f);
    }
    last
}

fn typed(s: &str) -> Vec<Key> {
    s.chars().map(Key::Char).collect()
}

const GOLD: Key = Key::Pf(1);

#[test]
fn print_writes_a_form_feed_two_empty_records_then_typed_lines() {
    let mut e = edt(&["one", "two", "three"]);
    let mut f = Mem::default();
    assert_eq!(e.command("PRINT P.LIS 2:3", &mut f), Flow::Go);
    assert_eq!(e.take(), Vec::<String>::new());
    assert_eq!(
        f.written,
        [(
            "P.LIS".to_string(),
            vec![
                "\x0c".into(),
                "".into(),
                "".into(),
                "    2       two".into(),
                "    3       three".into()
            ]
        )]
    );
}

#[test]
fn typing_moving_and_the_screen() {
    let mut e = edt(&[]);
    let mut f = Mem::default();
    let mut k = Keypad::new();
    let mut keys = typed("hello world");
    keys.extend([Key::Return]);
    keys.extend(typed("second"));
    press(&mut k, &mut e, &mut f, &keys);
    assert_eq!(text(&e), ["hello world", "second"]);
    let s = k.screen(&e, 24, 80);
    assert_eq!(&s.rows[..3], ["hello world", "second", "[EOB]"]);
    assert_eq!(s.cursor, (1, 6));
    // TOP, then WORD three times: "world", the line's end, "second".
    press(
        &mut k,
        &mut e,
        &mut f,
        &[GOLD, Key::Kp('5'), Key::Kp('1'), Key::Kp('1'), Key::Kp('1')],
    );
    assert_eq!((e.buf().cur, e.buf().col), (1, 0));
    // BACKUP, WORD twice: the line end before, then "world"; ADVANCE,
    // EOL: the end of the line.
    let keys = [
        Key::Kp('5'),
        Key::Kp('1'),
        Key::Kp('1'),
        Key::Kp('4'),
        Key::Kp('2'),
    ];
    press(&mut k, &mut e, &mut f, &keys);
    assert_eq!((e.buf().cur, e.buf().col), (0, 11));
    // GOLD 3 BACKUP CHAR: three back.
    press(
        &mut k,
        &mut e,
        &mut f,
        &[Key::Kp('5'), GOLD, Key::Char('3'), Key::Kp('3')],
    );
    assert_eq!(e.buf().col, 8);
    // Arrows keep the column; LINE goes to the next line's start.
    press(&mut k, &mut e, &mut f, &[Key::Kp('4'), Key::Down]);
    assert_eq!((e.buf().cur, e.buf().col), (1, 6));
    press(&mut k, &mut e, &mut f, &[Key::Up, Key::Kp('0')]);
    assert_eq!((e.buf().cur, e.buf().col), (1, 0));
}

#[test]
fn deletes_and_undeletes() {
    let mut e = edt(&["alpha beta gamma", "two", "three"]);
    let mut f = Mem::default();
    let mut k = Keypad::new();
    // DEL W deletes "alpha "; UND W puts it back.
    press(&mut k, &mut e, &mut f, &[Key::Kp('-')]);
    assert_eq!(text(&e)[0], "beta gamma");
    press(&mut k, &mut e, &mut f, &[GOLD, Key::Kp('-')]);
    assert_eq!(text(&e)[0], "alpha beta gamma");
    // DEL C, then DEL L takes the rest of the line with its end.
    press(
        &mut k,
        &mut e,
        &mut f,
        &[GOLD, Key::Kp('5'), Key::Kp(','), Key::Pf(4)],
    );
    assert_eq!(text(&e), ["two", "three"]);
    press(&mut k, &mut e, &mut f, &[GOLD, Key::Pf(4)]);
    assert_eq!(text(&e), ["lpha beta gamma", "two", "three"]);
    // DELETE deletes back over a line end; Ctrl/U to the line's start.
    press(&mut k, &mut e, &mut f, &[Key::Delete]);
    assert_eq!(text(&e), ["lpha beta gammatwo", "three"]);
    press(&mut k, &mut e, &mut f, &[Key::Ctrl('U')]);
    assert_eq!(text(&e), ["two", "three"]);
    // Ctrl/J deletes the word before the cursor.
    press(&mut k, &mut e, &mut f, &[Key::Kp('2'), Key::Ctrl('J')]);
    assert_eq!(text(&e), ["", "three"]);
}

#[test]
fn select_cut_paste_append_replace() {
    let mut e = edt(&["one two three", "four"]);
    let mut f = Mem::default();
    let mut k = Keypad::new();
    // SELECT, WORD: "one " is selected (shown reversed), CUT, then PASTE
    // at the end of the line.
    press(&mut k, &mut e, &mut f, &[Key::Kp('.'), Key::Kp('1')]);
    assert_eq!(k.screen(&e, 24, 80).reverse[0], Some((0, 4)));
    press(&mut k, &mut e, &mut f, &[Key::Kp('6')]);
    assert_eq!(text(&e)[0], "two three");
    assert_eq!(e.buffers[1].texts(), ["one "]);
    press(&mut k, &mut e, &mut f, &[Key::Kp('2'), GOLD, Key::Kp('6')]);
    assert_eq!(text(&e)[0], "two threeone ");
    // CUT without a select range says so.
    press(&mut k, &mut e, &mut f, &[Key::Kp('6')]);
    assert_eq!(k.message, "No select range active");
    // A whole line: SELECT, LINE, CUT; PASTE puts it back with its end.
    press(
        &mut k,
        &mut e,
        &mut f,
        &[GOLD, Key::Kp('5'), Key::Kp('.'), Key::Kp('0'), Key::Kp('6')],
    );
    assert_eq!(text(&e), ["four"]);
    press(&mut k, &mut e, &mut f, &[GOLD, Key::Kp('6')]);
    assert_eq!(text(&e), ["two threeone ", "four"]);
    // APPEND adds to the paste buffer; REPLACE puts it over a selection.
    press(
        &mut k,
        &mut e,
        &mut f,
        &[Key::Kp('.'), Key::Kp('3'), Key::Kp('9')],
    );
    assert_eq!(e.buffers[1].texts(), ["two threeone ", "f"]);
    press(
        &mut k,
        &mut e,
        &mut f,
        &[Key::Kp('.'), Key::Kp('3'), GOLD, Key::Kp('9')],
    );
    assert_eq!(text(&e), ["two threeone ", "two threeone ", "fur"]);
}

#[test]
fn find_subs_command_and_back_to_line_mode() {
    let mut e = edt(&["apple pie", "apple tart", "pear"]);
    let mut f = Mem::default();
    let mut k = Keypad::new();
    let mut keys = vec![GOLD, Key::Pf(3)];
    keys.extend(typed("apple"));
    assert_eq!(press(&mut k, &mut e, &mut f, &keys), After::Stay);
    assert_eq!(k.screen(&e, 24, 80).rows[23], "Search for: apple");
    press(&mut k, &mut e, &mut f, &[Key::KpEnter]);
    // From the start, the next match is on the second line.
    assert_eq!((e.buf().cur, e.buf().col), (1, 0));
    press(&mut k, &mut e, &mut f, &[Key::Pf(3)]);
    assert_eq!(k.message, "String was not found");
    // SUBS: the match at the cursor becomes the paste buffer's text (a
    // whole line, with its end).
    e.command("COPY 3 TO =PASTE", &mut f);
    e.take();
    press(&mut k, &mut e, &mut f, &[GOLD, Key::KpEnter]);
    assert_eq!(&text(&e)[1..3], ["pear", " tart"]);
    // CHNGCASE, OPEN LINE.
    press(
        &mut k,
        &mut e,
        &mut f,
        &[GOLD, Key::Kp('5'), GOLD, Key::Kp('1')],
    );
    assert_eq!(text(&e)[0], "Apple pie");
    press(&mut k, &mut e, &mut f, &[GOLD, Key::Kp('0')]);
    assert_eq!(&text(&e)[..2], ["A", "pple pie"]);
    // COMMAND: a line-mode command; EXIT ends EDT.
    let mut keys = vec![GOLD, Key::Kp('7')];
    keys.extend(typed("EXIT"));
    keys.push(Key::KpEnter);
    assert_eq!(
        press(&mut k, &mut e, &mut f, &keys),
        After::Done(Flow::Exit { save: false })
    );
    assert_eq!(f.written[0].0, "A.TXT");
    // Ctrl/Z: back to line mode.
    assert_eq!(
        press(&mut k, &mut e, &mut f, &[Key::Ctrl('Z')]),
        After::LineMode
    );
}

#[test]
fn scrolling_keeps_the_cursor_between_set_cursor_rows() {
    let lines: Vec<String> = (1..=100).map(|i| format!("line {i}")).collect();
    let refs: Vec<&str> = lines.iter().map(|s| s.as_str()).collect();
    let mut e = edt(&refs);
    let mut f = Mem::default();
    let mut k = Keypad::new();
    // SECT moves 16 lines; twice is line 33, on cursor row 14 of 7:14.
    press(
        &mut k,
        &mut e,
        &mut f,
        &[Key::Kp('8'), Key::Kp('8'), Key::Kp('0')],
    );
    let s = k.screen(&e, 24, 80);
    assert_eq!(e.buf().cur, 33);
    assert_eq!(s.cursor.0, 14);
    assert_eq!(s.rows[14], "line 34");
    // BOTTOM: [EOB] on the screen.
    press(&mut k, &mut e, &mut f, &[GOLD, Key::Kp('4')]);
    let s = k.screen(&e, 24, 80);
    assert_eq!(s.rows[s.cursor.0], "[EOB]");
}

#[test]
fn defined_keys_run_nokeypad_commands() {
    let mut e = edt(&["one two", "three"]);
    let mut f = Mem::default();
    e.command("DEFINE KEY CONTROL X AS \"D+L.\"", &mut f);
    e.command("DEFINE KEY GOLD CONTROL A AS \"Ihi ^Z.\"", &mut f);
    assert_eq!(e.take(), Vec::<String>::new());
    let mut k = Keypad::new();
    press(&mut k, &mut e, &mut f, &[Key::Ctrl('X')]);
    assert_eq!(text(&e), ["three"]);
    press(&mut k, &mut e, &mut f, &[GOLD, Key::Ctrl('A')]);
    assert_eq!(text(&e), ["hi three"]);
}

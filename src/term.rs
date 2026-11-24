//! The terminal: full screen on /dev/tty (so stdout stays free for `--print-dir`), raw mode, frames
//! written as the diff against the previous one, and suspend / resume around programs run in it.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::{cursor, execute, queue, terminal};
use lvim_fuzzy_ui::canvas::{Canvas, Positioner};

/// The input thread reads the terminal only while this is on; off while another program has it.
pub static INPUT_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// The input thread says it is not reading (so the program run gets every key).
pub static INPUT_PAUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Read the terminal's events on a thread of their own, each sent to `events` with a wake-up on `wake`:
/// the UI then sleeps on ONE channel and wakes at once for a key or for any background work done.
pub fn spawn_input(events: std::sync::mpsc::Sender<crossterm::event::Event>, wake: std::sync::mpsc::Sender<()>) {
    use std::sync::atomic::Ordering;
    std::thread::spawn(move || {
        loop {
            if !INPUT_ON.load(Ordering::Acquire) {
                INPUT_PAUSED.store(true, Ordering::Release);
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            }
            INPUT_PAUSED.store(false, Ordering::Release);
            // a short poll: the flag is seen within 20 ms when another program is about to run
            match crossterm::event::poll(std::time::Duration::from_millis(20)) {
                Ok(true) => match crossterm::event::read() {
                    Ok(ev) => {
                        if events.send(ev).is_err() {
                            return;
                        }
                        let _ = wake.send(());
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
                },
                Ok(false) => {}
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
            }
        }
    });
}

/// Stop the input thread reading (and wait until it has), before another program takes the terminal.
fn pause_input() {
    use std::sync::atomic::Ordering;
    INPUT_ON.store(false, Ordering::Release);
    let t = std::time::Instant::now();
    while !INPUT_PAUSED.load(Ordering::Acquire) && t.elapsed() < std::time::Duration::from_millis(200) {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

pub struct Term {
    out: BufWriter<File>,
    pub w: u16,
    pub h: u16,
    prev: Option<Canvas>,
    pos: Positioner,
}

impl Term {
    pub fn open() -> std::io::Result<Term> {
        let tty = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        let mut out = BufWriter::with_capacity(1 << 16, tty);
        let (w, h) = match terminal::size()? {
            (0, _) | (_, 0) => (80, 24),
            s => s,
        };
        terminal::enable_raw_mode()?;
        execute!(out, terminal::EnterAlternateScreen, cursor::Hide, EnableMouseCapture)?;
        lvim_fuzzy_ui::keyboard::push(&mut out);
        out.flush()?;
        // Zellij: our pane takes the Ctrl-h / j / k / l of lvim-winnav while lvc shows
        Ok(Term {
            out,
            w,
            h,
            prev: None,
            pos: Positioner { inline: false, row: 0 },
        })
    }

    /// Bytes straight to the terminal (an OSC sequence: the clipboard).
    pub fn write_raw(&mut self, b: &[u8]) -> std::io::Result<()> {
        self.out.write_all(b)?;
        self.out.flush()
    }

    pub fn resize(&mut self, w: u16, h: u16) {
        self.w = w.max(1);
        self.h = h.max(1);
        self.prev = None;
    }

    pub fn draw(&mut self, cv: Canvas) -> std::io::Result<()> {
        queue!(self.out, terminal::BeginSynchronizedUpdate)?;
        // icon images drawn for the first time in this frame: sent before the cells that show them
        let pending = lvim_fuzzy_ui::iconimg::take_pending();
        if !pending.is_empty() {
            self.out.write_all(&pending)?;
        }
        cv.flush(&mut self.out, self.prev.as_ref(), &mut self.pos)?;
        queue!(self.out, terminal::EndSynchronizedUpdate)?;
        self.out.flush()?;
        self.prev = Some(cv);
        Ok(())
    }

    /// Hand the terminal to another program (an editor, a shell command, the finder).
    pub fn suspend(&mut self) -> std::io::Result<()> {
        pause_input();
        // the program run marks the pane itself if it wants the nav keys (Neovim does)
        lvim_fuzzy_ui::keyboard::pop(&mut self.out);
        queue!(
            self.out,
            DisableMouseCapture,
            terminal::LeaveAlternateScreen,
            cursor::Show
        )?;
        self.out.flush()?;
        terminal::disable_raw_mode()
    }

    /// Hand the terminal to a full-screen program started from this one (the finder) WITHOUT leaving the alternate
    /// screen: the shell never shows, so nothing flashes on the way out or back, and the images sent to this screen
    /// (the icons) stay. The program must draw in the screen it finds (`LVIM_FUZZY_KEEP_SCREEN`).
    pub fn suspend_in_place(&mut self) -> std::io::Result<()> {
        pause_input();
        self.out.flush()
    }

    /// Take the terminal back after [`Term::suspend_in_place`]: raw mode and the mouse again, a whole new frame.
    pub fn resume_in_place(&mut self) -> std::io::Result<()> {
        terminal::enable_raw_mode()?;
        INPUT_ON.store(true, std::sync::atomic::Ordering::Release);
        queue!(self.out, cursor::Hide, EnableMouseCapture)?;
        lvim_fuzzy_ui::keyboard::push(&mut self.out);
        self.out.flush()?;
        if let Ok((w, h)) = terminal::size() {
            self.resize(w, h);
        }
        self.prev = None;
        Ok(())
    }

    pub fn resume(&mut self) -> std::io::Result<()> {
        terminal::enable_raw_mode()?;
        INPUT_ON.store(true, std::sync::atomic::Ordering::Release);
        queue!(
            self.out,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            EnableMouseCapture
        )?;
        lvim_fuzzy_ui::keyboard::push(&mut self.out);
        self.out.flush()?;
        // kitty drops an alternate screen's images when the screen is switched: the icons (and a preview's picture)
        // are sent again as they are drawn
        lvim_fuzzy_ui::iconimg::invalidate();
        if let Ok((w, h)) = terminal::size() {
            self.resize(w, h);
        }
        self.prev = None;
        Ok(())
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        // the icon images go with the program
        let _ = self.out.write_all(&lvim_fuzzy_ui::iconimg::cleanup());
        lvim_fuzzy_ui::media::cleanup_files();
        lvim_fuzzy_ui::keyboard::pop(&mut self.out);
        let _ = queue!(
            self.out,
            DisableMouseCapture,
            terminal::LeaveAlternateScreen,
            cursor::Show
        );
        let _ = self.out.flush();
        let _ = terminal::disable_raw_mode();
    }
}

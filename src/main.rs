extern crate log;

mod interp;
mod disp;
mod input;

use interp::{Interpreter, InterpreterInput, InterpreterRequest, InterpreterKind};
use disp::{Display, Terminal};
use input::Keyboard;

use crossterm::event::{poll, read, Event, KeyCode as CrosstermKey, KeyModifiers as CrosstermKeyModifiers};
use log::LevelFilter;

use std::{
    ops::DerefMut,
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant}, io
};

const INSTRUCTION_FREQUENCY: u32 = 700;
const TIMER_FREQUENCY: u32 = 60;

#[derive(Default)]
struct CHIP8VM {
    interp: Interpreter,
    interp_input: InterpreterInput,
    display: Display,
    active: bool,
    keyboard: Keyboard,
    sound_timer: f64,
    delay_timer: f64,
}

impl CHIP8VM {
    fn exit(&mut self) {
        self.active = false;
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // arg parsing

    let mut args = std::env::args().skip(1).collect::<Vec<_>>();

    let logger_enabled = if let Some(i) = args.iter().position(|arg| arg == "--log") {
        let mut level_parsed = true;
        let level = match args
            .iter()
            .nth(i + 1)
            .map(|s| s.to_ascii_lowercase())
            .as_ref()
            .map(String::as_str) 
        {
            Some("trace") => LevelFilter::Trace,
            Some("debug") => LevelFilter::Debug,
            Some("info") => LevelFilter::Info,
            Some("warn") => LevelFilter::Warn,
            Some("error") => LevelFilter::Error,
            Some("off") => LevelFilter::Off,
            _ => {
                level_parsed = false;
                LevelFilter::Info
            }
        };

        if level_parsed {
            args.remove(i + 1);
        }

        tui_logger::init_logger(level).unwrap();
        tui_logger::set_default_level(level);

        args.remove(i);
        true
    } else {
        false
    };

    let program_kind: InterpreterKind = if let Some(i) = args.iter().position(|arg| arg == "--kind") {
        let kind = match args
            .iter()
            .nth(i + 1)
            .map(|s| s.to_ascii_lowercase())
            .as_ref()
            .map(String::as_str) 
        {
            Some("cosmacvip") => InterpreterKind::COSMACVIP,
            Some("chip48") => InterpreterKind::CHIP48,
            _ => Err("--kind must be followed by COSMACVIP or CHIP48")?
        };

        args.remove(i + 1);
        args.remove(i);

        kind
    } else {
        Default::default()
    };

    let program_name = args.first().ok_or("expected program name")?;
    let program_path = format!("roms/{}.ch8", program_name);

    // vm

    let mut terminal = Terminal::setup(format!(" CHIP8 Virtual Machine ({}) ", program_name), logger_enabled)?;
    let vm = Arc::new(Mutex::new(CHIP8VM { active: true, interp: Interpreter::from_program(program_path, program_kind)?, ..Default::default()}));

    let mut handles: Vec<JoinHandle<Result<(), std::io::Error>>> = vec![];

    { // interp step + interp output handler thread

        let vm = Arc::clone(&vm);
        let mut timer_instant = Instant::now();
        handles.push(spawn_interval("interp", Duration::from_secs_f64(1.0 / INSTRUCTION_FREQUENCY as f64), Duration::from_millis(8), move || {
            let mut vm_guard = vm.lock().unwrap();
            let vm = vm_guard.deref_mut();

            if !vm.active {
                return Ok(IntervalState::Done(()));
            }

            // timer update
            let elapsed = timer_instant.elapsed().as_secs_f64();
            timer_instant = Instant::now();

            // TODO: maybe support sound (right now the sound timer does nothing external)

            vm.sound_timer = (vm.sound_timer - elapsed*TIMER_FREQUENCY as f64).max(0.0);
            vm.delay_timer = (vm.delay_timer - elapsed*TIMER_FREQUENCY as f64).max(0.0);

            // keyboard
            let (pressed_keys, maybe_key_change) = vm.keyboard.update();

            // interp input
            let input = &mut vm.interp_input;

            input.delay_timer = vm.delay_timer.ceil() as u8;
            input.pressed_keys = pressed_keys;
            if let Some((key, is_pressed)) = maybe_key_change {
                if is_pressed {
                    input.just_pressed_key = Some(key);
                } else {
                    input.just_released_key = Some(key);
                }
            }

            // execute next interp instruction
            let output = vm.interp.step(input);

            // interp output
            if let Some(request) = output.request {
                match request {
                    InterpreterRequest::Display => vm.display.update(&output.display),
                    InterpreterRequest::SetDelayTimer(time) => vm.delay_timer = time as f64,
                    InterpreterRequest::SetSoundTimer(time) => vm.sound_timer = time as f64
                }
            }

            // for refreshing terminal to show new log
            if logger_enabled {
                vm.display.refresh();
            }

            // clear ephemeral inputs
            vm.interp_input.just_pressed_key = None;
            vm.interp_input.just_released_key = None;

            Ok(IntervalState::Continue)
        }))
    }

    { // terminal render thread

        let vm = Arc::clone(&vm);
        handles.push(spawn_interval("render", Duration::from_millis(16), Duration::from_millis(16), move || {
            let mut vm = vm.lock().unwrap();

            //vm.display.refresh(); // force trigger (test)

            if vm.active {
                if let Some(buf) = vm.display.extract_new_frame() {
                    drop(vm); // drawing should run concurrently with the vm
                    terminal.draw(&buf)?;
                }

                Ok(IntervalState::Continue)
            } else {
                drop(vm);
                terminal.exit()?;
                Ok(IntervalState::Done(()))
            }
        }))
    }

    { // event handler thread

        let vm = Arc::clone(&vm);
        handles.push(thread::spawn(move || -> Result<(), io::Error> {
            loop {
                if poll(Duration::from_millis(100))? {
                    match read()? {
                        // terminal resize
                        Event::Resize(_, _) => vm.lock().unwrap().display.refresh(),
                        Event::FocusGained => vm.lock().unwrap().keyboard.handle_focus(),
                        Event::FocusLost => vm.lock().unwrap().keyboard.handle_unfocus(),
                        Event::Key(key_event) => {
                            if 
                                key_event.code == CrosstermKey::Esc || 
                                key_event.modifiers.contains(CrosstermKeyModifiers::CONTROL) && (
                                    key_event.code == CrosstermKey::Char('c') || 
                                    key_event.code == CrosstermKey::Char('C')
                                )
                            {
                                vm.lock().unwrap().exit();
                                return Ok(());
                            } else {
                                vm.lock().unwrap().keyboard.handle_poke(); // kinda expecting a crossterm key event to mean terminal is in focus
                            }
                        },
                        _ => ()
                    }
                }
            }
        }))
    }

    for handler in handles {
        handler.join().unwrap()?;
    }

    Ok(())
}

pub enum IntervalState<T> {
    Continue,
    Done(T)
}

fn spawn_interval<F, T, E>(name: &'static str, interval: Duration, max_quantum: Duration, mut f: F) -> JoinHandle<Result<T, E>> 
    where
        F: FnMut() -> Result<IntervalState<T>, E> + Sync,
        F: Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
{
    thread::spawn(move || {
        let mut oversleep_duration = Duration::ZERO;
        let mut control_duration = Duration::ZERO;

        loop {
            let task_start = Instant::now();
            match f() {
                Ok(state) => match state {
                    IntervalState::Continue => {
                        let task_duration = task_start.elapsed();
                        let mut sleep_duration = interval.saturating_sub(task_duration).saturating_sub(oversleep_duration);

                        control_duration += task_duration;
                        if sleep_duration.is_zero() && control_duration < max_quantum {
                            oversleep_duration = Duration::ZERO;
                        } else {
                            if sleep_duration.is_zero() && control_duration >= max_quantum {
                                sleep_duration = Duration::from_millis(1);
                            }

                            let now = Instant::now();
                            
                            // NOTE:
                            // sleeping on windows is ungodly innacurate (~15 ms accuracy) 
                            // but this also increases CPU utilization from nonexistent to around 10% on my machine
                            spin_sleep::sleep(sleep_duration); 
                            
                            oversleep_duration = now.elapsed().saturating_sub(sleep_duration);
                            control_duration = Duration::ZERO;
                        }
                        
                        log::trace!(
                            "name: {}, task: {} us, sleep: {} us, oversleep: {} us", 
                            name,
                            task_duration.as_micros(), 
                            sleep_duration.as_micros(), 
                            oversleep_duration.as_micros()
                        );
                    },
                    IntervalState::Done(result) => return Ok(result)
                },
                Err(e) => return Err(e)
            }
        }
    })
}
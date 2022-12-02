use crate::{
    run::{C8Lock, Interval, IntervalAccuracy},
    config::C8Config,
    dbg::core::{DebuggerWidget, DebuggerWidgetState, Debugger},
    vm::{
        disp::{Display, DisplayWidget}, core::VM,
        
    },
};

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use tui::{
    backend::{CrosstermBackend, Backend},
    style::{Color, Style},
    widgets::{Block, Borders}, Frame,
};

use tui_logger::{TuiLoggerLevelOutput, TuiLoggerWidget};

use std::{
    io::{self, stdout, Result},
    ops::DerefMut, sync::mpsc::{Sender, channel, TryRecvError}, thread::{JoinHandle, self}, time::Duration,
};

type Terminal = tui::Terminal<CrosstermBackend<io::Stdout>>;

pub fn spawn_render_thread(c8: C8Lock, config: C8Config) -> (Sender<()>, JoinHandle<Result<()>>) {
    let (render_sender, render_receiver) = channel::<()>();
    let render_thread_handle = thread::spawn(move || -> Result<()> {
        // change terminal to an alternate screen so user doesnt lose terminal history on exit
        // and enable raw mode so we have full authority over event handling and output
        enable_raw_mode()?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen)?;

        let mut terminal = tui::Terminal::new(CrosstermBackend::new(stdout))?;

        let mut renderer = Renderer {
            dbg_widget_state: Default::default(),
            dbg_visible: false,
            vm_disp: Display::from(config.title.clone()),
            config,
        };

        let mut interval = Interval::new(
            "render",
            Duration::from_millis(16),
            Duration::from_millis(16),
            IntervalAccuracy::Default
        );

        let mut should_redraw = false;

        loop {
            if render_receiver.try_iter().last().is_some() {
                should_redraw = true;
            }

            if let Err(TryRecvError::Disconnected) = render_receiver.try_recv() {
                // clean up the terminal so its usable after program exit
                disable_raw_mode()?;
                execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
                terminal.show_cursor()?;
                return Ok(())
            }

            renderer.step(&mut terminal, should_redraw, &c8)?;
            should_redraw = false;

            interval.sleep();
        }
    });

    (render_sender, render_thread_handle)
}

struct Renderer {
    config: C8Config,
    vm_disp: Display,
    dbg_visible: bool,
    dbg_widget_state: DebuggerWidgetState
}

impl Renderer {
    fn step(&mut self, terminal: &mut Terminal, should_redraw: bool, c8: &C8Lock) -> Result<()> {
        let mut _guard = c8.lock().unwrap();
        let (vm, maybe_dbg) = _guard.deref_mut();

        let did_vm_disp_update = if let Some(buf) = vm.extract_new_frame() {
            self.vm_disp.buffer = buf;
            true
        } else {
            false
        };

        let is_dbg_visible = maybe_dbg.as_ref().map_or(false, Debugger::is_active);
        let should_draw = should_redraw || did_vm_disp_update || is_dbg_visible != self.dbg_visible;

        if should_draw {
            self.dbg_visible = is_dbg_visible;
            if is_dbg_visible {
                let Some(dbg) = maybe_dbg else {
                    unreachable!("debugger must exist for debugger draw call to be made")
                };

                terminal.draw(|f| {
                    self.render_debugger(f, dbg, vm);
                })?;
            } else {
                drop(_guard);
                
                terminal.draw(|f| {
                    self.render_virtual_machine(f);
                })?;
            }
        }

        Ok(())
    }

    fn render_debugger<B: Backend>(&mut self, f: &mut Frame<B>, dbg: &Debugger, vm: &VM) {
        let dbg_area = f.size();
        let dbg_widget = DebuggerWidget {
            dbg,
            vm,
            vm_disp: &self.vm_disp,
            logging: self.config.logging,
        };

        if self.config.logging {
            f.render_widget(logger_widget(), dbg_widget.logger_area(dbg_area));
        }

        if let Some((x, y)) = dbg_widget.cursor_position(dbg_area, &mut self.dbg_widget_state) {
            f.set_cursor(x, y);
        }

        f.render_stateful_widget(dbg_widget, dbg_area, &mut self.dbg_widget_state);
    }

    fn render_virtual_machine<B: Backend>(&self, f: &mut Frame<B>) {
        let display_area = f.size();
        let display_widget = DisplayWidget {
            display: &self.vm_disp,
            logging: self.config.logging,
        };

        if self.config.logging {
            f.render_widget(
                logger_widget(),
                display_widget.logger_area(display_area),
            );
        }

        f.render_widget(display_widget, display_area);
    }
}

fn logger_widget() -> TuiLoggerWidget<'static> {
    TuiLoggerWidget::default()
        .block(
            Block::default()
                .title(" Log ")
                .border_style(Style::default().fg(Color::White))
                .borders(Borders::ALL),
        )
        .output_separator('|')
        .output_timestamp(Some("%H:%M:%S%.3f".to_string()))
        .output_level(Some(TuiLoggerLevelOutput::Abbreviated))
        .output_target(false)
        .output_file(false)
        .output_line(false)
        .style_error(Style::default().fg(Color::Red))
        .style_debug(Style::default().fg(Color::Cyan))
        .style_warn(Style::default().fg(Color::Yellow))
        .style_trace(Style::default().fg(Color::White))
        .style_info(Style::default().fg(Color::Green))
}
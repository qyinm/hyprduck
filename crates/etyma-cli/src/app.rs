#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Jobs,
    Settings,
}

#[derive(Debug)]
pub struct App {
    pub screen: Screen,
    pub should_quit: bool,
}

impl Default for App {
    fn default() -> Self {
        Self {
            screen: Screen::Home,
            should_quit: false,
        }
    }
}

impl App {
    pub fn next_screen(&mut self) {
        self.screen = match self.screen {
            Screen::Home => Screen::Jobs,
            Screen::Jobs => Screen::Settings,
            Screen::Settings => Screen::Home,
        };
    }

    pub fn previous_screen(&mut self) {
        self.screen = match self.screen {
            Screen::Home => Screen::Settings,
            Screen::Jobs => Screen::Home,
            Screen::Settings => Screen::Jobs,
        };
    }
}

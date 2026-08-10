use std::{
    fs,
    io::{self, Read, Stdout},
    sync::LazyLock,
    vec,
};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use tui::{
    Terminal,
    backend::CrosstermBackend,
    layout,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use unicode_width::UnicodeWidthStr;

use crate::{action::Action, dir::Dir, log, options::OPTIONS, template::TemplateEngine, zellij};

pub(crate) fn action_selector(sessions: zellij::Sessions) -> Action {
    let screen = ActionSelectorScreen::new(sessions.clone());
    UI::render(Box::new(screen), sessions)
}

pub(crate) fn new_session_prompt(sessions: zellij::Sessions) -> Action {
    let screen = ChangeCurrentDirPromptScreen;
    UI::render(Box::new(screen), sessions)
}

static BANNERS: LazyLock<Banners> = LazyLock::new(Banners::new);

type Term = tui::Terminal<CrosstermBackend<Stdout>>;
type Frame<'a> = tui::Frame<'a, CrosstermBackend<Stdout>>;

trait Screen {
    fn render(&mut self, term: &mut Term, ctx: &UIContext) -> io::Result<ScreenResult>;
}

struct UI<'a> {
    screen: Box<dyn Screen>,
    context: UIContext<'a>,
}

struct UIContext<'a> {
    cwd: Dir,
    sessions: zellij::Sessions,
    banner: Option<Banner<'a>>,
}

enum ScreenResult {
    NextScreen(Box<dyn Screen>),
    Action(Action),
}

impl<'a> UI<'a> {
    pub(crate) fn render(screen: Box<dyn Screen>, sessions: zellij::Sessions) -> Action {
        let mut ui = Self {
            screen,
            context: UIContext {
                cwd: Dir::cwd(),
                sessions,
                banner: BANNERS.random(),
            },
        };

        ui.run().unwrap_or_else(|error| Action::Exit(Err(error)))
    }

    fn run(&mut self) -> io::Result<Action> {
        terminal::enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut term = Terminal::new(backend)?;

        let result = loop {
            match self.screen.render(&mut term, &self.context) {
                Ok(ScreenResult::NextScreen(screen)) => {
                    self.screen = screen;
                    continue;
                }
                Ok(ScreenResult::Action(action)) => break Ok(action),
                Err(error) => break Err(error),
            }
        };

        terminal::disable_raw_mode()?;
        execute!(
            term.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        term.show_cursor()?;

        result
    }

    fn layout<B: tui::backend::Backend>(
        frame: &mut tui::Frame<B>,
        constraints: &[Constraint],
        banner: &Option<Banner>,
    ) -> Vec<layout::Rect> {
        const VERTICAL_MARGIN: u16 = 6;

        let container = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Percentage(25),
                    Constraint::Percentage(50),
                    Constraint::Percentage(25),
                ]
                .as_ref(),
            )
            .split(frame.size());

        let minimum_content_height = constraints.iter().fold(0_u16, |height, constraint| {
            let constraint_height = match constraint {
                Constraint::Length(height) | Constraint::Min(height) => *height,
                Constraint::Percentage(_) | Constraint::Ratio(_, _) | Constraint::Max(_) => 0,
            };
            height.saturating_add(constraint_height)
        });

        match banner {
            Some(banner)
                if container[1].height
                    >= minimum_content_height
                        .saturating_add(VERTICAL_MARGIN * 2)
                        .saturating_add(banner.len() as u16 + 2) =>
            {
                let banner_constraint = Constraint::Length(banner.len() as u16 + 2);
                let constraints = [&[banner_constraint], constraints].concat();

                let mut layout = Self::build_layout(&constraints, container[1]);

                let header_container = layout.remove(0);

                banner.render(frame, header_container);

                layout
            }
            Some(_) | None => Self::build_layout(constraints, container[1]),
        }
    }

    fn build_layout(constraints: &[Constraint], container: layout::Rect) -> Vec<layout::Rect> {
        Layout::default()
            .direction(Direction::Vertical)
            .horizontal_margin(0)
            .vertical_margin(6)
            .constraints(constraints)
            .split(container)
    }
}

struct Templates(Vec<String>);

impl Templates {
    fn new() -> Self {
        let engine = TemplateEngine::new(&OPTIONS.templates, &OPTIONS.cache);
        match engine.discover() {
            Ok(templates) => Self(
                templates
                    .into_iter()
                    .map(|template| template.name)
                    .collect(),
            ),
            Err(error) => {
                log::warn(format!("Failed to read workspace templates. {error}"));
                Self(Vec::new())
            }
        }
    }

    fn into_vec(self) -> Vec<String> {
        self.0
    }
}

struct Banners(Option<Vec<String>>);

impl Banners {
    fn new() -> Self {
        match &OPTIONS.banners {
            None => Self(None),
            Some(dir) => match Self::read_banners(dir) {
                Ok(banners) => {
                    if banners.is_empty() {
                        log::warn("Directory with banners is empty.");
                        Self(None)
                    } else {
                        Self(Some(banners))
                    }
                }
                Err(error) => {
                    log::warn(format!("Failed to read banners. {}", error));
                    Self(None)
                }
            },
        }
    }

    fn read_banners(dir: &Dir) -> io::Result<Vec<String>> {
        let mut banners = vec![];
        let banner_extension = "banner";

        let files = fs::read_dir(dir)?;

        for file in files.flatten() {
            if let Some(ext) = file.path().extension() {
                if ext == banner_extension {
                    let mut file = fs::File::open(file.path())?;
                    let mut banner = String::new();
                    file.read_to_string(&mut banner)?;
                    banners.push(banner);
                }
            }
        }

        Ok(banners)
    }

    fn padded_lines(banner: &str) -> Vec<Spans<'static>> {
        let width = banner
            .lines()
            .map(UnicodeWidthStr::width)
            .max()
            .unwrap_or_default();

        banner
            .lines()
            .map(|line| {
                let padding = width.saturating_sub(UnicodeWidthStr::width(line));
                Spans::from(Span::raw(format!("{line}{}", " ".repeat(padding))))
            })
            .collect()
    }

    fn random(&self) -> Option<Banner<'_>> {
        match &self.0 {
            None => None,
            Some(banners) => {
                let idx = fastrand::usize(..banners.len());
                let banner = &banners[idx];
                let lines = Self::padded_lines(banner);

                Some(Banner { lines })
            }
        }
    }
}

struct Banner<'a> {
    lines: Vec<Spans<'a>>,
}

impl<'a> Banner<'a> {
    fn len(&self) -> usize {
        self.lines.len()
    }

    fn render<B: tui::backend::Backend>(&self, frame: &mut tui::Frame<B>, container: layout::Rect) {
        let header = Paragraph::new(self.lines.clone()).alignment(Alignment::Center);
        frame.render_widget(header, container)
    }
}

#[cfg(test)]
mod tests {
    use tui::{
        Terminal,
        backend::TestBackend,
        layout::Constraint,
        text::{Span, Spans},
    };

    use super::{ActionSelectorItem, ActionSelectorScreen, Banner, Banners, UI};
    use crate::zellij;

    #[test]
    fn compact_terminal_keeps_essential_selector_rows_visible() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let banner = Some(Banner {
            lines: (0..13).map(|_| Spans::from(Span::raw("banner"))).collect(),
        });
        let mut sections = None;

        terminal
            .draw(|frame| {
                sections = Some(UI::layout(
                    frame,
                    &[Constraint::Length(3), Constraint::Min(3)],
                    &banner,
                ));
            })
            .unwrap();

        let sections = sections.unwrap();
        assert!(sections[0].height >= 3, "input row was squeezed out");
        assert!(sections[1].height >= 3, "selector rows were squeezed out");
    }

    #[test]
    fn banner_rows_share_one_render_width() {
        let lines = Banners::padded_lines("  @@@\n @");

        assert_eq!(lines[0].0[0].content.as_ref(), "  @@@");
        assert_eq!(lines[1].0[0].content.as_ref(), " @   ");
    }

    #[test]
    fn empty_session_list_selects_the_creation_action() {
        let screen = ActionSelectorScreen::new(zellij::Sessions::empty());
        let selected = screen.selector.flush().unwrap().unwrap();

        assert!(matches!(
            selected,
            ActionSelectorItem::NewSession { input: None }
        ));
    }
}

struct Title;

impl Title {
    fn render<'a, T>(text: &'a T, frame: &mut Frame, container: layout::Rect)
    where
        T: Into<Text<'a>> + Clone,
    {
        let title = Paragraph::new(text.clone()).block(Block::default().borders(Borders::BOTTOM));
        frame.render_widget(title, container);
    }
}

struct Input {
    value: String,
    label: String,
}

impl Input {
    fn new(label: impl Into<String>) -> Self {
        Self::with_value(String::new(), label)
    }

    fn with_value(value: String, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
        }
    }

    fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    fn insert(&mut self, char: char) {
        self.value.push(char);
    }

    fn delete(&mut self) {
        self.value.pop();
    }

    fn render(&self, frame: &mut Frame, container: layout::Rect) {
        let input_prefix = "❯ ";

        let input = Paragraph::new(format!("{input_prefix}{input}", input = self.value))
            .style(Style::default().fg(Color::Green))
            .block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .title(Span::styled(
                        &self.label,
                        Style::default().fg(Color::DarkGray),
                    )),
            );

        frame.render_widget(input, container);
        frame.set_cursor(
            container.x + self.value.width() as u16 + input_prefix.width() as u16,
            container.y + 1,
        );
    }
}

struct Prompt<'a, F: Fn(bool) -> ScreenResult> {
    question: Text<'a>,
    selector: Selector<'a, bool>,
    on_select: F,
}

impl<'a, F> Prompt<'a, F>
where
    F: Fn(bool) -> ScreenResult,
{
    pub fn new(question: impl Into<Text<'a>>, on_select: F) -> Self {
        let items = vec![
            SelectorItem {
                value: SelectorValue::Selectable(true),
                label: Span::raw("Yes").into(),
            },
            SelectorItem {
                value: SelectorValue::Selectable(false),
                label: Span::raw("No").into(),
            },
        ];

        Prompt {
            question: question.into(),
            selector: Selector::with_items(items),
            on_select,
        }
    }

    fn draw(&mut self, term: &mut Term, ctx: &UIContext) -> io::Result<()> {
        term.draw(|frame| {
            let layout = UI::layout(
                frame,
                &[Constraint::Length(2), Constraint::Length(2)],
                &ctx.banner,
            );

            let title_container = layout[0];
            let selector_container = layout[1];

            Title::render(&self.question, frame, title_container);
            self.selector.render(frame, selector_container);
        })?;

        Ok(())
    }

    fn listen(&mut self) -> io::Result<EventResult> {
        if let Event::Key(key) = event::read()? {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Ok(EventResult::Exit),
                (KeyCode::Char('y'), _) => {
                    self.selector.select(true);
                    return Ok(EventResult::Return);
                }
                (KeyCode::Char('n'), _) => {
                    self.selector.select(false);
                    return Ok(EventResult::Return);
                }
                (KeyCode::Down, _) => {
                    self.selector.next();
                }
                (KeyCode::Up, _) => {
                    self.selector.previous();
                }
                (KeyCode::Enter, _) => return Ok(EventResult::Return),
                (KeyCode::Esc, _) => return Ok(EventResult::Cancel),
                _ => (),
            }
        }

        Ok(EventResult::Continue)
    }
}

impl<'a, F> Screen for Prompt<'a, F>
where
    F: Fn(bool) -> ScreenResult,
{
    fn render(&mut self, term: &mut Term, ctx: &UIContext) -> io::Result<ScreenResult> {
        loop {
            self.draw(term, ctx)?;
            match self.listen()? {
                EventResult::Continue => (),
                EventResult::Return => {
                    match self.selector.flush() {
                        Err(error) => break Err(io::Error::other(error)),
                        Ok(Some(selection)) => break Ok((self.on_select)(selection)),
                        Ok(None) => continue,
                    };
                }
                EventResult::Cancel => {
                    break Ok(ScreenResult::NextScreen(Box::new(
                        ActionSelectorScreen::new(ctx.sessions.clone()),
                    )));
                }
                EventResult::Exit => break Ok(ScreenResult::Action(Action::Exit(Ok(())))),
            };
        }
    }
}

enum SelectorValue<T> {
    Selectable(T),
    Decortive,
}

struct SelectorItem<'a, T> {
    label: Text<'a>,
    value: SelectorValue<T>,
}

impl<'a, T> SelectorItem<'a, T> {
    pub fn pad() -> Self {
        Self {
            label: " ".into(),
            value: SelectorValue::Decortive,
        }
    }
}

struct Selector<'a, T> {
    state: ListState,
    items: Vec<SelectorItem<'a, T>>,
}

impl<'a, T> Selector<'a, T>
where
    T: PartialEq + Clone,
{
    fn with_items(items: Vec<SelectorItem<'a, T>>) -> Selector<'a, T> {
        let mut state = ListState::default();

        if !items.is_empty() {
            let selection = Self::find_first_selectable(&items, 0, false);
            state.select(selection);
        }

        Self { state, items }
    }

    fn render(&mut self, frame: &mut Frame, container: layout::Rect) {
        let items: Vec<ListItem> = self
            .items
            .iter()
            .map(|i| ListItem::new(i.label.to_owned()))
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::NONE))
            .highlight_style(
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▪ ");

        frame.render_stateful_widget(list, container, &mut self.state);
    }

    fn select(&mut self, value: T) {
        let position = self.items.iter().position(|i| match &i.value {
            SelectorValue::Selectable(v) => v == &value,
            SelectorValue::Decortive => false,
        });
        self.state.select(position);
    }

    fn select_by<F>(&mut self, f: F)
    where
        F: Fn(&T) -> bool,
    {
        let position = self.items.iter().position(|i| match &i.value {
            SelectorValue::Selectable(v) => f(v),
            SelectorValue::Decortive => false,
        });
        self.state.select(position);
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => Self::find_first_selectable(&self.items, i + 1, false).unwrap_or(i),
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(0) => 0,
            Some(i) => Self::find_first_selectable(&self.items, i - 1, true).unwrap_or(i),
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn find_first_selectable(
        items: &Vec<SelectorItem<'a, T>>,
        starting_from: usize,
        backwards: bool,
    ) -> Option<usize> {
        let item = items.get(starting_from);

        match item {
            None => None,
            Some(item) => match item.value {
                SelectorValue::Selectable(_) => Some(starting_from),
                SelectorValue::Decortive => {
                    if (starting_from == 0 && backwards)
                        || (starting_from == items.len() - 1 && !backwards)
                    {
                        None
                    } else {
                        let next_idx = if backwards {
                            starting_from - 1
                        } else {
                            starting_from + 1
                        };
                        Self::find_first_selectable(items, next_idx, backwards)
                    }
                }
            },
        }
    }

    fn flush(&self) -> Result<Option<T>, String> {
        let selected = match self.state.selected() {
            Some(x) => x,
            None => return Ok(None),
        };
        let item = &self.items[selected];
        match &item.value {
            SelectorValue::Selectable(value) => Ok(Some(value.to_owned())),
            SelectorValue::Decortive => Err(format!(
                "Decorative item selected. Label: {:#?}",
                item.label
            )),
        }
    }
}

enum EventResult {
    Continue,
    Return,
    Cancel,
    Exit,
}

#[derive(PartialEq, Clone)]
pub enum ActionSelectorItem {
    Session { name: String },
    NewSession { input: Option<String> },
    Exit,
}

pub struct ActionSelectorScreen<'a> {
    input: Input,
    selector: Selector<'a, ActionSelectorItem>,
}

impl<'a> ActionSelectorScreen<'a> {
    pub fn new(sessions: zellij::Sessions) -> Self {
        let items = Self::build_selector_list(sessions);

        Self {
            input: Input::new("Select session"),
            selector: Selector::with_items(items),
        }
    }

    fn build_selector_list(
        sessions: zellij::Sessions,
    ) -> Vec<SelectorItem<'a, ActionSelectorItem>> {
        let mut items = sessions
            .into_iter()
            .map(|session| SelectorItem {
                label: session.name.clone().into(),
                value: SelectorValue::Selectable(ActionSelectorItem::Session {
                    name: session.name,
                }),
            })
            .collect::<Vec<SelectorItem<ActionSelectorItem>>>();

        if !items.is_empty() {
            items.push(SelectorItem::pad());
            items.push(SelectorItem {
                value: SelectorValue::Decortive,
                label: Span::styled("---", Style::default().fg(Color::DarkGray)).into(),
            });
        }
        items.push(SelectorItem {
            label: " create (or hit Ctrl-N)".into(),
            value: SelectorValue::Selectable(ActionSelectorItem::NewSession { input: None }),
        });
        items.push(SelectorItem {
            label: " exit (or hit Esc)".into(),
            value: SelectorValue::Selectable(ActionSelectorItem::Exit),
        });

        items
    }

    fn draw(&mut self, term: &mut Term, ctx: &UIContext) -> io::Result<()> {
        term.draw(|frame| {
            let layout = UI::layout(
                frame,
                &[Constraint::Length(3), Constraint::Min(3)],
                &ctx.banner,
            );

            let input_container = layout[0];
            let selector_container = layout[1];

            self.input.render(frame, input_container);
            self.selector.render(frame, selector_container);
        })?;

        Ok(())
    }

    fn listen(&mut self, ctx: &UIContext) -> io::Result<EventResult> {
        if let Event::Key(key) = event::read()? {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Ok(EventResult::Exit),
                (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                    self.selector.select_by(|value| match value {
                        ActionSelectorItem::NewSession { input: _ } => true,
                        ActionSelectorItem::Session { .. } | ActionSelectorItem::Exit => false,
                    });
                    return Ok(EventResult::Return);
                }
                (KeyCode::Char(char), _) => {
                    self.input.insert(char);
                    self.filter(ctx);
                }
                (KeyCode::Backspace, _) => {
                    self.input.delete();
                    self.filter(ctx);
                }
                (KeyCode::Down, _) => {
                    self.selector.next();
                }
                (KeyCode::Up, _) => {
                    self.selector.previous();
                }
                (KeyCode::Enter, _) => return Ok(EventResult::Return),
                (KeyCode::Esc, _) => return Ok(EventResult::Exit),
                _ => (),
            }
        }

        Ok(EventResult::Continue)
    }

    fn filter(&mut self, ctx: &UIContext) {
        let next_sessions = if self.input.is_empty() {
            ctx.sessions.to_owned()
        } else {
            ctx.sessions
                .iter()
                .filter_map(|session| {
                    if session.name.contains(&self.input.value) {
                        Some(session.to_owned())
                    } else {
                        None
                    }
                })
                .collect::<zellij::Sessions>()
        };
        let next_items = Self::build_selector_list(next_sessions);
        self.selector = Selector::with_items(next_items);
    }
}

impl<'a> Screen for ActionSelectorScreen<'a> {
    fn render(&mut self, term: &mut Term, ctx: &UIContext) -> io::Result<ScreenResult> {
        loop {
            self.draw(term, ctx)?;
            match self.listen(ctx)? {
                EventResult::Continue => (),
                EventResult::Return => {
                    match self.selector.flush() {
                        Ok(None) => continue,
                        Ok(Some(selection)) => {
                            let result = match selection {
                                ActionSelectorItem::Exit => {
                                    ScreenResult::Action(Action::Exit(Ok(())))
                                }
                                ActionSelectorItem::Session { name: session } => {
                                    ScreenResult::Action(Action::AttachToSession(session))
                                }
                                ActionSelectorItem::NewSession { input: _ } => {
                                    ScreenResult::NextScreen(Box::new(ChangeCurrentDirPromptScreen))
                                }
                            };
                            return Ok(result);
                        }
                        Err(error) => return Err(io::Error::other(error)),
                    };
                }
                EventResult::Cancel | EventResult::Exit => {
                    return Ok(ScreenResult::Action(Action::Exit(Ok(()))));
                }
            }
        }
    }
}

struct ChangeCurrentDirPromptScreen;

impl Screen for ChangeCurrentDirPromptScreen {
    fn render(&mut self, term: &mut Term, ctx: &UIContext) -> io::Result<ScreenResult> {
        let question = Spans(vec![
            Span::raw("Change directory? "),
            Span::styled(
                ctx.cwd.to_string(),
                Style::default().add_modifier(Modifier::DIM),
            ),
        ]);

        let on_select = |should_change_dir| {
            if should_change_dir {
                ScreenResult::NextScreen(Box::new(DirSelectorScreen::new()))
            } else {
                ScreenResult::NextScreen(Box::new(SessionNameScreen::new(
                    SessionNameScreen::default(&ctx.cwd),
                    None,
                )))
            }
        };

        Prompt::new(question, on_select).render(term, ctx)
    }
}

pub struct DirSelectorScreen<'a> {
    input: Input,
    selector: Selector<'a, Dir>,
    dirs: Vec<Dir>,
}

// It would be cool to make this more responsive (debounce the search, move it to bg, etc.)
// It works ok'ish for my use case on my machine,
// but may be slow on less beefy machine with more files/folders
impl<'a> DirSelectorScreen<'a> {
    pub fn new() -> Self {
        use ignore::WalkBuilder;

        let results = WalkBuilder::new(&OPTIONS.root)
            .max_depth(OPTIONS.depth)
            .filter_entry(|e| {
                let path = e.path();

                if !path.is_dir() {
                    return false;
                }

                let dir = path.file_name();

                match dir.and_then(|x| x.to_str()) {
                    None => false,
                    Some(dir) => !OPTIONS.ignore.iter().any(|s| *s == dir),
                }
            })
            .build();

        let mut dirs: Vec<Dir> = match results.size_hint().1 {
            None => vec![],
            Some(n) => Vec::with_capacity(n),
        };

        for entry in results.flatten() {
            let dir: Dir = entry.path().into();
            dirs.push(dir);
        }

        let items = Self::build_selector_list(&dirs);

        Self {
            input: Input::new("Select directory"),
            selector: Selector::with_items(items),
            dirs,
        }
    }

    fn build_selector_list(dirs: &[Dir]) -> Vec<SelectorItem<'a, Dir>> {
        dirs.iter()
            .take(40)
            .map(|dir| SelectorItem {
                label: dir.to_string().into(),
                value: SelectorValue::Selectable(dir.clone()),
            })
            .collect::<Vec<SelectorItem<Dir>>>()
    }

    fn draw(&mut self, term: &mut Term, ctx: &UIContext) -> io::Result<()> {
        term.draw(|frame| {
            let layout = UI::layout(
                frame,
                &[Constraint::Length(3), Constraint::Min(3)],
                &ctx.banner,
            );

            let input_container = layout[0];
            let selector_container = layout[1];

            self.input.render(frame, input_container);
            self.selector.render(frame, selector_container);
        })?;

        Ok(())
    }

    fn listen(&mut self) -> io::Result<EventResult> {
        if let Event::Key(key) = event::read()? {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Ok(EventResult::Exit),
                (KeyCode::Char(char), _) => {
                    self.input.insert(char);
                    self.filter();
                }
                (KeyCode::Backspace, _) => {
                    self.input.delete();
                    self.filter();
                }
                (KeyCode::Down, _) => {
                    self.selector.next();
                }
                (KeyCode::Up, _) => {
                    self.selector.previous();
                }
                (KeyCode::Enter, _) => return Ok(EventResult::Return),
                (KeyCode::Esc, _) => return Ok(EventResult::Cancel),
                _ => (),
            }
        }

        Ok(EventResult::Continue)
    }

    fn filter(&mut self) {
        let next_items = if self.input.is_empty() {
            Self::build_selector_list(&self.dirs)
        } else {
            let dirs = self
                .dirs
                .iter()
                .filter_map(|dir| {
                    if dir
                        .to_string()
                        .to_lowercase()
                        .contains(&self.input.value.to_lowercase())
                    {
                        Some(dir.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<Dir>>();
            Self::build_selector_list(&dirs)
        };
        self.selector = Selector::with_items(next_items);
    }
}

impl<'a> Screen for DirSelectorScreen<'a> {
    fn render(&mut self, term: &mut Term, ctx: &UIContext) -> io::Result<ScreenResult> {
        loop {
            self.draw(term, ctx)?;
            match self.listen()? {
                EventResult::Continue => (),
                EventResult::Return => {
                    match self.selector.flush() {
                        Ok(None) => continue,
                        Ok(Some(dir)) => {
                            let result = ScreenResult::NextScreen(Box::new(
                                SessionNameScreen::new(SessionNameScreen::default(&dir), Some(dir)),
                            ));
                            return Ok(result);
                        }
                        Err(error) => return Err(io::Error::other(error)),
                    };
                }
                EventResult::Cancel => {
                    return Ok(ScreenResult::NextScreen(Box::new(
                        ActionSelectorScreen::new(ctx.sessions.clone()),
                    )));
                }
                EventResult::Exit => return Ok(ScreenResult::Action(Action::Exit(Ok(())))),
            }
        }
    }
}

struct SessionNameScreen {
    input: Input,
    dir: Option<Dir>,
}

impl SessionNameScreen {
    fn new(initial_name: Option<String>, dir: Option<Dir>) -> Self {
        let label = "Give session a name";

        Self {
            input: match initial_name {
                Some(value) => Input::with_value(value, label),
                None => Input::new(label),
            },
            dir,
        }
    }

    fn default(dir: &Dir) -> Option<String> {
        if cfg!(target_os = "windows") {
            dir.filename()
        } else {
            let home = Dir::home();

            if &home == dir {
                Some("~".to_owned())
            } else {
                dir.filename()
            }
        }
    }

    fn draw(&mut self, term: &mut Term, ctx: &UIContext) -> io::Result<()> {
        term.draw(|frame| {
            let layout = UI::layout(
                frame,
                &[Constraint::Length(3), Constraint::Min(1)],
                &ctx.banner,
            );

            let input_container = layout[0];

            self.input.render(frame, input_container);
        })?;

        Ok(())
    }

    fn listen(&mut self) -> io::Result<EventResult> {
        if let Event::Key(key) = event::read()? {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Ok(EventResult::Exit),
                (KeyCode::Char(char), _) => {
                    self.input.insert(char);
                }
                (KeyCode::Backspace, _) => {
                    self.input.delete();
                }
                (KeyCode::Enter, _) => return Ok(EventResult::Return),
                (KeyCode::Esc, _) => return Ok(EventResult::Cancel),
                _ => (),
            }
        }

        Ok(EventResult::Continue)
    }
}

impl Screen for SessionNameScreen {
    fn render(&mut self, term: &mut Term, ctx: &UIContext) -> io::Result<ScreenResult> {
        loop {
            self.draw(term, ctx)?;
            match self.listen()? {
                EventResult::Continue => (),
                EventResult::Return => {
                    match self.input.value.as_str() {
                        "" => continue,
                        _ => {
                            if ctx.sessions.contains(&self.input.value) {
                                return Ok(ScreenResult::NextScreen(Box::new(
                                    AttachToExistingSessionPromptScreen::new(
                                        &self.input.value,
                                        &self.dir,
                                    ),
                                )));
                            } else {
                                let templates = Templates::new().into_vec();
                                if templates.is_empty() {
                                    return Ok(ScreenResult::Action(Action::CreateNewSession {
                                        session: self.input.value.to_owned(),
                                        template: None,
                                        dir: self.dir.to_owned(),
                                    }));
                                }
                                let next_screen = TemplateSelectorScreen::new(
                                    templates,
                                    &self.input.value.to_owned(),
                                    &self.dir,
                                );
                                return Ok(ScreenResult::NextScreen(Box::new(next_screen)));
                            };
                        }
                    };
                }
                EventResult::Cancel => {
                    return Ok(ScreenResult::NextScreen(Box::new(
                        ActionSelectorScreen::new(ctx.sessions.clone()),
                    )));
                }
                EventResult::Exit => {
                    return Ok(ScreenResult::Action(Action::Exit(Ok(()))));
                }
            }
        }
    }
}

struct AttachToExistingSessionPromptScreen {
    session: String,
    dir: Option<Dir>,
}

impl AttachToExistingSessionPromptScreen {
    fn new(session: &str, dir: &Option<Dir>) -> Self {
        Self {
            session: session.to_owned(),
            dir: dir.clone(),
        }
    }
}

impl Screen for AttachToExistingSessionPromptScreen {
    fn render(&mut self, term: &mut Term, ctx: &UIContext) -> io::Result<ScreenResult> {
        let question = Spans(vec![
            Span::raw("Session with the name "),
            Span::styled(
                format!("`{}`", self.session),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(" exists. Attach to it? "),
        ]);

        let on_select = |should_attach| {
            if should_attach {
                ScreenResult::Action(Action::AttachToSession(self.session.to_owned()))
            } else {
                let dir = self.dir.to_owned();

                ScreenResult::NextScreen(Box::new(SessionNameScreen::new(
                    dir.as_ref().and_then(|d| d.filename()),
                    dir,
                )))
            }
        };

        Prompt::new(question, on_select).render(term, ctx)
    }
}

struct TemplateSelectorScreen {
    input: Input,
    selector: Selector<'static, Option<String>>,
    templates: Vec<String>,
    session: String,
    dir: Option<Dir>,
}

impl TemplateSelectorScreen {
    fn new(templates: Vec<String>, session: &str, dir: &Option<Dir>) -> Self {
        let items = Self::build_selector_list(templates.clone());

        Self {
            input: Input::new("Select workspace template"),
            selector: Selector::with_items(items),
            templates,
            session: session.to_owned(),
            dir: dir.clone(),
        }
    }

    fn build_selector_list(templates: Vec<String>) -> Vec<SelectorItem<'static, Option<String>>> {
        let mut items = templates
            .iter()
            .map(|template| SelectorItem {
                label: template.to_string().into(),
                value: SelectorValue::Selectable(Some(template.clone())),
            })
            .collect::<Vec<SelectorItem<Option<String>>>>();

        items.insert(
            0,
            SelectorItem {
                label: Span::styled("[default]", Style::default().add_modifier(Modifier::DIM))
                    .into(),
                value: SelectorValue::Selectable(None),
            },
        );

        items
    }

    fn draw(&mut self, term: &mut Term, ctx: &UIContext) -> io::Result<()> {
        term.draw(|frame| {
            let layout = UI::layout(
                frame,
                &[Constraint::Length(3), Constraint::Min(3)],
                &ctx.banner,
            );

            let input_container = layout[0];
            let selector_container = layout[1];

            self.input.render(frame, input_container);
            self.selector.render(frame, selector_container);
        })?;

        Ok(())
    }

    fn listen(&mut self) -> io::Result<EventResult> {
        if let Event::Key(key) = event::read()? {
            match (key.code, key.modifiers) {
                (KeyCode::Char('c'), KeyModifiers::CONTROL) => return Ok(EventResult::Exit),
                (KeyCode::Char(char), _) => {
                    self.input.insert(char);
                    self.filter();
                }
                (KeyCode::Backspace, _) => {
                    self.input.delete();
                    self.filter();
                }
                (KeyCode::Down, _) => {
                    self.selector.next();
                }
                (KeyCode::Up, _) => {
                    self.selector.previous();
                }
                (KeyCode::Enter, _) => return Ok(EventResult::Return),
                (KeyCode::Esc, _) => return Ok(EventResult::Exit),
                _ => (),
            }
        }

        Ok(EventResult::Continue)
    }

    fn filter(&mut self) {
        let next_items = if self.input.is_empty() {
            Self::build_selector_list(self.templates.to_owned())
        } else {
            let templates = self
                .templates
                .iter()
                .filter_map(|template| {
                    if template.contains(&self.input.value) {
                        Some(template.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<String>>();
            Self::build_selector_list(templates)
        };
        self.selector = Selector::with_items(next_items);
    }
}

impl Screen for TemplateSelectorScreen {
    fn render(&mut self, term: &mut Term, ctx: &UIContext) -> io::Result<ScreenResult> {
        loop {
            self.draw(term, ctx)?;
            match self.listen()? {
                EventResult::Continue => (),
                EventResult::Return => {
                    match self.selector.flush() {
                        Ok(None) => continue,
                        Ok(Some(selection)) => {
                            let result = ScreenResult::Action(Action::CreateNewSession {
                                session: self.session.to_owned(),
                                template: selection,
                                dir: self.dir.to_owned(),
                            });
                            return Ok(result);
                        }
                        Err(error) => return Err(io::Error::other(error)),
                    };
                }
                EventResult::Cancel => {
                    return Ok(ScreenResult::NextScreen(Box::new(
                        ActionSelectorScreen::new(ctx.sessions.clone()),
                    )));
                }
                EventResult::Exit => {
                    return Ok(ScreenResult::Action(Action::Exit(Ok(()))));
                }
            }
        }
    }
}

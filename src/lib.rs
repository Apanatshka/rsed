extern crate regex;

pub mod buffer;
pub mod ui;
pub mod pos;
pub mod cmd;
pub mod util;

use std::result;
use std::fs::File;
use std::path::Path;
use std::io;
use std::convert;

use std::ffi::OsString;

use self::cmd::Cmd;

use pos::Converter;

pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum ErrorType {
    Unknown,
    ParseError,
    IoError(io::Error),
    UnimplementedCmd(Cmd),
    UnimplementedAction(ui::Action),
    InvalidRange(pos::Range),
}

#[derive(Debug)]
pub struct Error {
    msg: String,
    error: ErrorType,
}

impl convert::From<regex::Error> for Error {
    fn from(e: regex::Error) -> Error {
        Error::detailed(ErrorType::ParseError, format!("{}", e))
    }
}

impl convert::From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::new(ErrorType::IoError(e))
    }
}

impl Error {
    pub fn unknown(msg: &str) -> Error {
        Error {
            msg: msg.to_string(),
            error: ErrorType::Unknown,
        }
    }

    pub fn new(error: ErrorType) -> Error {
        Error {
            msg: String::new(),
            error: error,
        }
    }

    pub fn detailed<T: ToString>(error: ErrorType, msg: T) -> Error {
        Error {
            msg: msg.to_string(),
            error: error,
        }
    }
}

#[derive(Debug)]
struct InputInformation {
    position: pos::Pos,
    input_buffer: buffer::Buffer,
}

impl InputInformation {
    fn new(pos: pos::Pos) -> InputInformation {
        InputInformation {
            position: pos,
            input_buffer: buffer::Buffer::new(),
        }
    }
}

#[derive(Debug)]
pub struct Rsed {
    current_buffer: buffer::Buffer,
    input_info: Option<InputInformation>,
    current_line: usize,
    ui: ui::Ui,
    running: bool,
    file_name: Option<OsString>,
}

impl Rsed {
    pub fn new() -> Self {
        Rsed {
            current_buffer: buffer::Buffer::new(),
            input_info: None,
            current_line: 1,
            ui: ui::Ui::new(),
            running: true,
            file_name: None,
        }
    }

    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Rsed> {
        let file_name = path.as_ref().as_os_str().to_os_string();
        let file = try!(File::open(path));
        let reader = io::BufReader::new(file);
        let buffer = try!(buffer::Buffer::from_buf_read(reader));

        Ok(Rsed {
            current_buffer: buffer,
            input_info: None,
            current_line: 1,
            ui: ui::Ui::new(),
            running: true,
            file_name: Some(file_name),
        })
    }

    pub fn read_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let file_name = Some(path.as_ref().as_os_str().to_os_string());

        let file = try!(File::open(path));
        let reader = io::BufReader::new(file);

        self.current_buffer = try!(buffer::Buffer::from_buf_read(reader));
        self.file_name = file_name;

        Ok(())
    }

    pub fn main_loop(&mut self) {

        let stdin = io::stdin();

        while self.running {
            let parsed_action = self.ui.get_input(&stdin);

            let action_result = match parsed_action {
                Ok(action) => self.handle_action(action),
                Err(e) => Err(e),
            };

            if let Err(e) = action_result {
                println!("? {:?}", e);
            }
        }
    }

    fn handle_action(&mut self, action: ui::Action) -> Result<()> {
        use ui::Action::*;

        match action {
            Command(Cmd::Quit) => Ok(self.running = false),
            Command(Cmd::Delete(r)) => self.delete(r),
            Command(Cmd::Print(r, option)) => self.print_range(r, option),
            Command(Cmd::Jump(r)) => self.jump_to(r),
            Command(Cmd::PrintLineNumber(r)) => self.print_line_number(r),
            Command(Cmd::JumpNext) => self.jump_next(),
            Command(Cmd::Edit(f)) => self.read_file(f),
            Command(Cmd::Write(Some(f))) => self.write_file(f),
            Command(Cmd::Write(None)) => self.write_same_file(),
            Command(Cmd::EnterInsertMode(r)) => self.enter_insert_mode(r),
            Command(rest) => Err(Error::new(ErrorType::UnimplementedCmd(rest))),

            Insert(s) => self.insert_line(s),
            InsertEnd => self.end_insert_mode(),
        }
    }

    fn write_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let file_name = Some(path.as_ref().as_os_str().to_os_string());

        // TODO open will crash if file doesn't exist
        let mut file = try!(File::open(path));
        try!(self.current_buffer.write(&mut file));

        self.file_name = file_name;

        Ok(())
    }

    fn write_same_file(&self) -> Result<()> {
        Err(Error::unknown("unimplemented"))
    }

    fn enter_insert_mode(&mut self, r: pos::Range) -> Result<()> {
        if let Some(_) = self.input_info {
            panic!()
        }

        let pos = pos::Pos::from(r);

        self.input_info = Some(InputInformation::new(pos));

        Ok(self.ui.set_mode(ui::Mode::Insert))
    }

    fn insert_line(&mut self, s: String) -> Result<()> {
        if let Some(ref mut input_info) = self.input_info {
            Ok(input_info.input_buffer.add_line(s))
        } else {
            panic!();
        }
    }

    fn end_insert_mode(&mut self) -> Result<()> {

        if let Some(input_info) = self.input_info.take() {
            let pos = self.convert(&input_info.position);
            let input_buffer = input_info.input_buffer;

            self.current_line = pos + input_buffer.len();
            self.current_buffer.insert_buffer(pos, input_buffer);
        } else {
            panic!();
        }

        Ok(self.ui.set_mode(ui::Mode::Command))
    }

    fn print_line_number(&self, r: pos::Range) -> Result<()> {
        let range = r.to_range(self);

        println!("{}", range.end);
        Ok(())
    }

    fn delete(&mut self, r: pos::Range) -> Result<()> {
        let range = r.to_range(self);

        self.current_buffer.delete_lines(range.start, range.end);
        Ok(())
    }

    fn print_range(&self, r: pos::Range, option: ui::PrintOption) -> Result<()> {
        let range = r.to_range(self);

        if self.current_buffer.is_range_out_of_bounds(&range) {
            return Err(Error::new(ErrorType::InvalidRange(r)));
        }

        let model = ui::DisplayModel::new(&self.current_buffer, range, option);

        self.ui.display(model);

        Ok(())
    }

    fn jump_to(&mut self, r: pos::Range) -> Result<()> {
        self.current_line = self.convert(&pos::Pos::from(r));
        self.print_range(pos::Range::current_line(), ui::PrintOption::Normal)
    }

    fn jump_next(&mut self) -> Result<()> {
        if self.current_buffer.is_out_of_bounds(self.current_line + 1) {
            return Err(Error::unknown("invalid line"));
        }

        self.current_line += 1;
        self.print_range(pos::Range::current_line(), ui::PrintOption::Normal)
    }
}

impl Default for Rsed {
    fn default() -> Self {
        Rsed::new()
    }
}

impl<'a> pos::Converter<&'a pos::Pos, usize> for Rsed {
    fn convert(&self, pos: &pos::Pos) -> usize {
        match *pos {
            pos::Pos::Line(n) => n,
            pos::Pos::Current => self.current_line,
            pos::Pos::End => self.current_buffer.len(),
        }
    }
}

use std::env;
use std::process::ExitCode;

const USAGE: &str = r#"oeffi - Simple CLI backbone

Usage:
  oeffi <command>

Commands:
  hello    Print "hello world"
  help     Show this help message

Options:
  -h, --help   Show this help message
"#;

#[derive(Debug)]
enum Command {
    Hello,
    Help,
}

fn parse_command(args: &[String]) -> Result<Command, String> {
    match args {
        [] => Ok(Command::Help),
        [flag] if flag == "-h" || flag == "--help" => Ok(Command::Help),
        [cmd] if cmd == "hello" => Ok(Command::Hello),
        [cmd] if cmd == "help" => Ok(Command::Help),
        [unknown] => Err(format!("Unknown command: '{unknown}'")),
        _ => Err("Too many arguments.".to_string()),
    }
}

fn run(command: Command) -> ExitCode {
    match command {
        Command::Hello => {
            println!("hello world");
            ExitCode::SUCCESS
        }
        Command::Help => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    match parse_command(&args) {
        Ok(command) => run(command),
        Err(message) => {
            eprintln!("Error: {message}\n");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hello() {
        let args = vec!["hello".to_string()];
        assert!(matches!(parse_command(&args), Ok(Command::Hello)));
    }

    #[test]
    fn parses_help_flag() {
        let args = vec!["--help".to_string()];
        assert!(matches!(parse_command(&args), Ok(Command::Help)));
    }

    #[test]
    fn rejects_unknown_command() {
        let args = vec!["nope".to_string()];
        let err = parse_command(&args).unwrap_err();
        assert!(err.contains("Unknown command"));
    }
}
